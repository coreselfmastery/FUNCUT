//! The AUTO EDIT program module.
//! The AUTO EDIT button's whole program: every stage the unattended run
//! can run without a creative brain, written here - beside the adapter
//! that drives it - so the window's pane asks for it and the tests that
//! hold it to its word run where the runner does.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use concat_director::{
    DerivedStep, EditProgram, FanStep, ProgramStep, ReplyGate, Stage, StageStep, ToolCall,
};
use serde_json::{Value, json};

use crate::visual_read::{VisualRead, visual_read_of, visual_reads};
use crate::voice_read::{Entity, LineRead, Motif, VoiceRead};

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
    /// The sibling path of the carried video whose audio the take was
    /// extracted from, when the narration came out of a talking-head
    /// video: the tighten cuts the picture and the words in lockstep, so
    /// they stay in sync. `None` when the take is a plain audio file.
    pub aroll: Option<String>,
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
    /// Suggested export destination for a later explicit render; AUTO EDIT itself leaves the timeline unrendered.
    pub export: String,
    /// The contact sheet's name, relative to the project: the window's
    /// server refuses absolute write paths, and the sheet is written
    /// through it.
    pub sheet: String,
    /// The configured live stock shelf for B-roll. AUTO EDIT never imports
    /// OpenVid's research-only dataset because it has no per-clip licence.
    pub stock_provider: String,
    /// The script the run was handed, when the Forge page carried one: the
    /// authoritative wording the voiceover was read from. It travels to the
    /// voice reading, which aligns the transcript against it - misheard
    /// words corrected to the spelling the author wrote, measured timings
    /// kept - so the captions, the B-roll queries and the emotion read
    /// spend the author's words rather than the recogniser's guesses.
    /// `None` when no script was given: the transcript then stands as it
    /// was transcribed, exactly as it always did.
    pub script: Option<String>,
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
pub(crate) fn media_paths(view: &Value) -> Vec<(String, String)> {
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
pub(crate) fn clip_file_span(clip: &Value) -> (f64, f64, f64) {
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
            // A clip the summary cannot place (no start yet, a view the
            // project has not finished writing) is one clip's gap, not a
            // reason to abandon the whole scan: the same mercy the
            // medialess clip two lines up already gets.
            let Some(start) = clip["start"].as_f64() else {
                continue;
            };
            return Some(start + (moment - source_in) / speed);
        }
    }
    None
}

/// The clip pieces of one take on the current view, as the coverage tiling
/// reads them: `(file_from, file_to, speed, timeline_start)` per piece, in
/// timeline order. A take the tighten split is several pieces; a take not
/// placed is none.
fn take_pieces(view: &Value, media: &[(String, String)], take: &str) -> Vec<(f64, f64, f64, f64)> {
    let mut pieces: Vec<(f64, f64, f64, f64)> = Vec::new();
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
        if path != take {
            continue;
        }
        let (source_in, source_out, speed) = clip_file_span(clip);
        let Some(start) = clip["start"].as_f64() else {
            continue;
        };
        if source_out > source_in {
            pieces.push((source_in, source_out, speed, start));
        }
    }
    pieces.sort_by(|left, right| left.3.total_cmp(&right.3));
    pieces
}

/// Where the nearest file-second the timeline still plays sits at or
/// before `moment`: the exact mapping when the moment survives, else the
/// timeline end of the last piece that ends at or before it. The
/// tighten's `removeSpans` deletes whole file spans, so a slot bound that
/// lands inside a removed pause used to answer None and the shot was
/// silently dropped - the gap the B-roll was planned for was the very
/// span the cut took. Mapping to the surviving edge keeps the slot, at
/// the join the cut made.
fn timeline_at_or_before(
    view: &Value,
    media: &[(String, String)],
    take: &str,
    moment: f64,
) -> Option<f64> {
    if let Some(at) = timeline_at(view, media, take, moment) {
        return Some(at);
    }
    let mut best: Option<(f64, f64)> = None; // (file_to, timeline_end)
    for (source_in, source_out, speed, start) in take_pieces(view, media, take) {
        if source_out <= moment {
            let end = start + (source_out - source_in) / speed;
            if best.is_none_or(|(known, _)| source_out > known) {
                best = Some((source_out, end));
            }
        }
    }
    best.map(|(_, end)| end)
}

/// The mirror of `timeline_at_or_before` for a slot's opening bound: the
/// nearest surviving file-second at or after `moment`, as the timeline
/// start of the first piece that begins at or after it.
fn timeline_at_or_after(
    view: &Value,
    media: &[(String, String)],
    take: &str,
    moment: f64,
) -> Option<f64> {
    if let Some(at) = timeline_at(view, media, take, moment) {
        return Some(at);
    }
    let mut best: Option<(f64, f64)> = None; // (file_from, timeline_start)
    for (source_in, _, _, start) in take_pieces(view, media, take) {
        if source_in >= moment && best.is_none_or(|(known, _)| source_in < known) {
            best = Some((source_in, start));
        }
    }
    best.map(|(_, start)| start)
}

/// The timeline spans the sibling's own picture already covers: every
/// clip whose kind puts pixels on screen - carried A-roll, a placed
/// still, a baked motion card, a title - in timeline seconds. The
/// coverage floor does not shop for what the edit already shows.
fn visual_spans(view: &Value) -> Vec<(f64, f64)> {
    view["timeline"]["clips"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|clip| {
            matches!(
                clip["kind"].as_str(),
                Some("video" | "image" | "motion" | "text" | "shape")
            )
        })
        .filter_map(|clip| {
            let start = clip["start"].as_f64()?;
            let duration = clip["duration"].as_f64()?;
            (duration > 0.05).then_some((start, start + duration))
        })
        .collect()
}

/// The spans of `[floor, ceiling)` the covered spans leave open, merged
/// and in order: what a coverage slot may tile.
fn uncovered_spans(covered: &[(f64, f64)], floor: f64, ceiling: f64) -> Vec<(f64, f64)> {
    let mut open: Vec<(f64, f64)> = Vec::new();
    let mut cursor = floor;
    let mut sorted: Vec<(f64, f64)> = covered
        .iter()
        .copied()
        .filter(|(start, end)| *end > *start)
        .collect();
    sorted.sort_by(|left, right| left.0.total_cmp(&right.0));
    for (start, end) in sorted {
        if start > cursor {
            open.push((cursor, start.min(ceiling)));
        }
        cursor = cursor.max(end.min(ceiling));
        if cursor >= ceiling {
            break;
        }
    }
    if cursor < ceiling {
        open.push((cursor, ceiling));
    }
    open.into_iter()
        .filter(|(start, end)| end - start > 0.3)
        .collect()
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
    let Some(view) = latest_view(log) else {
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
        // The VAD's own account of the take's silences bounds the tighten:
        // a take whose longest measured silence never reached 0.9 s is a
        // tight take whose every silence is its breathing, and the
        // classifier's dead-air word over a span that short is the one
        // mistake that makes an edit feel mechanical - the tighten leaves
        // such a take whole. A take that did hold a second of silence has
        // real gaps, and the classifier's word stands.
        if VoiceRead::of(reply).is_some_and(|read| {
            read.take_wide
                .longest_silence_sec
                .is_some_and(|longest| longest < 0.9)
        }) {
            continue;
        }
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
    // The cuts, in one pass over the clips: each take clip is cut where
    // its own pauses measured, the A-ROLL a talking-head take was
    // extracted from is cut in lockstep (the same file clock, mapped
    // through the video clip's own span, so the picture and the words
    // never drift), and every other visual clip is MOVED by the timeline
    // time the cuts take before it - the lane ripple the take's own lane
    // gets for free, given to the carried picture the same way.
    let mut cuts = Vec::new();
    // The removed spans in TIMELINE seconds, gathered as take clips are
    // cut, in the order the cuts land.
    let mut removed: Vec<(f64, f64)> = Vec::new();
    // The a-roll clips to cut in lockstep: (clip id, spans in the clip's
    // own seconds).
    let mut lockstep: Vec<(String, Vec<Value>)> = Vec::new();
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
        // A talking-head's carried video: cut by the take it was
        // extracted from, in the video's own span. The extraction is the
        // whole file, so the take's file clock is the video's.
        if let Some(aroll) = takes
            .iter()
            .find(|take| take.aroll.as_deref() == Some(path))
            .map(|take| take.take.clone())
        {
            let clip_id = clip["id"].as_str().unwrap_or_default().to_owned();
            let (source_in, source_out, speed) = clip_file_span(clip);
            let start_at = clip["start"].as_f64().unwrap_or(0.0);
            let mut spans: Vec<Value> = Vec::new();
            for (take, rows) in &pauses {
                if *take != aroll {
                    continue;
                }
                for (start, end) in rows {
                    let from = ((start - source_in) / speed).max(0.0);
                    let to = ((end - source_in) / speed).min((source_out - source_in) / speed);
                    if to > from + 0.05 {
                        removed.push((start_at + from, start_at + to));
                        spans.push(json!([round3(from), round3(to)]));
                    }
                }
            }
            if !spans.is_empty() {
                lockstep.push((clip_id, spans));
            }
            continue;
        }
        if !takes.iter().any(|take| take.take == path) {
            continue;
        }
        let (source_in, source_out, speed) = clip_file_span(clip);
        let clip_id = clip["id"].as_str().unwrap_or_default().to_owned();
        let start_at = clip["start"].as_f64().unwrap_or(0.0);
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
                    removed.push((start_at + from, start_at + to));
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
    // The lockstep cuts ride behind the take's own: the same spans, on
    // the video clip, so picture and words move together.
    for (clip_id, spans) in lockstep {
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
    // The carried picture that is not the a-roll - a cutaway, a still, a
    // card - moves with the lanes: every removed timeline second before
    // its start is a second it slides left, exactly the arithmetic the
    // lane's own close_gaps applies.
    if !removed.is_empty() {
        let moves: Vec<Value> = view["timeline"]["clips"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|clip| {
                matches!(
                    clip["kind"].as_str(),
                    Some("video" | "image" | "motion" | "text" | "shape")
                ) && clip["media"]
                    .as_str()
                    .and_then(|id| media.iter().find(|(mid, _)| mid == id))
                    .is_none_or(|(_, path)| {
                        // Not a take, and not a take's a-roll: those are
                        // cut, not moved.
                        !takes.iter().any(|take| take.take == *path)
                            && !takes
                                .iter()
                                .any(|take| take.aroll.as_deref() == Some(path.as_str()))
                    })
            })
            .filter_map(|clip| {
                let start = clip["start"].as_f64()?;
                let shift = removed_before_span(&removed, start);
                if shift < 0.05 {
                    return None;
                }
                Some(json!({
                    "clipId": clip["id"].as_str().unwrap_or_default(),
                    "start": round3((start - shift).max(0.0)),
                    "trackId": clip["track"].as_str().unwrap_or_default(),
                }))
            })
            .collect();
        if !moves.is_empty() {
            cuts.push(ToolCall {
                tool: "edit_apply".to_owned(),
                arguments: json!({
                    "path": project,
                    "command": { "op": "moveClips", "moves": moves },
                }),
            });
        }
    }
    cuts
}

/// How much of the removed spans lies before `at`, counting what they
/// share once - the lane-closing arithmetic, answered for one clip.
fn removed_before_span(removed: &[(f64, f64)], at: f64) -> f64 {
    let mut before: Vec<(f64, f64)> = removed
        .iter()
        .filter(|(start, _)| *start < at)
        .map(|&(start, end)| (start, end.min(at)))
        .collect();
    if before.is_empty() {
        return 0.0;
    }
    before.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut gap = 0.0;
    let (mut from, mut to) = before[0];
    for (start, end) in before.into_iter().skip(1) {
        if start <= to {
            to = to.max(end);
        } else {
            gap += to - from;
            from = start;
            to = end;
        }
    }
    gap + (to - from)
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
    peaks: &[(String, f64, f64)],
    log: &[Value],
) -> Option<f64> {
    let view = latest_view(log)?;
    let media = media_paths(view);
    while *placed < peaks.len() {
        // The peak's own take, never the first take that happens to
        // span the same file-second: a file-second only means anything
        // inside its own take's clock, and a multi-take cut plays both
        // clocks at once.
        let (take, moment, _) = &peaks[*placed];
        if let Some(at) = timeline_at(view, &media, take, *moment) {
            return Some(at);
        }
        *placed += 1;
    }
    None
}

/// The transcript each take's wait left behind: the take file and the
/// segments its words timed. A wait that never landed is not here.
///
/// The voice reading's own lines, when the stage ran, take precedence
/// over the raw transcript: a run handed a script aligns the two inside
/// `voice_signals` - misheard words corrected to the spelling the author
/// wrote, measured timings kept - so every consumer downstream (the
/// captions' text, the queries the B-roll shops with, the emotion read,
/// the plan's own spoken lines) spends the author's words rather than the
/// recogniser's guesses. A take with no voice reading keeps its transcript
/// exactly as it was transcribed, and a voice reply whose lines carry no
/// timings is passed over: it is a filtered lab read, not a replacement
/// for the transcript the captions are timed from.
fn transcripts_of(log: &[Value], takes: &[TakePaths]) -> Vec<(String, Vec<Value>)> {
    let mut found: Vec<(String, Vec<Value>)> = Vec::new();
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
    // The overlay pass: the newest voice reading per take wins, because
    // a reply can only be newer than the transcript it was read from -
    // and a lab re-read of the same take carries the same corrections
    // the profile did, so the last word is also the best one.
    for reply in log {
        let Some(voice) = VoiceRead::of(reply) else {
            continue;
        };
        if !takes.iter().any(|take| take.take == voice.take) {
            continue;
        }
        let Some(rows) = reply["lines"].as_array() else {
            continue;
        };
        // Only a reading whose lines can stand in for a transcript may
        // replace it: each line needs its span (the captions and the
        // slots map every line onto the timeline through its start and
        // end), its text (the queries shop with it) and its timed words
        // (the captions are cut on them). A reply whose lines carry only
        // the prosody - a lab fixture, a filtered read - is passed over:
        // it is not a replacement for the transcript the words came from.
        let timed: Vec<Value> = rows
            .iter()
            .filter(|line| {
                let Some(start) = line["start"].as_f64().filter(|at| *at >= 0.0) else {
                    return false;
                };
                line["end"].as_f64().is_some_and(|to| to > start)
                    && line["text"]
                        .as_str()
                        .is_some_and(|text| !text.trim().is_empty())
                    && line["words"]
                        .as_array()
                        .is_some_and(|words| !words.is_empty())
            })
            .cloned()
            .collect();
        if timed.is_empty() {
            continue;
        }
        match found.iter_mut().find(|(path, _)| *path == voice.take) {
            Some((_, segments)) => *segments = timed,
            None => found.push((voice.take.clone(), timed)),
        }
    }
    found
}

/// One B-roll slot: a stretch of the narration with no words in it, the
/// take it was read from, and the words that describe what the picture
/// should show there.
#[derive(Clone)]
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

/// One B-roll candidate the run has actually looked at: the shelf's own
/// hit, the file the fetch cached, and the frame the contact sheet
/// measured from that file. The tile is the sheet's own reading - luma,
/// contrast, saturation, warmth, the dHash and the crop hint - kept
/// whole so the continuity manifest and the reframe spend the same
/// measurements the choice was made on.
#[derive(Clone)]
struct StockCandidate {
    provider: String,
    id: String,
    url: String,
    /// The shelf's own words for the clip - the caption the search hit
    /// carried. The emotion gate judges candidates on their text; without
    /// this, every candidate's blob was the slot's shared query plus a
    /// tile ordinal, and the gate was deaf to the difference between
    /// them.
    caption: String,
    local_path: String,
    width: f64,
    height: f64,
    tile: Value,
}

impl StockCandidate {
    /// The visual score the sheet measured for this candidate, or `None`
    /// when the frame disqualifies itself. The rejects are the sheet's
    /// own flags - a mostly-black frame, a blown one, a letterboxed one -
    /// and the score is what the pixels carry: chroma first (grey mush
    /// reads as mush however sharp it is), then contrast and detail, with
    /// exposure balance and visual duplicates counted against. A frame
    /// the run already placed (same dHash, different shelf id) is not a
    /// candidate again, so two visually identical clips never cut back
    /// to back.
    fn visual_score(&self, used_hashes: &[String]) -> Option<f64> {
        let tile = &self.tile;
        let black = tile["blackFraction"].as_f64().unwrap_or(0.0);
        let blown = tile["blownFraction"].as_f64().unwrap_or(0.0);
        let bars = tile["barRatio"].as_f64().unwrap_or(0.0);
        if black > 0.55 || blown > 0.35 || bars > 0.6 {
            return None;
        }
        let saturation = tile["saturation"].as_f64().unwrap_or(0.0);
        let contrast = tile["contrast"].as_f64().unwrap_or(0.0);
        let detail = tile["detail"].as_f64().unwrap_or(0.0);
        let luma = tile["luma"].as_f64().unwrap_or(128.0);
        let mut score =
            saturation * 2.0 + contrast / 64.0 + detail / 8.0 - (luma - 128.0).abs() / 128.0;
        let hash = tile["hash"].as_str().unwrap_or_default();
        if !hash.is_empty() && used_hashes.iter().any(|seen| seen == hash) {
            score -= 3.0;
        }
        Some(score)
    }

    /// The warmth the sheet measured, on the -1 cool through +1 warm
    /// scale the continuity gate's colour-jump rule is written on. An
    /// unmeasured warmth is not a neutral zero: `None` keeps the gate's
    /// own honest standing-down for unmeasured colour.
    fn warmth(&self) -> Option<f64> {
        self.tile["warmth"]
            .as_f64()
            .filter(|warmth| warmth.is_finite() && (-1.0..=1.0).contains(warmth))
    }
}

/// The doors the bed's search walks, in the order they outrank each
/// other: the emotional read's own `bgmWords` for the dominant line (the
/// mood and the act name the genre - the three-word primary first, the
/// bare category behind it), then the reading's language hint the profile
/// derived from the script, and last the browse - a shelf that is ready
/// is never left unheard because three mood words matched no whole title.
/// `None` in the chain is the browse door: a search with no query.
fn bed_query_chain(log: &[Value], takes: &[TakePaths]) -> Vec<Option<String>> {
    let mut chain: Vec<Option<String>> = Vec::new();
    if let Some(line) = dominant(&emotion_lines(log, takes)) {
        chain.extend(
            line.bgm_words
                .iter()
                .filter(|words| !words.trim().is_empty())
                .cloned()
                .map(Some),
        );
    }
    chain.extend(
        voice_reads(log, takes)
            .into_iter()
            .map(|read| read.take_wide.music_genre)
            .filter(|genre| !genre.trim().is_empty())
            .map(Some),
    );
    chain.extend(
        log.iter()
            .filter_map(|reply| reply.pointer("/language/musicGenreHint"))
            .filter_map(|hint| hint.as_str())
            .filter(|hint| !hint.trim().is_empty())
            .map(|hint| Some(hint.to_owned())),
    );
    // Deduped: a hint that repeats the read's own words is one door, not
    // two walks of the same empty answer.
    let mut seen: Vec<String> = Vec::new();
    chain.retain(|door| match door {
        Some(query) => {
            if seen.iter().any(|spent| spent == query) {
                false
            } else {
                seen.push(query.clone());
                true
            }
        }
        None => true,
    });
    chain.push(None);
    chain
}

/// The bed that fits the cut, from the page the shelf answered with:
/// among the tracks long enough to cover the runtime, the shortest one
/// (least waste); when none covers, the longest (most coverage). A track
/// whose duration the census could not measure is never guessed at -
/// `None`, and the caller keeps the shelf's own first answer.
fn best_bed(tracks: &[Value], runtime: f64) -> Option<Value> {
    let measured: Vec<(f64, &Value)> = tracks
        .iter()
        .filter_map(|track| {
            let seconds = track["seconds"].as_f64()?;
            (seconds.is_finite() && seconds > 0.0).then_some((seconds, track))
        })
        .collect();
    if measured.is_empty() {
        return None;
    }
    let covers: Vec<&(f64, &Value)> = measured
        .iter()
        .filter(|(seconds, _)| *seconds >= runtime)
        .collect();
    let chosen: &(f64, &Value) = if covers.is_empty() {
        // Nothing covers: the longest covers the most.
        measured
            .iter()
            .max_by(|left, right| left.0.total_cmp(&right.0))?
    } else {
        // Something covers: the shortest of those wastes the least.
        covers
            .iter()
            .min_by(|left, right| left.0.total_cmp(&right.0))?
    };
    Some((*chosen.1).clone())
}

/// The shelf hits a slot will actually be looked at through: the ones
/// with an explicit provider licence, long enough to cover the span,
/// facing the way the project's frame faces, and not already chosen -
/// in the shelf's own relevance order, at most `look_at` of them. The
/// sheet - not this function - reorders them by what the pixels
/// measured.
fn fitting_hits(
    hits: &[Value],
    span: f64,
    aspect: &str,
    used: &[String],
    look_at: usize,
) -> Vec<Value> {
    let wants_vertical = matches!(aspect, "9:16" | "4:5");
    let wants_horizontal = aspect == "16:9";
    hits.iter()
        .filter(|hit| {
            let id = hit["id"].as_str().unwrap_or_default();
            let url = hit["url"].as_str().unwrap_or_default();
            let license = hit["license"].as_str().unwrap_or_default();
            if id.trim().is_empty() || url.trim().is_empty() || license.trim().is_empty() {
                return false;
            }
            if used.iter().any(|seen| seen == id) {
                return false;
            }
            let width = hit["width"].as_f64().unwrap_or(0.0);
            let height = hit["height"].as_f64().unwrap_or(0.0);
            if height > 0.0 && width > 0.0 {
                if wants_vertical && height < width {
                    return false;
                }
                if wants_horizontal && width < height {
                    return false;
                }
            }
            let seconds = hit["seconds"].as_f64().unwrap_or(0.0);
            seconds.is_finite() && seconds > 0.0 && seconds + 0.05 >= span
        })
        .take(look_at)
        .cloned()
        .collect()
}

/// The crop hint's offsets, as a `setClipTransform` the fill can spend -
/// only on the axis that actually crops, and only when the source is at
/// least as wide (or tall) as the square the hint's arithmetic assumes:
/// a wider source overhangs more than the hint spends, so following it
/// moves the subject toward centre without leaving the frame, while a
/// narrower one overhangs less and the hint could push past it.
fn reframe_offsets(tile: &Value, aspect: &str, width: f64, height: f64) -> Option<(f64, f64)> {
    let transform = tile.pointer("/reframeHint/clipTransform")?;
    let offset_x = transform["offsetX"].as_f64().unwrap_or(0.0);
    let offset_y = transform["offsetY"].as_f64().unwrap_or(0.0);
    let portrait = matches!(aspect, "9:16" | "4:5" | "1:1");
    let (x, y) = if portrait {
        if width < height {
            return None;
        }
        (offset_x, 0.0)
    } else {
        if height < width {
            return None;
        }
        (0.0, offset_y)
    };
    (x.abs() >= 0.05 || y.abs() >= 0.05).then_some((x, y))
}

/// One word of a caption group: the word as the recogniser heard it, the
/// times it measured for it, and the character offset the word holds in
/// the group's own shown text - the number the title's range styling
/// counts.
struct CaptionWord {
    text: String,
    start: f64,
    end: f64,
    at: usize,
}

/// One caption group: two to four words cut on the delivery's own seams,
/// the span they occupy in the take's file seconds, and every word's
/// place in the text - so the accent pass can style the word that
/// carried the line without re-deriving anything.
struct CaptionGroup {
    /// The take file the group was read from.
    take: String,
    start: f64,
    end: f64,
    text: String,
    words: Vec<CaptionWord>,
}

/// The caption groups one transcript segment carries: the recogniser's
/// own word times, chunked where a viewer reads a seam - the sentence's
/// own punctuation, a breath gap the recogniser measured, four words,
/// or three and a half seconds, whichever comes first. The safe-remove
/// fillers the profile timed are dropped here, so the viewer never sees
/// the `um` they already heard.
fn caption_groups(segment: &Value, fillers: &[String]) -> Vec<CaptionGroup> {
    fn close(groups: &mut Vec<CaptionGroup>, current: &mut Vec<CaptionWord>, text: &mut String) {
        if current.is_empty() {
            text.clear();
            return;
        }
        let start = current.first().map(|word| word.start).unwrap_or(0.0);
        let end = current.last().map(|word| word.end).unwrap_or(start);
        groups.push(CaptionGroup {
            take: String::new(),
            start,
            end,
            text: std::mem::take(text),
            words: std::mem::take(current),
        });
    }

    let mut groups: Vec<CaptionGroup> = Vec::new();
    let mut current: Vec<CaptionWord> = Vec::new();
    let mut text = String::new();
    for word in segment["words"].as_array().into_iter().flatten() {
        let Some(word_text) = word["text"].as_str().map(str::trim) else {
            continue;
        };
        let (Some(start), Some(end)) = (word["start"].as_f64(), word["end"].as_f64()) else {
            continue;
        };
        if word_text.is_empty() || end <= start {
            continue;
        }
        let bare = word_text
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if !bare.is_empty() && fillers.iter().any(|filler| filler == &bare) {
            continue;
        }
        // A breath gap the recogniser measured is a seam the reader
        // reads: the group closes before the gap, not across it.
        if current.last().is_some_and(|last| start - last.end > 0.6) {
            close(&mut groups, &mut current, &mut text);
        }
        if !text.is_empty() {
            text.push(' ');
        }
        let at = text.chars().count();
        text.push_str(word_text);
        let ends_sentence =
            word_text.ends_with('.') || word_text.ends_with('!') || word_text.ends_with('?');
        current.push(CaptionWord {
            text: word_text.to_owned(),
            start,
            end,
            at,
        });
        let span = end - current.first().map(|first| first.start).unwrap_or(end);
        if ends_sentence || current.len() >= 4 || span >= 3.5 {
            close(&mut groups, &mut current, &mut text);
        }
    }
    close(&mut groups, &mut current, &mut text);
    groups
}

/// Where the shopping of one B-roll slot stands. The stage shops every
/// slot first - search, fetch, look, choose - so the continuity gate
/// can judge the whole picture run before anything is imported, and a
/// flagged slot swaps to an alternate the sheet already measured rather
/// than re-shopping the shelf.
enum GatherPhase {
    /// About to search the shelf for the slot being shopped.
    Search,
    /// The search reply is in: the eligible hits to look at are next.
    ReadSearch,
    /// About to fetch the next candidate into the cache.
    Fetch,
    /// The fetch reply is in: the next fetch, or the sheet.
    ReadFetch,
    /// About to look at the fetched candidates through a contact sheet.
    Sheet,
    /// The sheet reply is in: rank, choose and record the slot.
    ReadSheet,
    /// About to measure the ranked candidates' pictures, not just their
    /// frames: motion, camera travel, scale, quality and scene cuts.
    Visual,
    /// The visual reply is in: a candidate the frames refuse sinks, then
    /// the emotion gate weighs the register.
    ReadVisual,
    /// The sheet's choice is ranked: the emotion gate weighs the
    /// register before the choice is spent.
    Gate,
    /// The gate's reply is in: the first candidate the sheet ranked
    /// highest among the ones the gate passed takes the slot, or the
    /// beat is dropped.
    ReadGate,
    /// About to run the continuity gate over the chosen run.
    Continuity,
    /// The continuity reply is in: repair what the measurements flagged.
    ReadContinuity,
    /// About to import the next chosen candidate.
    Import,
    /// The import reply is in: the lane every B-roll clip shares is
    /// minted once, before the first placement.
    ReadTrack,
    /// The import reply is in: place the clip at its gap.
    ReadImport,
    /// The placement reply is in: style the clip and move on.
    Place,
}

/// One slot's shopped choice: the candidates the sheet ranked, best
/// first, and which of them currently holds the slot. The continuity
/// pass advances `chosen` (a colour jump) or holds the slot (a category
/// run); the import pass spends whichever candidate holds it.
struct SlotChoice {
    /// The slot's stable id, the one the continuity gate's flags name.
    id: String,
    /// The gap itself, as the planner read it.
    slot: BrollSlot,
    /// The looked-at candidates, the sheet's ranking first.
    candidates: Vec<StockCandidate>,
    /// Which candidate holds the slot.
    chosen: usize,
    /// A slot the continuity pass downgraded: the A-roll holds its span
    /// and nothing is imported for it.
    held: bool,
    /// The continuity gate found the clip too still for its slot. The
    /// drift is the remedy, so the shot pushes even though the visual
    /// read would otherwise leave an already-moving frame alone.
    force_push: bool,
}

impl SlotChoice {
    /// Advance to the next alternate the sheet measured as a different
    /// picture: a new clip id, a new frame hash, and not a clip another
    /// slot already chose. No such alternate leaves the flag standing,
    /// recorded by the check's own second pass.
    fn swap_past(
        &mut self,
        used_ids: &[String],
        used_hashes: &[String],
    ) -> Option<(String, String)> {
        let current = &self.candidates[self.chosen];
        let current_hash = current.tile["hash"].as_str().unwrap_or_default().to_owned();
        let current_id = current.id.clone();
        for step in 1..self.candidates.len() {
            let at = (self.chosen + step) % self.candidates.len();
            let candidate = &self.candidates[at];
            let hash = candidate.tile["hash"].as_str().unwrap_or_default();
            if candidate.id == current_id || hash == current_hash {
                continue;
            }
            if used_ids.iter().any(|seen| seen == &candidate.id) {
                continue;
            }
            if used_hashes.iter().any(|seen| seen == hash) {
                continue;
            }
            self.chosen = at;
            // The spend travels with the swap: two flagged slots used to
            // be able to swap onto the SAME alternate, because neither
            // recorded it as used - both then imported the same clip and
            // the place pass styled only the first.
            return Some((candidate.id.clone(), hash.to_owned()));
        }
        None
    }
}

/// The continuity gate's manifest, from the measurements the sheet made
/// of the choices: the warmth each chosen frame read, and the slot's own
/// query words as its category - the plan's vocabulary, the same thing
/// the watcher's manifest carries. Nothing the pixels did not say is
/// written: an unmeasured warmth is omitted, never a neutral zero.
fn continuity_manifest(shopped: &[SlotChoice], reads: &[VisualRead]) -> Vec<Value> {
    shopped
        .iter()
        .filter(|shop| !shop.held)
        .map(|shop| {
            let candidate = &shop.candidates[shop.chosen];
            let mut entry = json!({
                "slotId": shop.id,
                "clipId": candidate.id,
                "category": category_of(&shop.slot.query),
            });
            if let Some(warmth) = candidate.warmth() {
                entry["colorTemperature"] = json!(warmth);
            }
            // The picture's own read fills the three fields the manifest
            // used to leave empty because a contact sheet measures one
            // frame: how much moves, which way it travels and how wide
            // the shot is. The gate's motion, scale and stillness rules
            // stand on these; without them it could only judge colour.
            if let Some(read) = visual_read_of(reads, &candidate.local_path) {
                if let Some(mean) = read.motion_mean {
                    // The library's own motion scale is 0-131; the read's
                    // mean is a fraction of the work width per frame.
                    entry["motion"] = json!(round3((mean * 131.0).clamp(0.0, 131.0)));
                    // Below 4 of 131 is a still frame by the library's own
                    // scale: the gate flags it and the drift answers. A
                    // clip nobody measured keeps the gate's floor at zero
                    // rather than being judged as still.
                    entry["motionMin"] = json!(4.0);
                }
                if let Ok(direction) = serde_json::to_value(read.direction()) {
                    entry["dominantDirection"] = direction;
                }
                if let Some(scale) = read.shot_scale()
                    && let Ok(scale) = serde_json::to_value(scale)
                {
                    entry["shotScale"] = scale;
                }
            }
            entry
        })
        .collect()
}

/// Where the eye lands in the picture under a timeline second: the video
/// clip covering the moment, joined to its media path and the visual
/// read the gather measured. `None` when no measured clip covers it, so
/// the caption keeps the band the frame's aspect gave it rather than
/// guessing from a shot nobody read.
fn visual_focus_at(log: &[Value], at: f64) -> Option<String> {
    let view = latest_view(log)?;
    let media = media_paths(view);
    let reads = visual_reads(log);
    let clip = view["timeline"]["clips"].as_array()?.iter().find(|clip| {
        let start = clip["start"].as_f64().unwrap_or(0.0);
        let duration = clip["duration"].as_f64().unwrap_or(0.0);
        clip["kind"].as_str() == Some("video") && start <= at + 0.05 && at < start + duration
    })?;
    let id = clip["media"].as_str()?;
    let path = media
        .iter()
        .find(|(media_id, _)| media_id == id)
        .map(|(_, path)| path.as_str())?;
    reads
        .iter()
        .find(|read| read.path == path)
        .and_then(|read| read.saliency_focus())
        .map(str::to_owned)
}

/// A query's coarse category: the longest word the plan shopped the slot
/// with, which for a planner slot is its topic tag and for a plain gap
/// is the most content-bearing word before it. Longest rather than
/// first, because a plain gap's query begins with the article the
/// sentence ended on.
fn category_of(query: &str) -> String {
    query
        .split_whitespace()
        .filter(|word| word.chars().any(|c| c.is_alphanumeric()))
        .max_by_key(|word| word.chars().count())
        .unwrap_or_default()
        .to_lowercase()
}

/// The cut-in a gap earns, read from the same measurements the old
/// inline choice read: the mood the line before the gap carried, the
/// seam's own reading (a speaker change, a question, a held beat, a
/// chapter, a pivot), and the no-repeat rule against the last move.
/// The seam's own readings, named so the precedence below reads as a
/// sentence rather than a tuple of anonymous booleans.
#[derive(Default)]
struct SeamRead {
    crosses_speaker: bool,
    asks: bool,
    exclaim: bool,
    fragment: bool,
    holds: bool,
    scene_turn: bool,
    dramatic: bool,
    beat: bool,
    breath: bool,
    chapter: bool,
    pivot: bool,
    phrase: bool,
}

fn seam_transition(
    voice: &[VoiceRead],
    chapters: &[TopicBoundary],
    pivots: &[(String, f64)],
    emotion: &[EmotionLine],
    slot: &BrollSlot,
    last: Option<&'static str>,
) -> &'static str {
    let mood = emotion_at(emotion, &slot.take, slot.from)
        .map(|line| line.emotion.clone())
        .unwrap_or_else(|| "neutral".to_owned());
    let seam = {
        let read = voice_read_of(voice, &slot.take);
        let pause_class = read.and_then(|read| read.pause_class_at(slot.from));
        SeamRead {
            crosses_speaker: read.is_some_and(|read| {
                // The speakers' own shares gate the change points: a
                // change the diarization found in a take one voice holds
                // (a second speaker under a fifth of the speech) is a
                // clustering wobble, not a turn, and a single-voice
                // take's seams belong to the phrase and the question -
                // the changes are only spent where the take is a real
                // conversation. Unmeasured shares keep the changes: the
                // one reading is not the other's contradiction.
                read.conversation() != Some(false)
                    && read
                        .take_wide
                        .speaker_changes
                        .iter()
                        .any(|change| *change > slot.from - 0.35 && *change < slot.to + 0.35)
            }),
            asks: read.is_some_and(|read| {
                read.line_at(slot.from).is_some_and(|line| {
                    line.sentence_type == "question"
                        || line.rise_confirms == Some(true)
                        // The acoustic terminal rise stands in where the
                        // punctuation carried no question to confirm: the
                        // ear heard the ask even when the text did not
                        // write it.
                        || line.final_rise == Some(true)
                })
            }),
            holds: read.is_some_and(|read| read.keeps_pause_at(slot.from)),
            // The silence classes are distinct moves: a scene turn is
            // the topic moving, a dramatic beat is a held reveal, a
            // beat is a small breath in the argument, a breath is a
            // soft phrase boundary. A class with no entry falls through
            // to the mood's own move.
            scene_turn: pause_class == Some("scene_turn"),
            dramatic: pause_class == Some("dramatic"),
            beat: pause_class == Some("beat"),
            breath: pause_class == Some("breath"),
            // A sentence's own shape is a seam too: an exclamation lands
            // hard, a fragment is a thought that trails off.
            exclaim: read.is_some_and(|read| {
                read.line_at(slot.from)
                    .is_some_and(|line| line.sentence_type == "exclamation")
            }),
            fragment: read.is_some_and(|read| {
                read.line_at(slot.from)
                    .is_some_and(|line| line.sentence_type == "fragment")
            }),
            // A phrase boundary opens or closes in the gap: the
            // narration's own breath-group turned, a seam the picture can
            // register even where no speaker changed and no question was
            // asked.
            phrase: read.is_some_and(|read| read.phrase_seam(slot.from, 0.15)),
            // A chapter opens in this gap: the strongest seam the script
            // has, and the one the chapter card is already naming.
            chapter: chapters.iter().any(|boundary| {
                boundary.take == slot.take
                    && boundary.at > slot.from - 0.5
                    && boundary.at < slot.to + 0.5
            }),
            // A connective pivot inside the gap: the argument turned here,
            // so the picture turns with it.
            pivot: pivots.iter().any(|(take, at)| {
                take == &slot.take && *at > slot.from - 0.5 && *at < slot.to + 0.5
            }),
        }
    };
    let first = if seam.chapter {
        // A chapter opening is the clearest seam there is: the story
        // moved on, and the whip is the move that says so.
        "concat.whip-pan"
    } else if seam.pivot {
        // A connective pivot is the argument turning: the slice is the
        // move that makes the turn read.
        "concat.slice-social"
    } else if seam.scene_turn {
        // The profile heard the topic turn in the silence itself.
        "concat.whip-pan"
    } else if seam.crosses_speaker {
        // A speaker change is the clearest voice seam: the frame
        // changes because the voice did.
        "concat.glitch-social"
    } else if seam.asks {
        // A question opens a gap the answer fills: the snap is the
        // ask, not the mood.
        "concat.snap-zoom"
    } else if seam.exclaim {
        // An exclamation lands hard; the crash is the sentence's own
        // punctuation, not an emotion guessed from it.
        "concat.crash-zoom"
    } else if seam.fragment {
        // A fragment trails off, so the move stays light.
        "concat.cross-pop"
    } else if seam.dramatic {
        // A dramatic beat is a held reveal, so it takes the softest move.
        "concat.dissolve"
    } else if seam.holds {
        // A held beat keeps the softest move the catalogue has, so the
        // cut never fights the silence the read said to hold.
        "concat.dissolve"
    } else if seam.beat {
        // A small beat in the argument gets a flash, not a dissolve:
        // the pause is a punctuation mark, not a scene change.
        "concat.flash-social"
    } else if seam.breath {
        // A breath is the lightest seam: a soft pop keeps it visible
        // without making it a scene change.
        "concat.cross-pop"
    } else if seam.phrase {
        // A phrase boundary is the weakest structural seam: a visible
        // move, but only when nothing stronger - a hold, a turn -
        // claims the gap.
        "concat.cross-pop"
    } else {
        transition_for_emotion(&mood)
    };
    if last != Some(first) {
        return first;
    }
    // The seam's move is the one the last shot already wore. The mood's
    // alternate differs from the mood's own first, not from a seam that
    // may not be it - two chapter openings on a neutral read are whip-pan
    // twice - so the alternate can repeat as well. The mood's primary
    // move is the fallback: the alternate and the primary never agree, so
    // one of the two is always a move the last shot did not wear.
    let alternate = alternate_transition(&mood);
    if last == Some(alternate) {
        transition_for_emotion(&mood)
    } else {
        alternate
    }
}

/// The timeline's extent over the narration's own clips - the number a
/// bed must be trimmed to and a runtime read must not inflate. The bed
/// itself (a clip whose media lives in the project's bgm/ folder) is
/// excluded, because the question is always "how long is the speech",
/// asked both before and after a bed was placed.
fn narration_runtime(log: &[Value]) -> f64 {
    latest_view(log)
        .and_then(|view| {
            let media = media_paths(view);
            view["timeline"]["clips"].as_array().map(|clips| {
                clips
                    .iter()
                    .filter(|clip| {
                        clip["media"]
                            .as_str()
                            .and_then(|id| media.iter().find(|(mid, _)| mid == id))
                            .map(|(_, path)| !path.contains("/bgm/"))
                            .unwrap_or(false)
                    })
                    .map(|clip| {
                        clip["start"].as_f64().unwrap_or(0.0)
                            + clip["duration"].as_f64().unwrap_or(0.0)
                    })
                    .fold(0.0_f64, f64::max)
            })
        })
        .unwrap_or(0.0)
}

/// The bed's beats, translated from the timeline's clock (the bed plays
/// from zero, so its file seconds are timeline seconds) into one take's
/// own file seconds - the clock that take's transcript segments and
/// planner suggestions are read in. A beat outside the take's placements
/// is dropped: it belongs to another take or to silence.
fn beats_in_take_clock(beats: &Value, source: &AutoEditProgramSource, take: &str) -> Value {
    let rows = beats.as_array().cloned().unwrap_or_default();
    let translated: Vec<Value> = rows
        .iter()
        .filter_map(|beat| {
            let at = beat["seconds"].as_f64()?;
            let strength = beat["strength"].as_f64().unwrap_or(0.5);
            let placement = source.placements.iter().find(|placed| {
                placed.take == take && {
                    let duration = (placed.source_out - placed.source_in) / placed.speed;
                    (placed.start..placed.start + duration).contains(&at)
                }
            })?;
            let in_take = (at - placement.start) * placement.speed + placement.source_in;
            Some(json!({ "seconds": round3(in_take), "strength": strength }))
        })
        .collect();
    Value::Array(translated)
}

/// The take whose own audio already carries music, if any. The music
/// read echoes the path it was asked about, so a reply that carries
/// `musicPresent` names its take directly - a call that failed leaves
/// no reply behind, and pairing by the order the calls were issued in
/// would then name the wrong take as the one that already carries
/// music, with the bed decision for the real one wrong beside it. A
/// reply with no echo (a sidecar older than the field) still pairs the
/// way it always did: positionally, in `takes` order.
fn musical_take(log: &[Value], takes: &[TakePaths]) -> Option<String> {
    for reply in log {
        let Some(path) = reply.get("audio").and_then(Value::as_str) else {
            // An echo-less reply pairs positionally, below, the way it
            // always did.
            continue;
        };
        if reply
            .get("musicPresent")
            .and_then(Value::as_bool)
            .unwrap_or(false)
            && takes.iter().any(|take| take.take == path)
        {
            return Some(path.to_owned());
        }
    }
    let reads: Vec<bool> = log
        .iter()
        .filter_map(|reply| {
            if reply.get("audio").and_then(Value::as_str).is_some() {
                // A keyed reply was already paired above; only the
                // echo-less ones pair positionally.
                return None;
            }
            reply.get("musicPresent").and_then(Value::as_bool)
        })
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
/// takes. `analyze_beats` is a job: the grid itself lands in the wait's
/// own reply, `output` carrying the analysis JSON with the beats in
/// milliseconds under `beatsMs` (onsets the fallback), or - where a cut
/// suggestion answered instead - cut times in seconds under `cutTimes`
/// on the reply itself. No measured grid is `None`, which leaves the
/// monotony read standing down rather than guessing.
fn latest_beats(log: &[Value]) -> Option<Value> {
    let seconds: Vec<f64> = log
        .iter()
        .rev()
        .filter_map(|reply| {
            // A direct reply from the cut suggester: seconds, on the reply.
            let cuts = reply
                .get("cutTimes")
                .and_then(Value::as_array)
                .filter(|cuts| !cuts.is_empty())
                .map(|cuts| cuts.iter().filter_map(Value::as_f64).collect::<Vec<f64>>());
            if cuts.is_some() {
                return cuts;
            }
            // The wait's reply: the analysis JSON as a string, its beats
            // in milliseconds.
            let output = reply.get("output")?.as_str()?;
            let parsed: Value = serde_json::from_str(output).ok()?;
            let from_ms = |key: &str| {
                parsed[key]
                    .as_array()
                    .map(|beats| {
                        beats
                            .iter()
                            .filter_map(|ms| ms.as_f64())
                            .map(|ms| ms / 1000.0)
                            .collect::<Vec<f64>>()
                    })
                    .filter(|beats| !beats.is_empty())
            };
            from_ms("beatsMs").or_else(|| from_ms("onsetsMs"))
        })
        .next()?;
    Some(Value::Array(
        seconds
            .into_iter()
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

/// The B-roll slots a run should fill: a coverage floor first, the
/// planner's asks on top of it.
///
/// The floor is structural, not creative: a voiceover-only sibling has
/// no picture of its own, so every second the timeline does not already
/// show a visual clip is a second that would render as the project
/// background - the black-span defect the one-click edit shipped for
/// years. `coverage_slots` tiles those seconds at a shot-change cadence
/// the runtime sets, skipping what the sibling's own A-roll, cards and
/// titles already cover, so carried footage is never re-shopped and the
/// planner's refusals stand wherever a visual actually plays.
///
/// The planner's own read wins where it asked: `slots_from_plan` reads
/// `footage_plan_suggest`'s suggestions - dialogue gaps AND monotony
/// runs, each carrying the priority the planner scored it at - and a
/// planned slot replaces the floor's tile over the same span. An
/// explicit `needBRoll: false` still means no planned slot; the floor
/// only spends where the timeline shows nothing, which is a question
/// the planner was never asked.
///
/// The plain gap scan remains as the last resort: a run whose planner
/// never answered and whose view carries no take pieces (a transcript
/// with no timeline behind it) still fills its measured gaps.
fn broll_slots(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
    let floor = coverage_slots(log, takes);
    let planned = slots_from_plan(log, takes);
    // The gap scan runs only when the planner never answered AND the view
    // carries no take pieces to tile: an explicit `needBRoll: false` is
    // the planner's own answer, and a second, less-informed scan never
    // overrules it.
    let planner_answered = log.iter().any(|reply| {
        reply.get("take").and_then(Value::as_str).is_some()
            && reply
                .get("suggestions")
                .and_then(Value::as_array)
                .is_some_and(|suggestions| !suggestions.is_empty())
    });
    if !planner_answered && floor.is_empty() && planned.is_empty() {
        return broll_slots_from_gaps(log, takes);
    }
    // The planner's asks replace the floor's tiles over the same span;
    // the floor keeps tiling everywhere else. A floor tile that overlaps
    // any planned slot by more than a beat is dropped, so one span is
    // never shopped twice.
    let merged: Vec<BrollSlot> = planned
        .into_iter()
        .chain(
            floor
                .into_iter()
                .filter(|tile| !planned_spans_overlap(log, takes, tile)),
        )
        .collect();
    if merged.is_empty() {
        return merged;
    }
    // Play order across takes, first-play order of takes, then local
    // time within each take - the order the gather walks them in.
    let take_order: Vec<&str> = takes.iter().map(|take| take.take.as_str()).collect();
    let mut ordered = merged;
    ordered.sort_by(|left, right| {
        let rank = |slot: &BrollSlot| {
            take_order
                .iter()
                .position(|take| *take == slot.take.as_str())
                .unwrap_or(usize::MAX)
        };
        rank(left)
            .cmp(&rank(right))
            .then(left.from.total_cmp(&right.from))
    });
    ordered
}

/// Whether any planner suggestion covers this floor tile: a span the
/// planner already scored is its business, not the floor's.
fn planned_spans_overlap(log: &[Value], takes: &[TakePaths], tile: &BrollSlot) -> bool {
    const OVERLAP: f64 = 0.35;
    log.iter()
        .filter(|reply| {
            reply.get("take").and_then(Value::as_str).is_some()
                && reply.get("suggestions").and_then(Value::as_array).is_some()
        })
        .any(|plan| {
            let take = plan["take"].as_str().unwrap_or_default();
            if !takes.iter().any(|known| known.take == take) || take != tile.take {
                return false;
            }
            plan["suggestions"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|suggestion| {
                    suggestion.get("needBRoll").and_then(Value::as_bool) != Some(false)
                })
                .any(|suggestion| {
                    let from = suggestion["startS"].as_f64().unwrap_or(f64::MAX);
                    let to = suggestion["endS"].as_f64().unwrap_or(f64::MIN);
                    to > tile.from + OVERLAP && from < tile.to - OVERLAP
                })
        })
}

/// The structural moments a take measured, that a coverage tile's bound
/// may snap to: the pauses (where the speaker breathed), the speaker
/// changes (where the voice itself changed - only populated when the
/// take is a real conversation), the phrase boundaries (the narration's
/// own breath-groups), the stressed words (the delivery's own beats) and
/// the flux onsets (the transients, a word's own attack). The order the
/// snap spends them in is the order a human editor would: the scene
/// change first, the phrase turn next, the stressed word, the breath,
/// the attack last.
#[derive(Clone, Default)]
struct SnapBounds {
    /// Pause starts, in file seconds, sorted.
    pauses: Vec<f64>,
    /// Speaker-change times, in file seconds, sorted. Empty on a
    /// single-voice take.
    speaker_changes: Vec<f64>,
    /// Phrase-boundary starts, in file seconds, sorted: the narration's
    /// own breath-groups, the seam a shot can register even where no
    /// pause was classified.
    phrases: Vec<f64>,
    /// Stressed-word times, in file seconds, sorted: the delivery's own
    /// beats, a place a cut can land where the voice itself leaned.
    emphasis: Vec<f64>,
    /// Spectral-flux onsets, in file seconds.
    onsets: Vec<f64>,
}

impl SnapBounds {
    /// The strongest structural moment near `target` that stays past
    /// `floor` (the earliest bound a tile of honest length allows): a
    /// speaker change inside the breath window first, then a phrase
    /// boundary, then a stressed word, then a pause, then an onset inside
    /// the tighter transient window. `None` when no measured moment is
    /// near enough - the bound then stays where the cadence put it, which
    /// is an arbitrary frame but an honest one.
    fn snap(&self, target: f64, floor: f64, window: f64, onset_window: f64) -> Option<f64> {
        let nearest = |moments: &[f64], win: f64| {
            moments
                .iter()
                .copied()
                .filter(|at| (*at - target).abs() <= win && *at > floor)
                .min_by(|left, right| (left - target).abs().total_cmp(&(right - target).abs()))
        };
        // A speaker change is the strongest boundary the take has: a new
        // voice is a new scene, and the shot owes it a change of angle.
        nearest(&self.speaker_changes, window)
            // The phrase boundary: the narration's own breath-group
            // turned, so the picture can turn with it.
            .or_else(|| nearest(&self.phrases, window))
            // The stressed word: the delivery leaned here, so a cut that
            // lands on it reads as the edit's own beat.
            .or_else(|| nearest(&self.emphasis, window))
            // The breath: the shot changes where the speaker paused, and
            // a bound that lands mid-word is the one thing the tiling
            // must never do.
            .or_else(|| nearest(&self.pauses, window))
            // The transient: a cut that lands on an attack reads as
            // intentional in a way a cut twenty frames later never does.
            .or_else(|| nearest(&self.onsets, onset_window))
    }
}

/// The coverage floor: contiguous slots tiling every second of every
/// placed take that the sibling's own picture does not already show, at
/// a cadence the runtime sets. The bounds are FILE seconds - the gather
/// maps them onto the timeline the same way it maps the planner's - and
/// the tiling is built on the CURRENT view, so the tighten's cuts are
/// already taken: a dead-air span the tighten removed is not a span at
/// all, and the tiles close over the join.
///
/// Cadence: a reel cuts every three seconds; a piece longer than two
/// minutes widens the target up to a 6.5-second documentary pace, and
/// the tiling never passes forty shots - a run stays a run, not a
/// stock-shelf crawl. A piece the cadence tiles into fewer shots than
/// that simply is that short: no floor forces machine-gun cuts onto a
/// ten-second hook.
fn coverage_slots(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
    const CADENCE: f64 = 3.0;
    const CADENCE_CEILING: f64 = 6.5;
    const MOST: usize = 40;
    const SHORTEST_TILE: f64 = 1.6;
    const SNAP_WINDOW: f64 = 0.8;
    // The transient window is tighter than the breath window: an onset
    // is a word's attack, so a bound snapped to one from further away
    // would land inside the word the pause snap exists to protect.
    const ONSET_WINDOW: f64 = 0.3;
    const QUERY_WORDS: usize = 6;

    let Some(view) = latest_view(log) else {
        return Vec::new();
    };
    let media = media_paths(view);
    let covered = visual_spans(view);
    let read = emotion_lines(log, takes);
    // The structural moments every take measured, for snapping tile
    // bounds: the breaths the pauses classified, the seams a speaker
    // change opened (only when the shares say the take is a real
    // conversation - a lone wobble in a single-voice take is not a
    // scene change), and the transients the flux found. A tile bound is
    // a cut, and the cut wants the strongest structural moment near it:
    // who changed, then where the speaker breathed, then the nearest
    // attack - an arbitrary frame is the last resort, never the choice.
    let voice = voice_reads(log, takes);
    let pauses: Vec<(String, SnapBounds)> = voice
        .iter()
        .map(|voice| {
            let conversation = voice.conversation().filter(|real| *real);
            let mut speaker_changes = conversation
                .map(|_| voice.take_wide.speaker_changes.clone())
                .unwrap_or_default();
            if conversation.is_some() {
                // A turn edge is the exchange's own seam even when the
                // change detector missed it: the second speaker taking
                // the floor is the scene change, whatever the clustering
                // reported between the two.
                for turn in &voice.take_wide.speaker_turns {
                    speaker_changes.push(turn.start);
                    speaker_changes.push(turn.end);
                }
            }
            let mut bounds = SnapBounds {
                pauses: voice.pauses.iter().map(|pause| pause.start).collect(),
                speaker_changes,
                phrases: voice
                    .take_wide
                    .phrases
                    .iter()
                    .map(|phrase| phrase.start)
                    .collect(),
                emphasis: voice
                    .lines
                    .iter()
                    .flat_map(|line| line.emphasis.iter().map(|word| word.at))
                    .collect(),
                onsets: voice.take_wide.flux_onsets.clone(),
            };
            bounds.pauses.sort_by(|left, right| left.total_cmp(right));
            bounds
                .speaker_changes
                .sort_by(|left, right| left.total_cmp(right));
            bounds
                .speaker_changes
                .dedup_by(|left, right| (*left - *right).abs() < 1.0e-3);
            bounds.phrases.sort_by(|left, right| left.total_cmp(right));
            bounds.emphasis.sort_by(|left, right| left.total_cmp(right));
            (voice.take.clone(), bounds)
        })
        .collect();

    // First pass: the uncovered file spans of every take piece, and the
    // total uncovered runtime they carry - the number the cadence and
    // the budget are set from.
    let mut runs: Vec<(String, Vec<(f64, f64)>)> = Vec::new();
    let mut uncovered_total = 0.0_f64;
    for take in takes {
        let pieces = take_pieces(view, &media, &take.take);
        if pieces.is_empty() {
            continue;
        }
        let mut open: Vec<(f64, f64)> = Vec::new();
        for (source_in, source_out, speed, start) in pieces {
            // The piece's timeline span: what the covered spans are
            // read against, then mapped back into this piece's file
            // seconds.
            let timeline_from = start;
            let timeline_to = start + (source_out - source_in) / speed;
            let piece_open = uncovered_spans(&covered, timeline_from, timeline_to);
            for (open_from, open_to) in piece_open {
                let file_from = source_in + (open_from - timeline_from) * speed;
                let file_to = source_in + (open_to - timeline_from) * speed;
                open.push((file_from, file_to));
            }
        }
        // Merge across the pieces of one take: two pieces abut when the
        // tighten took nothing between them, and a tile may span the
        // join.
        open.sort_by(|left, right| left.0.total_cmp(&right.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (from, to) in open {
            match merged.last_mut() {
                Some(last) if from <= last.1 + 1e-6 => last.1 = last.1.max(to),
                _ => merged.push((from, to)),
            }
        }
        uncovered_total += merged.iter().map(|(from, to)| to - from).sum::<f64>();
        if !merged.is_empty() {
            runs.push((take.take.clone(), merged));
        }
    }
    if runs.is_empty() {
        return Vec::new();
    }
    // The cadence: a reel cuts every three seconds; a piece longer than
    // two minutes cannot afford forty shots at that pace, so the target
    // widens with the runtime up to a documentary 6.5 s, and the shot
    // count the tiling implies never passes forty.
    let target = if uncovered_total <= CADENCE * MOST as f64 {
        CADENCE
    } else {
        (uncovered_total / MOST as f64).min(CADENCE_CEILING)
    };

    // Second pass: tile each uncovered run at the cadence, snapping
    // bounds to the strongest measured moment, and shop each tile with the
    // words it carries.
    let mut slots: Vec<BrollSlot> = Vec::new();
    for (take, open) in &runs {
        let bounds = pauses
            .iter()
            .find(|(path, _)| path == take)
            .map(|(_, bounds)| bounds)
            .cloned()
            .unwrap_or_default();
        let spoken = transcripts_of(log, takes)
            .into_iter()
            .find(|(path, _)| path == take)
            .map(|(_, segments)| {
                let mut spans: Vec<(f64, f64, String)> = segments
                    .iter()
                    .filter_map(|segment| {
                        let start = segment["start"].as_f64()?;
                        let end = segment["end"].as_f64()?;
                        let text = segment["text"].as_str().unwrap_or_default().trim();
                        (end > start && !text.is_empty()).then(|| (start, end, text.to_owned()))
                    })
                    .collect();
                spans.sort_by(|left, right| left.0.total_cmp(&right.0));
                spans
            })
            .unwrap_or_default();
        for (from, to) in open.iter().copied() {
            let mut from = from;
            // A run's tail is never left as a hole: the last tile absorbs
            // whatever is shorter than a shot of its own, and a run too
            // short to split at all is one short tile rather than an
            // uncovered second.
            while to - from > 0.5 && slots.len() < MOST {
                let mut next = (from + target).min(to);
                // Snap the bound to the strongest measured moment near it:
                // the shot changes where the structure is - a speaker
                // change, a breath, a transient - and a bound that lands
                // mid-word is the one thing the tiling must never do.
                if let Some(at) =
                    bounds.snap(next, from + SHORTEST_TILE * 0.5, SNAP_WINDOW, ONSET_WINDOW)
                {
                    next = at;
                }
                // The last tile of a run takes the rest, however long -
                // the alternative is a hole, and a hole is the defect
                // this function exists to end.
                if to - next < SHORTEST_TILE {
                    next = to;
                }
                if next <= from + 1e-6 {
                    break;
                }
                let query = {
                    let take_read = voice.iter().find(|read| read.take == *take);
                    // A tile over a line the confidence gate could not
                    // hear, or one with almost no voice under it, has no
                    // words worth shopping: a neutral establishing shot
                    // is more honest than a query built on a misheard
                    // phrase.
                    let uncertain = take_read.is_some_and(|read| {
                        read.unsure_between(from, next).len() >= 2
                            || read
                                .lines
                                .iter()
                                .find(|line| line.end > from && line.start < next)
                                .and_then(|line| line.voiced_ratio)
                                .is_some_and(|ratio| ratio < 0.2)
                    });
                    if uncertain {
                        "cinematic establishing shot".to_owned()
                    } else if let Some(query) = take_read.and_then(|read| {
                        // A named organisation, person, date or place
                        // is a stronger subject than the words around
                        // it: the shot shows who or where the line is
                        // about, not merely the sentence's verb.
                        read.entities
                            .iter()
                            .filter(|entity| entity.at.is_some_and(|at| at >= from && at < next))
                            .find_map(entity_stock_query)
                    }) {
                        query
                    } else if let Some(query) = take_read
                        .and_then(|read| {
                            // A repeated phrase returns with the visual it
                            // established: the callback is the motif's own
                            // query, not a fresh interpretation of the words.
                            read.motifs.iter().find(|motif| {
                                motif.line_indices.iter().any(|index| {
                                    read.lines
                                        .get(*index)
                                        .is_some_and(|line| line.end > from && line.start < next)
                                })
                            })
                        })
                        .map(|motif| card_words(&motif.phrase))
                        .filter(|query| !query.trim().is_empty())
                    {
                        query
                    } else {
                        // The emotional read's own vocabulary for the line
                        // the tile sits in - the register x act shot the
                        // extract computed is the query candidates arrive
                        // pre-matched by.
                        let register = read
                            .iter()
                            .filter(|line| {
                                line.take == *take && line.end > from && line.start < next
                            })
                            .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
                            .and_then(|line| line.queries.first().cloned())
                            .unwrap_or_default();
                        if !register.trim().is_empty() {
                            register
                        } else {
                            // The tile's own words: what the narration says
                            // while this shot plays is what the shot should
                            // show.
                            let mut words: Vec<&str> = spoken
                                .iter()
                                .filter(|(_, end, _)| *end > from)
                                .take_while(|(start, _, _)| *start < next)
                                .flat_map(|(_, _, text)| text.split_whitespace())
                                .collect();
                            words.truncate(QUERY_WORDS);
                            let joined = words.join(" ");
                            if !joined.is_empty() {
                                joined
                            } else {
                                // A silent tile with no words behind it: the
                                // words that led into it, the way the gap
                                // scan always shopped.
                                spoken
                                    .iter()
                                    .rev()
                                    .find(|(_, end, _)| *end <= from + 1e-6)
                                    .map(|(_, _, text)| tail_words(text, QUERY_WORDS))
                                    .filter(|tail| !tail.trim().is_empty())
                                    .unwrap_or_else(|| {
                                        // Nothing measured anything here; a
                                        // neutral establishing shot is the
                                        // honest last resort.
                                        "cinematic establishing shot".to_owned()
                                    })
                            }
                        }
                    }
                };
                slots.push(BrollSlot {
                    take: take.clone(),
                    from: round3(from),
                    to: round3(next),
                    query,
                });
                from = next;
            }
        }
    }
    slots
}

/// The slots the footage planner asked for, in the order it ranked them.
/// The planner reads monotony as well as gaps - a long stretch of speech
/// no beat lands in wants coverage even though it is not silent - and it
/// scores each span, so the strongest asks are filled first and a run
/// that can only afford a few fills spends them where the plan says.
fn slots_from_plan(log: &[Value], takes: &[TakePaths]) -> Vec<BrollSlot> {
    const LEAST: usize = 6;
    const MOST: usize = 40;
    const CADENCE: f64 = 3.0;
    const SHORTEST: f64 = 1.2;
    const LONGEST: f64 = 8.0;
    // The budget scales with the runtime the view carries, by the same
    // law the coverage floor tiles at: a six-shot cap on a two-minute
    // reel starves the picture, and an unbounded one is a shelf crawl.
    let budget = latest_view(log)
        .and_then(|view| {
            view["timeline"]["clips"].as_array().map(|clips| {
                clips
                    .iter()
                    .map(|clip| {
                        clip["start"].as_f64().unwrap_or(0.0)
                            + clip["duration"].as_f64().unwrap_or(0.0)
                    })
                    .fold(0.0_f64, f64::max)
            })
        })
        .map(|runtime| ((runtime / CADENCE).ceil() as usize).clamp(LEAST, MOST))
        .unwrap_or(LEAST);

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
            // The emotional read's own vocabulary for the line the slot
            // sits in: the register x act shot the extract computed
            // (docs/EMOTION.md's 5a) is the query candidates arrive
            // pre-matched by - a topic tag shops the subject, the
            // register shops the mood, and the line the slot overlaps
            // names both.
            let register = emotion_lines(log, takes)
                .iter()
                .filter(|line| line.take == take_path && line.end > start && line.start < end)
                .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
                .and_then(|line| line.queries.first().cloned())
                .unwrap_or_default();
            let before = spoken
                .iter()
                .filter(|(_, segment_end, _)| *segment_end <= start)
                .max_by(|left, right| left.1.total_cmp(&right.1))
                .map(|(_, _, text)| tail_words(text, 5))
                .unwrap_or_default();
            let query = if !tags.is_empty() {
                tags.join(" ")
            } else if !register.trim().is_empty() {
                register
            } else {
                before
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

    // Spend the runtime-scaled budget on the planner's strongest asks,
    // then restore first-play take order and local time within each take.
    ranked.sort_by(|left, right| {
        right
            .0
            .total_cmp(&left.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.from.total_cmp(&right.2.from))
    });
    ranked.truncate(budget);
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
        aroll: None,
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
    /// The stock-search vocabulary the line's register asks for: the
    /// extract's own `queries`, emotion register x act shot, in priority
    /// order - what a B-roll slot overlapping this line shops with.
    queries: Vec<String>,
    /// The delivery's own curve, as the extract emitted it (the bezier
    /// x1/y1/x2/y2 of "tense eases in, warm settles out"): the caption
    /// entrance keys ride it. Springs and unread lines take the settle.
    easing: Vec<f64>,
    /// The line's own words, as the transcript measured them: a motion
    /// card carries the narration it was earned by, never a sample's text.
    text: String,
    /// How fast the line is delivered, as `voice_signals` measured it:
    /// punchy, conversational, editorial or slow. Empty when the reading
    /// carried no rate, which is the honest answer for a take measured
    /// without word timings.
    pace: String,
    /// The emotion engine's own confidence in this line's read, 0..1. The
    /// lexicon and the transformer answer at different certainties, and a
    /// signal log that rated an `emotion` entry by `voice_signals`'
    /// vocabulary alone could not rate it at all.
    confidence: Option<f64>,
}

impl EmotionLine {
    /// The line as an `emotion` extract wrote it, from a take file, with
    /// the words the transcript measured for it. The confidence is the
    /// extract's own reply-level read (the lexicon and the hosted model
    /// answer at different certainties): the engine never emits it per
    /// line, so a row-level value - when one day it does - outranks the
    /// reply's, and a signal log that rated an `emotion` entry by
    /// `voice_signals`' vocabulary alone could not rate it at all.
    fn of(
        take: &str,
        row: &Value,
        text: &str,
        pace: &str,
        read: Option<f64>,
    ) -> Option<EmotionLine> {
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
            queries: row["queries"]
                .as_array()
                .map(|words| {
                    words
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default(),
            easing: row
                .pointer("/easing/curve")
                .filter(|curve| curve["kind"] == json!("bezier"))
                .map(|curve| {
                    [
                        curve["x1"].as_f64(),
                        curve["y1"].as_f64(),
                        curve["x2"].as_f64(),
                        curve["y2"].as_f64(),
                    ]
                })
                .map(|parts| {
                    parts
                        .iter()
                        .map(|part| part.unwrap_or_default())
                        .collect::<Vec<f64>>()
                })
                .filter(|ease| ease.len() == 4 && ease.iter().all(|v| v.is_finite()))
                .unwrap_or_else(default_ease),
            text: text.to_owned(),
            pace: pace.to_owned(),
            confidence: row["confidence"].as_f64().or(read),
        })
    }
}

/// How fast the take's delivery is at a span, as `voice_signals` measured
/// it: the line of its reading that overlaps the span most carries the
/// rate. Empty when the reply measured no rate there, so a caller can tell
/// "measured slow" from "not measured" - a stage that invented a pace
/// would be claiming evidence it never read.
fn delivery_pace(log: &[Value], take: &str, start: f64, end: f64) -> String {
    let mut best: Option<(f64, String)> = None;
    for reply in log {
        if reply.pointer("/audio/path").and_then(Value::as_str) != Some(take) {
            continue;
        }
        for line in reply["lines"].as_array().into_iter().flatten() {
            let (Some(line_start), Some(line_end)) = (line["start"].as_f64(), line["end"].as_f64())
            else {
                continue;
            };
            // The profile's pace class is the primary reading; a foreign
            // reply that carries only words-per-minute still gets a pace
            // from the same 180/115 bars the producer uses.
            let pace = line
                .pointer("/speechRate/pace")
                .and_then(Value::as_str)
                .filter(|pace| !pace.trim().is_empty())
                .map(str::to_owned)
                .or_else(|| {
                    line.pointer("/speechRate/wordsPerMinute")
                        .and_then(Value::as_f64)
                        .map(|wpm| pace_from_wpm(wpm).to_owned())
                });
            let Some(pace) = pace else {
                continue;
            };
            let overlap = line_end.min(end) - line_start.max(start);
            if overlap <= 0.0 {
                continue;
            }
            if best
                .as_ref()
                .is_none_or(|(best_overlap, _)| overlap > *best_overlap)
            {
                best = Some((overlap, pace));
            }
        }
    }
    best.map(|(_, pace)| pace).unwrap_or_default()
}

/// The pace class a words-per-minute reading falls in, on the producer's
/// own bars: fast delivery at 180 and over, slow at 115 and under.
fn pace_from_wpm(wpm: f64) -> &'static str {
    if wpm >= 180.0 {
        "punchy"
    } else if wpm <= 115.0 {
        "slow"
    } else {
        "conversational"
    }
}

/// The words the take's own delivery stressed, lowercased and
/// deduplicated: `voice_signals`' `prosody.emphasisWords`, the signal the
/// caption stage spends. Empty when no reply measured any, which is the
/// honest answer for a take read without prosody - the stage then falls
/// back to its own evidence rather than inventing a stress.
fn stressed_words(log: &[Value], takes: &[TakePaths]) -> Vec<(String, Vec<String>)> {
    voice_reads(log, takes)
        .into_iter()
        .map(|read| {
            let mut words: Vec<String> = Vec::new();
            for word in read
                .lines
                .into_iter()
                .flat_map(|line| line.emphasis.into_iter().map(|word| word.word))
            {
                if !words.contains(&word) {
                    words.push(word);
                }
            }
            (read.take, words)
        })
        .collect()
}

/// The emphasis peaks the sound stage may spend, read from the same typed
/// profile as captions and pacing. A raw walk through lines[].prosody can
/// drift independently of the parser the other consumers use.
fn emphasis_peaks(reads: &[VoiceRead]) -> Vec<(String, f64, f64)> {
    reads
        .iter()
        .flat_map(|read| {
            read.lines.iter().flat_map(move |line| {
                line.emphasis.iter().map(move |word| {
                    // An accent lands on a transient, not on a word's
                    // nominal start: when the flux ear measured one
                    // nearby, the hit moves to it.
                    let at = read.onset_near(word.at, 0.12).unwrap_or(word.at);
                    (read.take.clone(), at, word.strength)
                })
            })
        })
        .collect()
}

/// Every take's `voice_signals` reading, in the order the replies landed.
/// A reply that is not a reading (a transcript, an emotion extract, a
/// timeline view) is skipped, and a reading of a file the run does not
/// play is skipped too: the profile answers about the takes this run
/// edits, and a stale reply from another session is not evidence.
fn voice_reads(log: &[Value], takes: &[TakePaths]) -> Vec<VoiceRead> {
    log.iter()
        .filter_map(VoiceRead::of)
        .filter(|read| takes.iter().any(|take| take.take == read.take))
        .collect()
}

/// The take's reading, when one landed.
fn voice_read_of<'a>(reads: &'a [VoiceRead], take: &str) -> Option<&'a VoiceRead> {
    reads.iter().find(|read| read.take == take)
}

/// A topic boundary the discourse tiling found: where one chapter ends and
/// the next begins, in the take's own seconds, with the words that name
/// the new chapter.
#[derive(Clone, Debug)]
struct TopicBoundary {
    /// The take the lines were read from.
    take: String,
    /// Where the new chapter opens, in file seconds.
    at: f64,
    /// The words the tiling says the chapter is about.
    headline: String,
    /// The tiling's own confidence: the segment's cohesion, 0..1. A
    /// chapter card cut on a weakly cohesive segment is the tiling's
    /// guess, and the log should say so rather than imply a certainty.
    confidence: Option<f64>,
}

/// The replies of one discourse action, in the order the stage asked for
/// them. The stage issues one call per take, so the replies answer the
/// takes in order - the same positional contract `emotion_lines` uses.
fn discourse_replies<'a>(log: &'a [Value], action: &str) -> Vec<&'a Value> {
    log.iter()
        .filter(|reply| reply["tool"].as_str() == Some(action))
        .collect()
}

/// The chapter boundaries the run measured, one take at a time. The
/// `segment_topics` reply names a boundary by the line it follows, so the
/// line's own start is where the new chapter opens - a boundary index the
/// transcript cannot resolve is dropped rather than guessed at second
/// zero, which would put every chapter card at the head of the piece.
fn topic_boundaries(log: &[Value], takes: &[TakePaths]) -> Vec<TopicBoundary> {
    let asked: Vec<&TakePaths> = takes
        .iter()
        .filter(|take| !spoken_lines(log, &take.take).is_empty())
        .collect();
    let mut boundaries: Vec<TopicBoundary> = Vec::new();
    for (take, reply) in asked.iter().zip(discourse_replies(log, "segment_topics")) {
        let lines = spoken_lines(log, &take.take);
        for segment in reply["segments"].as_array().into_iter().flatten() {
            // A segment that opens at line 0 is the piece's own head: a
            // card there duplicates the title the edit already has.
            let Some(first_line) = segment["startLine"].as_u64().map(|n| n as usize) else {
                continue;
            };
            if first_line == 0 {
                continue;
            }
            // The producer times the segment itself; the transcript lookup
            // is the fallback for a reply that carried no clock.
            let Some(at) = segment["start"].as_f64().or_else(|| {
                lines
                    .get(first_line)
                    .and_then(|line| line["start"].as_f64())
            }) else {
                continue;
            };
            let headline = segment["headline"].as_str().unwrap_or_default().trim();
            // The producer's fallback headline is "segment 3", which says
            // nothing a card can show. Its keywords do, so they win when
            // the headline is only the ordinal.
            let generic = headline.is_empty()
                || headline
                    .split_whitespace()
                    .next()
                    .is_some_and(|word| word.eq_ignore_ascii_case("segment"));
            let words = if generic {
                segment["keywords"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .take(3)
                    .collect::<Vec<_>>()
                    .join(" · ")
            } else {
                headline.to_owned()
            };
            if words.trim().is_empty() {
                continue;
            }
            boundaries.push(TopicBoundary {
                take: take.take.clone(),
                at,
                headline: words,
                confidence: segment["cohesion"].as_f64(),
            });
        }
    }
    boundaries.sort_by(|left, right| left.at.total_cmp(&right.at));
    boundaries
}

/// The pivots the connective analysis found: the moments a comparison,
/// contrast or contingency marker turns the argument, in file seconds.
/// The transition stage cuts on these, so a `but` gets a visible seam
/// rather than a hard cut that hides the turn.
fn discourse_pivots(log: &[Value], takes: &[TakePaths]) -> Vec<(String, f64)> {
    let asked: Vec<&TakePaths> = takes
        .iter()
        .filter(|take| !spoken_lines(log, &take.take).is_empty())
        .collect();
    let mut pivots: Vec<(String, f64)> = Vec::new();
    for (take, reply) in asked
        .iter()
        .zip(discourse_replies(log, "analyze_discourse"))
    {
        for line in reply["perLine"].as_array().into_iter().flatten() {
            let Some(at) = line["start"].as_f64() else {
                continue;
            };
            let turns = line["markers"].as_array().is_some_and(|markers| {
                markers.iter().any(|marker| {
                    matches!(
                        marker["relation"].as_str(),
                        Some("comparison" | "contrast" | "contingency")
                    )
                })
            });
            if turns || line["abruptTransition"] == json!(true) {
                pivots.push((take.take.clone(), at));
            }
        }
    }
    pivots.sort_by(|left, right| left.1.total_cmp(&right.1));
    pivots
}

/// One line of the script read as an act rather than a mood: the take it
/// was said in, its span in file seconds, and its words. The readers that
/// spend the discourse signals (a question, a claim with no proof) all
/// answer in this shape, so the stages that place their cards take one
/// type rather than a tuple each.
struct ScriptLine {
    /// The take the line was read from.
    take: String,
    /// Where the line opens, in file seconds.
    start: f64,
    /// Where it ends, in file seconds.
    end: f64,
    /// The line's own words.
    text: String,
    /// The discourse reader's own confidence in this line, 0..1, when the
    /// reply carried one. A signal log that rated a `speechActs` entry by
    /// `voice_signals`' vocabulary could not rate it at all: the act is
    /// the discourse engine's reading, not the voice profile's.
    confidence: Option<f64>,
}

/// The lines whose surface act asks the viewer something, in file
/// seconds. A question is a hook the narration opens and answers later,
/// and the edit owes it a card the mood alone would not justify - which
/// is exactly why the profile's graphic policy names `speechActs`.
fn question_acts(log: &[Value], takes: &[TakePaths]) -> Vec<ScriptLine> {
    let asked: Vec<&TakePaths> = takes
        .iter()
        .filter(|take| !spoken_lines(log, &take.take).is_empty())
        .collect();
    let mut acts: Vec<ScriptLine> = Vec::new();
    for (take, reply) in asked
        .iter()
        .zip(discourse_replies(log, "detect_speech_acts"))
    {
        for line in reply["perLine"].as_array().into_iter().flatten() {
            if !matches!(
                line["primary"].as_str(),
                Some("question" | "rhetorical_question")
            ) {
                continue;
            }
            let (Some(start), Some(end)) = (line["start"].as_f64(), line["end"].as_f64()) else {
                continue;
            };
            let text = line["text"].as_str().unwrap_or_default().trim();
            if text.is_empty() {
                continue;
            }
            acts.push(ScriptLine {
                take: take.take.clone(),
                start,
                end,
                text: text.to_owned(),
                // The act engine's rows carry no confidence of their
                // own - that is its sibling `classify_speech_acts`'
                // field - so the reading falls back to the engine's own
                // rating for a cued act. A question row only exists
                // because a cue matched, which is exactly the case that
                // engine rates at 0.75; reading the absent key left the
                // signalLog's confidence at 0 for every real run.
                confidence: line["confidence"].as_f64().or(Some(0.75)),
            });
        }
    }
    acts.sort_by(|left, right| left.start.total_cmp(&right.start));
    acts
}

/// The claims the narration asserts but never backs, in file seconds.
/// The claim graph names them itself (`uncoveredClaims`): a line that
/// asserts without an evidence cue and is never supported by one. That is
/// the line a proof card exists for - a source lower-third, a number, a
/// chart - which is why the profile's graphic policy names `claims`
/// beside `entities` and `speechActs`.
fn uncovered_claims(log: &[Value], takes: &[TakePaths]) -> Vec<ScriptLine> {
    let asked: Vec<&TakePaths> = takes
        .iter()
        .filter(|take| !spoken_lines(log, &take.take).is_empty())
        .collect();
    let mut claims: Vec<ScriptLine> = Vec::new();
    for (take, reply) in asked
        .iter()
        .zip(discourse_replies(log, "extract_claim_graph"))
    {
        for claim in reply["uncoveredClaims"].as_array().into_iter().flatten() {
            let (Some(start), Some(end)) = (claim["start"].as_f64(), claim["end"].as_f64()) else {
                continue;
            };
            let text = claim["text"].as_str().unwrap_or_default().trim();
            if text.is_empty() || end <= start {
                continue;
            }
            claims.push(ScriptLine {
                take: take.take.clone(),
                start,
                end,
                text: text.to_owned(),
                // The claim graph rates its nodes as `strength` (evidence
                // minus counters, 0.1 at the floor); `confidence` is a
                // key it never writes, and reading only it left every
                // claim's confidence at 0.
                confidence: claim["confidence"]
                    .as_f64()
                    .or_else(|| claim["strength"].as_f64()),
            });
        }
    }
    claims.sort_by(|left, right| left.start.total_cmp(&right.start));
    claims
}

/// The preset a repeated phrase returns as. A short phrase is a stamp; a
/// longer one reads as a line of narration, so it takes the kinetic
/// caption card rather than a full title. Never a card the library does
/// not carry.
fn motif_preset(phrase: &str) -> &'static str {
    if phrase.split_whitespace().count() <= 3 {
        "funcut.sting.wordmark"
    } else {
        "funcut.caption.kinetic"
    }
}

/// The family a script's caption should be set in, mapped from the
/// profile's font hint onto the faces this editor actually bundles. The
/// hint names a Noto family the machine may not have; the bundled
/// equivalent is the one `concat/src/fonts.rs` registers, and a hint with
/// no bundled equivalent falls back to the house face rather than to a
/// family that would render as boxes.
fn bundled_font(hint: &str) -> &'static str {
    match hint {
        "Noto Sans Bengali" => "Noto Sans Bengali",
        "Noto Sans Devanagari" => "Noto Sans Devanagari",
        "Noto Naskh Arabic" | "Noto Sans Arabic" => "Noto Sans Arabic UI",
        "Noto Sans Hebrew" => "Noto Sans Hebrew",
        "Noto Sans Thai" => "Noto Sans Thai",
        "Noto Sans KR" | "Noto Sans JP" | "Noto Sans SC" => "Noto Sans CJK SC",
        "Noto Sans" => "Inter",
        _ => "Hanken Grotesk",
    }
}

/// The stock query an entity earns: a named organisation wants its
/// industry's footage, a person wants a portrait, a date wants the era's
/// archive, a place wants its location. A number wants no footage - the
/// stat card is its artefact - and a reading the NER was not sure of is
/// not a subject to shop on.
fn entity_stock_query(entity: &Entity) -> Option<String> {
    if entity.confidence.is_some_and(|confidence| confidence < 0.5) {
        return None;
    }
    let subject = card_words(&entity.text);
    if subject.is_empty() {
        return None;
    }
    let suffix = match entity.kind.as_str() {
        "ORG" => "company",
        "PER" => "portrait",
        "DATE" => "archive",
        "GPE" | "LOC" | "FAC" => "location",
        _ => return None,
    };
    Some(format!("{subject} {suffix}"))
}

/// The preset an entity's kind asks for, and the words the card carries.
///
/// The NER is the only reader that knows a line said a number, a date or a
/// name, and the profile's own `card` field already names the artefact it
/// wants: a `stat` for money, a percentage and a bare number, a
/// `timeline` for a date, a `lower-third` for an organisation or person.
/// A kind with no artefact (`PII`, an address) earns no card - naming
/// someone's details on screen is not a graphic, it is a disclosure - and
/// answers `None` so the caller skips it rather than baking a card the
/// narration never asked to show.
fn comp_for_entity(entity: &Entity) -> Option<(&'static str, String)> {
    let card = entity.card.to_ascii_lowercase();
    let preset = match card.as_str() {
        "stat" => "funcut.social.stat",
        "timeline" => "funcut.timeline.horizontal",
        "lower-third" => "funcut.lower-third.minimal",
        _ => match entity.kind.as_str() {
            // A kind the profile's card field did not map: the entity
            // vocabulary's own reading of what a viewer needs to see.
            "MONEY" | "PERCENT" | "PER" | "CARDINAL" | "QUANTITY" => "funcut.social.stat",
            "DATE" | "TIME" => "funcut.timeline.horizontal",
            "ORG" | "PERSON" | "GPE" | "LOC" | "FAC" => "funcut.lower-third.minimal",
            _ => return None,
        },
    };
    Some((preset, entity_card_words(entity)))
}

/// The words a card carries: the parsed value and unit when the ear read
/// one, the entity's own text otherwise. A stat card should show
/// `$10,450` or `45%`, not make every viewer re-parse the recogniser's
/// words; a lower-third keeps the spoken name.
fn entity_card_words(entity: &Entity) -> String {
    let Some(value) = entity.value else {
        return card_words(&entity.text);
    };
    let number = grouped_number(value);
    match entity
        .unit
        .as_deref()
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "percent" => format!("{number}%"),
        "usd" => format!("${number}"),
        "gbp" => format!("£{number}"),
        "eur" => format!("€{number}"),
        "bdt" => format!("৳{number}"),
        "inr" => format!("₹{number}"),
        "jpy" => format!("¥{number}"),
        // A year is read as a year, not as a grouped quantity: 2,026 is
        // not a date anyone writes. A count has no unit to print.
        "year" => format!("{value:.0}"),
        "" | "count" => number,
        _ => format!("{number} {}", entity.unit.as_deref().unwrap_or_default()),
    }
}

/// A number as a card reads it: whole values keep their grouping, and a
/// fractional value keeps two places. The profile rounds away float noise
/// before it writes the value; this is only the display form.
fn grouped_number(value: f64) -> String {
    let raw = if (value.fract()).abs() < 1.0e-6 {
        format!("{value:.0}")
    } else {
        format!("{value:.2}")
    };
    let (whole, fraction) = raw.split_once('.').unwrap_or((raw.as_str(), ""));
    let mut grouped = String::new();
    for (index, digit) in whole.chars().enumerate() {
        if index > 0 && (whole.len() - index) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    if fraction.is_empty() {
        grouped
    } else {
        format!("{grouped}.{fraction}")
    }
}

/// The entity cards the run will bake, strongest first: an entity the NER
/// timed, in the take the timeline plays, at a moment the cut kept. A
/// card with no time cannot be placed, and a card on a moment the tighten
/// cut away has nowhere to land, so both are left out here rather than
/// discovered as a silent skip later.
fn entity_graphics(log: &[Value], takes: &[TakePaths]) -> Vec<Entity> {
    const MOST: usize = 3;
    let view = latest_view(log);
    let media = view.map(media_paths);
    let mut cards: Vec<Entity> = Vec::new();
    for read in voice_reads(log, takes) {
        for entity in &read.entities {
            let Some(at) = entity.at else {
                continue;
            };
            if comp_for_entity(entity).is_none() {
                continue;
            }
            // An approximate reading - the regex complement's fallback,
            // not the NER's own - does not earn the cards that claim
            // exactness. A stat asserts the number is right and a timeline
            // asserts the date is; a lower-third only says the words were
            // said, which a pattern can stand behind.
            if entity.approximate
                && matches!(
                    entity.card.to_ascii_lowercase().as_str(),
                    "stat" | "timeline"
                )
            {
                continue;
            }
            // The producer's own confidence holds the same bar from the
            // other side: a 0.3-confidence MONEY read still earned the
            // stat that claims exactness, so a measured confidence under
            // 0.6 steps down with the approximate ones.
            if entity.confidence.is_some_and(|confidence| confidence < 0.6)
                && matches!(
                    entity.card.to_ascii_lowercase().as_str(),
                    "stat" | "timeline"
                )
            {
                continue;
            }
            // The moment must still be on the timeline: a card for a line
            // the dead-air tighten cut away would never be seen.
            if let (Some(view), Some(media)) = (view, media.as_ref())
                && timeline_at(view, media, &read.take, at).is_none()
            {
                continue;
            }
            if cards.iter().any(|known| {
                known.text.eq_ignore_ascii_case(&entity.text) && known.take == entity.take
            }) {
                continue;
            }
            cards.push(entity.clone());
            if cards.len() >= MOST {
                return cards;
            }
        }
    }
    cards
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

/// The line read that holds a file second, in the take the card belongs
/// to. `None` when the ear read no line there - a foreign reply may have
/// the emotion but not the prosody.
fn voice_line_at<'a>(voice: &'a [VoiceRead], take: &str, at: f64) -> Option<&'a LineRead> {
    voice
        .iter()
        .find(|read| read.take == take)
        .and_then(|read| read.line_at(at))
}

/// Where the card should open: the strongest stressed word inside the
/// line, so the graphic lands on the delivery's own beat rather than a
/// fixed offset from the line start. `None` when the ear stressed no word
/// in the span, which keeps the old line-start behaviour.
fn emphasis_moment(voice: &[VoiceRead], take: &str, from: f64, to: f64) -> Option<f64> {
    voice
        .iter()
        .find(|read| read.take == take)
        .and_then(|read| {
            read.lines
                .iter()
                .filter(|line| line.end > from && line.start < to)
                .flat_map(|line| line.emphasis.iter())
                .max_by(|left, right| left.strength.total_cmp(&right.strength))
                .map(|word| word.at)
        })
        .filter(|at| *at >= from && *at <= to)
}

/// Whether the line under a card is solid enough to carry one: the
/// forensic ear did not call the voice unstable, the line has some voice
/// under it, and the confidence gate did not flag most of its words. An
/// unmeasured line is not a vetoed one - only a reading that says "this
/// line is not what the transcript thinks" refuses the card.
fn motion_line_reliable(voice: &[VoiceRead], take: &str, at: f64) -> bool {
    let Some(read) = voice_read_of(voice, take) else {
        return true;
    };
    let Some(line) = read.line_at(at) else {
        return true;
    };
    read.clean_voice(line) != Some(false)
        && line.voiced_ratio.is_none_or(|ratio| ratio >= 0.2)
        && read.unsure_between(line.start, line.end).len() < 2
}

/// The line's register lift over the take's mean pitch, in semitones. A
/// card on a lifted line holds a little longer: the delivery raised it.
fn pitch_lift_at(voice: &[VoiceRead], take: &str, at: f64) -> Option<f64> {
    voice
        .iter()
        .find(|read| read.take == take)
        .and_then(|read| read.line_at(at).and_then(|line| read.register_lift(line)))
}

/// The confidence floor a card must clear: a reading the producer rated
/// below this is a guess, and a guess does not get to interrupt the
/// picture. `None` is not a low score - an unrated reading keeps its
/// card, because the producer's silence is not a contradiction.
const CARD_FLOOR: f64 = 0.35;

fn card_confident(confidence: Option<f64>) -> bool {
    confidence.is_none_or(|confidence| confidence >= CARD_FLOOR)
}

/// The motion the run will bake, one row per line that earned a card, in
/// play order: the take and file second the line opens on (the stage maps
/// it through the timeline), the preset, and the words the card sets.
///
/// The density floor is the plan schema's own: at most one card per
/// section and at most one per fifteen seconds, strongest line first. Two
/// cards in the same three seconds is a slideshow, not an edit.
fn motion_specs(
    log: &[Value],
    lines: &[EmotionLine],
    voice: &[VoiceRead],
    entities: &[Entity],
    chapters: &[TopicBoundary],
    motifs: &[Motif],
    questions: &[ScriptLine],
    claims: &[ScriptLine],
    // Whether the captions stage already placed word-timed captions: a
    // kinetic question card sits in the same band the captions occupy
    // (~80% of frame height), so with captions on, the question's own
    // words are already on screen and a second copy would collide with
    // them - the card is a captions-off run's answer, not a bonus.
    captions_placed: bool,
) -> Vec<MotionSpec> {
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
            let at = emphasis_moment(voice, &line.take, line.start, line.end)
                .unwrap_or(line.start)
                .clamp(line.start, (line.end - 0.2).max(line.start));
            if !motion_line_reliable(voice, &line.take, at) || !card_confident(line.confidence) {
                return None;
            }
            let line_read = voice_line_at(voice, &line.take, at);
            Some((
                line.intensity * line.confidence.unwrap_or(0.8),
                MotionSpec {
                    take: line.take.clone(),
                    at,
                    end: line.end,
                    preset,
                    words,
                    act: line.act.clone(),
                    pace: line.pace.clone(),
                    pitch_range: line_read.and_then(|line| line.pitch_range),
                    pitch_lift: pitch_lift_at(voice, &line.take, at),
                    earned_by: "emotion",
                    intensity: line.intensity,
                    pulse: voice_read_of(voice, &line.take).and_then(|read| read.pulse_period()),
                    motion_speed: motion_speed_at(log, &line.take, at),
                    chapter: false,
                    slot: None,
                    slots: Vec::new(),
                },
            ))
        })
        .collect();
    // The entity cards: a stat or a lower-third the narration actually
    // named, weighted by how hard its line reads so a number said in
    // passing loses to one said at the peak. The NER's confidence is the
    // entity's own, and a kind with no artefact never reaches here.
    for entity in entities {
        let (Some(at), Some((preset, words))) = (entity.at, comp_for_entity(entity)) else {
            continue;
        };
        if words.is_empty()
            || !motion_line_reliable(voice, &entity.take, at)
            || !card_confident(entity.confidence)
        {
            continue;
        }
        let line = lines.iter().find(|line| {
            line.take == entity.take && line.start <= at + 0.05 && line.end > at - 0.05
        });
        // A number the narration measured is the card's own Number slot -
        // the reader's display form sets it instead of the prose the
        // recogniser heard - and the words the narration said become the
        // Label under it. The old fill put the raw `10450 usd` in Number
        // and the formatted `$10,450` in Label, so the card repeated the
        // number where its label belonged.
        let slots: Vec<(&'static str, String)> = if entity.value.is_some() {
            vec![
                ("Number", entity_card_words(entity)),
                ("Label", card_words(&entity.text)),
            ]
        } else {
            Vec::new()
        };
        let rank =
            line.map(|line| line.intensity).unwrap_or(0.5) * entity.confidence.unwrap_or(0.8);
        ranked.push((
            rank,
            MotionSpec {
                take: entity.take.clone(),
                at,
                end: line.map(|line| line.end).unwrap_or(at + 2.5),
                preset,
                words,
                act: line
                    .map(|line| line.act.clone())
                    .unwrap_or_else(|| "proof".to_owned()),
                pace: line.map(|line| line.pace.clone()).unwrap_or_default(),
                pitch_range: voice_line_at(voice, &entity.take, at)
                    .and_then(|line| line.pitch_range),
                pitch_lift: pitch_lift_at(voice, &entity.take, at),
                earned_by: "entities",
                intensity: line.map(|line| line.intensity).unwrap_or(0.5),
                pulse: voice_read_of(voice, &entity.take).and_then(|read| read.pulse_period()),
                motion_speed: motion_speed_at(log, &entity.take, at),
                chapter: false,
                slot: None,
                slots,
            },
        ));
    }
    // The chapter cards: a title over each topic change, named by the
    // tiling's own headline. They rank above a line's beat - a chapter is
    // the strongest thing the script said - but still obey the spacing
    // floor, because a chapter card landing on a stat card is a pile.
    for (index, chapter) in chapters.iter().enumerate() {
        let words = card_words(&chapter.headline);
        if words.is_empty() || !card_confident(chapter.confidence) {
            continue;
        }
        ranked.push((
            chapter.confidence.unwrap_or(0.75),
            MotionSpec {
                take: chapter.take.clone(),
                at: chapter.at,
                end: chapter.at + 2.0,
                preset: "funcut.chapter.numbered",
                words,
                act: "turn".to_owned(),
                pace: String::new(),
                pitch_range: None,
                pitch_lift: None,
                earned_by: "topics",
                intensity: 1.0,
                pulse: None,
                motion_speed: motion_speed_at(log, &chapter.take, chapter.at),
                chapter: true,
                slot: Some("Title"),
                slots: vec![
                    ("Number", format!("{:02}", index + 1)),
                    ("Kicker", "CHAPTER".to_owned()),
                ],
            },
        ));
    }
    // The motif cards: a phrase the narration repeats gets its visual back
    // when it returns. The first mention establishes the card, so only the
    // repeats become specs - the same words, the same preset, so a viewer
    // reads the return as a callback rather than a new idea.
    for motif in motifs {
        let words = card_words(&motif.phrase);
        if words.is_empty() || motif.line_indices.len() < 2 {
            continue;
        }
        let preset = motif_preset(&motif.phrase);
        for index in motif.line_indices.iter().skip(1) {
            let Some(line) = lines.get(*index) else {
                continue;
            };
            let at = emphasis_moment(voice, &line.take, line.start, line.end)
                .unwrap_or(line.start)
                .clamp(line.start, (line.end - 0.2).max(line.start));
            if !motion_line_reliable(voice, &line.take, at) {
                continue;
            }
            let confidence = voice_read_of(voice, &line.take)
                .map(|read| read.weight("motifs"))
                .unwrap_or(0.7);
            if confidence < CARD_FLOOR {
                continue;
            }
            ranked.push((
                line.intensity * 0.9 * confidence,
                MotionSpec {
                    take: line.take.clone(),
                    at,
                    end: line.end,
                    preset,
                    words: words.clone(),
                    act: line.act.clone(),
                    pace: line.pace.clone(),
                    pitch_range: voice_line_at(voice, &line.take, at)
                        .and_then(|line| line.pitch_range),
                    pitch_lift: pitch_lift_at(voice, &line.take, at),
                    earned_by: "motifs",
                    intensity: line.intensity,
                    pulse: voice_read_of(voice, &line.take).and_then(|read| read.pulse_period()),
                    motion_speed: motion_speed_at(log, &line.take, at),
                    chapter: false,
                    slot: None,
                    slots: Vec::new(),
                },
            ));
        }
    }
    // The question cards: the narration asks the viewer something, and the
    // card holds the question while the answer plays. A question is the
    // one line whose act, not its mood, is why it earns a card - the
    // profile's graphic policy names exactly this signal.
    for question in questions {
        let words = card_words(&question.text);
        if words.is_empty() || captions_placed || !card_confident(question.confidence) {
            continue;
        }
        let at = emphasis_moment(voice, &question.take, question.start, question.end)
            .unwrap_or(question.start)
            .clamp(question.start, (question.end - 0.2).max(question.start));
        if !motion_line_reliable(voice, &question.take, at) {
            continue;
        }
        ranked.push((
            0.85 * question.confidence.unwrap_or(0.75),
            MotionSpec {
                take: question.take.clone(),
                at,
                end: question.end,
                preset: "funcut.caption.kinetic",
                words,
                act: String::new(),
                pace: String::new(),
                pitch_range: voice_line_at(voice, &question.take, at)
                    .and_then(|line| line.pitch_range),
                pitch_lift: pitch_lift_at(voice, &question.take, at),
                earned_by: "speechActs",
                intensity: 0.85,
                pulse: voice_read_of(voice, &question.take).and_then(|read| read.pulse_period()),
                motion_speed: motion_speed_at(log, &question.take, at),
                chapter: false,
                slot: None,
                slots: Vec::new(),
            },
        ));
    }
    // The proof cards: a claim the narration asserts but never backs. The
    // card is a pull quote, not a caption - the line is the argument, so
    // it is set as the argument. This is the reader the profile's
    // `claims` signal exists for; a run that never asked the graph has no
    // claims and grows no card.
    for claim in claims {
        let words = card_words(&claim.text);
        if words.is_empty() || !card_confident(claim.confidence) {
            continue;
        }
        let at = emphasis_moment(voice, &claim.take, claim.start, claim.end)
            .unwrap_or(claim.start)
            .clamp(claim.start, (claim.end - 0.2).max(claim.start));
        if !motion_line_reliable(voice, &claim.take, at) {
            continue;
        }
        ranked.push((
            0.8 * claim.confidence.unwrap_or(0.75),
            MotionSpec {
                take: claim.take.clone(),
                at,
                end: claim.end,
                preset: "funcut.quote.pull",
                words,
                act: "proof".to_owned(),
                pace: String::new(),
                pitch_range: voice_line_at(voice, &claim.take, at)
                    .and_then(|line| line.pitch_range),
                pitch_lift: pitch_lift_at(voice, &claim.take, at),
                earned_by: "claims",
                intensity: 0.8,
                pulse: voice_read_of(voice, &claim.take).and_then(|read| read.pulse_period()),
                motion_speed: motion_speed_at(log, &claim.take, at),
                chapter: false,
                slot: None,
                slots: Vec::new(),
            },
        ));
    }
    ranked.sort_by(|left, right| right.0.total_cmp(&left.0));
    // Cards from different takes carry file seconds of different,
    // independent clocks - two cards at the same local second of two
    // takes are minutes apart on the timeline, and within one take the
    // tighten has since moved the words. The spacing the card layout
    // actually owes is measured on the timeline the run edits, mapped
    // through the placements the tighten left; a card whose moment
    // cannot be mapped keeps its file second rather than being dropped.
    let view = latest_view(log);
    let media = view.as_ref().map(|view| media_paths(view));
    let timeline_of = |spec: &MotionSpec| -> f64 {
        match (&view, &media) {
            (Some(view), Some(media)) => {
                timeline_at(view, media, &spec.take, spec.at).unwrap_or(spec.at)
            }
            _ => spec.at,
        }
    };
    let mut kept: Vec<MotionSpec> = Vec::new();
    for (_, spec) in ranked {
        if kept.len() >= MOST {
            break;
        }
        let at = timeline_of(&spec);
        let crowded = kept
            .iter()
            .any(|other| (timeline_of(other) - at).abs() < SPACING);
        if !crowded {
            kept.push(spec);
        }
    }
    kept.sort_by(|left, right| left.at.total_cmp(&right.at));
    kept
}

/// The preset library's id, first text slot, all slot labels, and duration.
type MotionPresetShape = (String, String, Vec<String>, f64);

/// One designed beat, as the plan records it and the motion stage bakes
/// it: the line it sits on, the preset, the span it covers, and the words
/// the card sets - the line's own, so a card is never a sample's text.
#[derive(Clone, PartialEq, Debug, Default)]
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
    /// The line's pitch range in semitones: a performed line holds its
    /// card a touch longer than a reported one.
    pitch_range: Option<f64>,
    /// The line's register lift over the take, in semitones: a lifted
    /// line's card holds longer still.
    pitch_lift: Option<f64>,
    /// The signal that earned the card: the read's act, or the entity the
    /// narration named. The log cites this, so a stat card is never
    /// accounted for by an emotion that had nothing to do with it.
    earned_by: &'static str,
    /// The emotional intensity of the line, 0..1. A high-intensity beat
    /// holds its card a touch longer and a calm one lets it go.
    intensity: f64,
    /// The take's rhythm period, in seconds, when the prosody read found
    /// one with strength behind it. A card whose hold lands on the
    /// delivery's own beat reads as part of the edit rather than over it.
    pulse: Option<f64>,
    /// The motion-design read's tempo for the line, 0.6-1.6: a punchy
    /// delivery shortens the hold, a slow one lets it breathe.
    motion_speed: Option<f64>,
    /// How the card holds. A chapter card is a title over a topic change:
    /// it holds its own beat rather than the line's, and never longer.
    chapter: bool,
    /// The preset slot the words belong in, when the card's words are not
    /// for the preset's first text slot. A chapter card's headline is its
    /// title, not its kicker.
    slot: Option<&'static str>,
    /// Extra (label, value) fills beside the words: a chapter's Number and
    /// Kicker. Every slot the preset carries that neither the words nor
    /// this list fills is BLANKED at the bake - the preset's sample text
    /// ("-- FUNCUT, MOTION NOTES", "RENDERS THIS WEEK", "REAR ADMIRAL")
    /// is a placeholder for a designer, never a fact the narration said,
    /// and shipping it fabricated an attribution, a statistic and a
    /// credential the video never made.
    slots: Vec<(&'static str, String)>,
}

impl MotionSpec {
    /// Seconds the card holds: its line's length, floored so a clipped
    /// line still reads, and capped at the editorial default so a long
    /// line does not own the screen.
    fn hold(&self) -> f64 {
        if self.chapter {
            // A chapter card is a signpost, not a beat: two seconds reads
            // it and gets out of the narration's way.
            return 2.0;
        }
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
        // The motion-design tempo is a second, measured opinion on the
        // delivery: a fast read shortens the card, a slow one lengthens it.
        let paced = paced / self.motion_speed.unwrap_or(1.0).clamp(0.6, 1.6);
        // A performed line (a wide pitch range) and a lifted one (its
        // register over the take's) carry more than a flat report: the
        // card stays a beat longer, still inside the editorial cap.
        let performed = self.pitch_range.is_some_and(|range| range >= 6.0);
        let lifted = self.pitch_lift.is_some_and(|lift| lift >= 2.0);
        let shaped = paced * if performed { 1.08 } else { 1.0 } * if lifted { 1.04 } else { 1.0 };
        // The emotional read's own intensity is the card's last arm: a
        // beat the read called intense holds a touch longer, a calm one
        // lets the narration move on. It never breaks the editorial cap.
        let arc = 0.9 + 0.2 * self.intensity.clamp(0.0, 1.0);
        let shaped = shaped * arc;
        // A strong pulse quantizes the hold to the delivery's own beat,
        // inside the editorial bounds. A period with no strength behind
        // it never reaches here: `pulse_period` already held it to 0.25.
        let shaped = match self.pulse.filter(|period| (0.2..=2.0).contains(period)) {
            Some(period) => (shaped / period).round() * period,
            None => shaped,
        };
        round3(shaped.clamp(1.2, 5.0))
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
/// soft one. The ids are the catalogue's own transition packages - the
/// transition-KIND ones, with a `[transition]` section the export
/// resolver requires: an effect-kind id (the plain `concat.zoom-blur`
/// family) falls through the resolver's catalogue arm and the cut
/// renders plain, the very move the emotion asked for silently lost.
fn transition_for_emotion(emotion: &str) -> &'static str {
    match emotion {
        "joy" => "concat.zoom-blur-transition",
        "surprise" => "concat.hyper-zoom",
        "anger" => "concat.crash-zoom",
        "fear" => "concat.blur-dissolve",
        "sadness" => "concat.dissolve",
        "disgust" => "concat.blinds-soft",
        _ => "concat.blur-dissolve",
    }
}

/// The second transition for a mood, so two shots cut from the same
/// emotion never wear the same move twice running: the no-repeat rule
/// the director enforces, not merely asks for. Every id is the
/// catalogue's own - the viral pack's neighbours of the first move,
/// not the first move again.
fn alternate_transition(emotion: &str) -> &'static str {
    match emotion {
        "joy" => "concat.cross-pop",
        "surprise" => "concat.flash-social",
        "anger" => "concat.glitch-warp",
        "fear" => "concat.dissolve",
        "sadness" => "concat.zoom-blur-transition",
        "disgust" => "concat.dissolve",
        _ => "concat.whip-pan",
    }
}

/// The emotional read the run made, keyed by the take each reply
/// answered. The extract echoes the `audio` it was handed, so the
/// pairing is keyed on the answer itself: a call that failed leaves no
/// reply behind, and the positional zip this replaced would then have
/// read every later take under its neighbour's arc - the wrong words,
/// acts, beds and B-roll queries, with no error anywhere. A reply with
/// no echo (a sidecar older than the field) still pairs positionally
/// with the unclaimed takes, in program order, which is what the stage's
/// asking order makes right when every call answered.
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
    // The takes each reply is paired with: the echo names its own take
    // when it carries one, and an echo-less reply takes the first
    // unclaimed take in program order - never a take another reply has
    // already answered.
    let mut claimed = vec![false; asked.len()];
    replies
        .iter()
        .filter_map(|reply| {
            let echoed = reply.get("audio").and_then(Value::as_str);
            let at = match echoed {
                Some(path) => asked.iter().position(|take| take.take == path),
                None => claimed.iter().position(|free| !free),
            }?;
            claimed[at] = true;
            Some((asked[at], *reply))
        })
        .flat_map(|(take, reply)| {
            // The lines the stage sent this take's extract, in order: the
            // reply's rows answer them one for one, and the words the
            // transcript measured are the words a card carries. The
            // extract's own confidence rides the reply, not the row - the
            // engine answers once for the whole read.
            let read = reply["confidence"].as_f64();
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
                    EmotionLine::of(&take.take, row, text, &pace, read)
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

/// Whether the take's emotional arc rises: the mean intensity of the
/// last third of its emotion lines at least 0.25 over the first third's,
/// over at least four lines. The catalogue's `building` cut style -
/// start held, accelerate into the climax - is the arc's own shape, and
/// this is the reading that says the take has it. The profile can derive
/// `building` itself only when the emotion read was handed to it, which
/// the unattended spine never did; the arc it measured afterwards is the
/// same evidence read one stage later, and this is where it is spent.
fn arc_building(read: &[EmotionLine], take: &str) -> bool {
    let mut lines: Vec<&EmotionLine> = read.iter().filter(|line| line.take == take).collect();
    if lines.len() < 4 {
        return false;
    }
    lines.sort_by(|left, right| left.start.total_cmp(&right.start));
    let third = lines.len() / 3;
    let mean = |rows: &[&EmotionLine]| {
        rows.iter().map(|line| line.intensity).sum::<f64>() / rows.len().max(1) as f64
    };
    mean(&lines[lines.len() - third..]) - mean(&lines[..third]) >= 0.25
}

/// The look the grade lays, and the readings that chose it. Emotion is
/// the primary read; the forensic tone is the tie-breaker only where the
/// emotion named no temperature (the house teal-and-orange), so a
/// measured dull take is warmed and a measured bright or sibilant one is
/// cooled instead of shipping the same default look on every neutral
/// script. The arc decides whether the grade lifts again at the end.
struct GradeChoice {
    look: &'static str,
    signals: Vec<&'static str>,
    note: String,
    builds: bool,
}

fn grade_decision(log: &[Value], takes: &[TakePaths]) -> GradeChoice {
    let read = emotion_lines(log, takes);
    let dominant = dominant(&read);
    let mut signals: Vec<&'static str> = Vec::new();
    let mut look = "concat.teal-orange";
    let mut builds = false;
    let mut facts: Vec<String> = Vec::new();
    let mut add = |signal: &'static str| {
        if !signals.contains(&signal) {
            signals.push(signal);
        }
    };
    if let Some(line) = dominant {
        look = look_for_temperature(&line.temperature);
        add("emotion");
        facts.push(format!("the {} line asks for {look}", line.emotion));
        builds = arc_building(&read, &line.take);
        if builds {
            add("arc");
            facts.push("the arc builds, so the grade lifts at the end".to_owned());
        }
    }
    let reads = voice_reads(log, takes);
    let mut centroid_sum = 0.0;
    let mut centroid_count = 0usize;
    for hz in reads
        .iter()
        .filter_map(|read| read.take_wide.spectral_centroid_hz)
        .filter(|hz| hz.is_finite() && *hz > 0.0)
    {
        centroid_sum += hz;
        centroid_count += 1;
    }
    if centroid_count > 0 {
        add("forensic.spectralCentroidHz");
        let centroid = centroid_sum / centroid_count as f64;
        if look == "concat.teal-orange" {
            if centroid < 1600.0 {
                look = "concat.golden-hour";
                facts.push(format!(
                    "the take's centroid measures {centroid:.0} Hz, so the dull read is warmed"
                ));
            } else if centroid > 3200.0 {
                look = "concat.cool";
                facts.push(format!(
                    "the take's centroid measures {centroid:.0} Hz, so the bright read is cooled"
                ));
            }
        }
    }
    let sibilance = reads
        .iter()
        .filter_map(|read| read.take_wide.sibilance_ratio)
        .fold(None, |worst: Option<f64>, share| {
            Some(worst.map_or(share, |worst| worst.max(share)))
        });
    if let Some(share) = sibilance {
        add("forensic.sibilanceRatio");
        if look == "concat.teal-orange" && share >= 0.15 {
            look = "concat.cool";
            facts.push(format!(
                "the sibilance share is {share:.2}, so the warm look would ride the harsh band"
            ));
        }
    }
    let reverb = reads
        .iter()
        .filter_map(|read| read.take_wide.reverb_tail_sec)
        .fold(None, |longest: Option<f64>, tail| {
            Some(longest.map_or(tail, |longest| longest.max(tail)))
        });
    if let Some(tail) = reverb {
        add("forensic.reverbTailSec");
        if look == "concat.teal-orange" && tail >= 0.4 {
            look = "concat.vintage";
            facts.push(format!(
                "the reverb tail measures {tail:.2} s, so the softer vintage texture fits the room"
            ));
        }
    }
    let note = if facts.is_empty() {
        "no read measured a look; the house look stands".to_owned()
    } else {
        facts.join("; ")
    };
    GradeChoice {
        look,
        signals,
        note,
        builds,
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
        // Every word here has to answer the shelf: "texture" matched
        // none of the 530, so a disgust peak whose rotation reached it
        // was silently dropped. Foley is the family the shelf actually
        // carries, crunch and hit are its disgust register.
        "disgust" => &["foley", "hit", "crunch"],
        _ => &["whoosh", "impact", "riser", "pop", "click", "foley"],
    }
}

/// The level an accent lands at. The shelf is normalised near full
/// scale and the moment an accent marks is, by design, the voice's own
/// loudest, so an accent left at unity gain sums past full scale - and
/// the true-peak gate at verify would fail the run only after the render
/// was paid for. The gain follows the peak's own strength, inside the
/// skill's -6..-24 dB table.
fn accent_gain_db(strength: f64) -> f64 {
    // The prosody ear's strength is a z-score over the line's own energy
    // envelope, and it only reports peaks above mean+1sigma - so every
    // measured strength is > 1.0 and the old 0.8/0.65 buckets put every
    // accent at -9 dB, deaf to the difference between a shout and a
    // lean-in. The buckets read in sigma space now: 2+ sigma is the
    // shout, 1.4 the lean-in, anything else the whisper the floor
    // always was.
    if strength >= 2.0 {
        -9.0
    } else if strength >= 1.4 {
        -12.0
    } else {
        -15.0
    }
}

/// The queries the emotional read's own `sfxType` names, which is more
/// specific than the emotion alone: a line read as a stinger is shopped
/// for a stinger, not the family its emotion happens to share. The
/// family is the fallback when the read named no register.
/// The words a generated accent is written from: the family the peak's
/// emotion named, said the way a sound model hears it - the object first,
/// then the character, then what the file must not carry. A prompt with
/// music in it is a prompt the SFX model answers with a bed.
fn sfx_prompt(queries: &[&'static str], emotion: &str) -> String {
    let word = queries.first().copied().unwrap_or("impact");
    format!("a {word} sound effect, {emotion}, dry, close, cinematic, no music")
}

/// The words a generated music bed is written from: the read's own bed
/// words when it named any, and otherwise the one register an unattended
/// cut can always take.
fn bed_prompt(log: &[Value], takes: &[TakePaths]) -> String {
    let word = bed_query_chain(log, takes)
        .into_iter()
        .flatten()
        .next()
        .unwrap_or_else(|| "cinematic underscore".to_owned());
    format!("{word}, instrumental, no drums, sparse, warm, cinematic")
}

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
        // "texture" answers nothing on the shelf (zero of the 530 carry
        // the word), so the register's family is shopped in words the
        // sounds actually have.
        &["foley", "crunch", "hit"]
    } else {
        &[]
    };
    if named.is_empty() {
        sfx_queries_for_emotion(emotion).to_vec()
    } else {
        named.to_vec()
    }
}

/// Translate the shelf's search words into the sound role that the
/// placement reader must verify. Without this, check_placement falls back
/// to the file's own detected role, so a wrong-family file can validate
/// itself by definition.
fn sfx_role_for_query(query: &str) -> &'static str {
    match query {
        "stinger" => "cinematic_reveal",
        "drone" | "riser" => "tension_build",
        "chime" | "pop" | "swell" => "gentle_entrance",
        "ambience" | "tone" | "pad" | "foley" | "crunch" => "emotional_texture",
        "impact" | "hit" => "action_impact",
        "whoosh" | "click" => "quick_transition",
        _ => "quick_transition",
    }
}

/// The take a timeline moment sits on, and the file-second of the take it
/// is playing: the inverse of `timeline_at`, asked by a clip that carries
/// no media of its own (a caption, a title) whose delivery must be read
/// from the voice that plays beneath it. Where two takes overlap (a
/// collision the timeline check refuses), the later start wins.
fn take_second_at(
    view: &Value,
    media: &[(String, String)],
    takes: &[TakePaths],
    moment: f64,
) -> Option<(String, f64)> {
    let mut best: Option<(f64, String, f64)> = None; // (start, take, file second)
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
        let (Some(start), Some(duration)) = (clip["start"].as_f64(), clip["duration"].as_f64())
        else {
            continue;
        };
        if !(moment >= start && moment < start + duration) {
            continue;
        }
        let (source_in, _, speed) = clip_file_span(clip);
        let file = source_in + (moment - start) * speed;
        if best.as_ref().is_none_or(|(known, _, _)| start > *known) {
            best = Some((start, path.to_owned(), file));
        }
    }
    best.map(|(_, take, file)| (take, file))
}

/// The titles the entrance pass keys: every text clip the timeline now
/// holds that is long enough to hold an entrance and is not already
/// keyed. The captions stage places them a batch earlier, so they are
/// the words the run just landed - the one place a one-click run can
/// give its type motion without a designer.
fn entrance_targets(log: &[Value], takes: &[TakePaths]) -> Vec<(String, f64, f64, Vec<f64>)> {
    let Some(view) = latest_view(log) else {
        return Vec::new();
    };
    // The delivery's own curve, where a line was read: a tense line eases
    // in, a warm one settles out (EMOTION.md's headline law). A caption
    // has no media of its own, so the line is found through the timeline:
    // the take playing under the caption's first second, at the
    // file-second that moment plays. The default is the settle every
    // caption used to take regardless of what the voice did.
    let read = emotion_lines(log, takes);
    let media = media_paths(view);
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
            // The clip's own placement: an entrance keyed absolutely
            // around zero would drag a lower-third caption to the middle
            // of the frame, so the rise is keyed around wherever the
            // caption actually sits.
            let offset_y = clip["offsetY"].as_f64().unwrap_or(0.0);
            let ease = clip["start"]
                .as_f64()
                .and_then(|start| take_second_at(view, &media, takes, start))
                .and_then(|(path, file)| emotion_at(&read, &path, file))
                .map(|line| line.easing.clone())
                .unwrap_or_else(default_ease);
            (duration >= 0.4).then_some((id, duration, offset_y, ease))
        })
        .collect()
}

/// The settle every caption takes when no emotion line was read: the
/// emphasis curve (0.16, 1.0, 0.3, 1.0).
fn default_ease() -> Vec<f64> {
    vec![0.16, 1.0, 0.3, 1.0]
}

/// One title's entrance, as the batch of keys that draws it: the words
/// rise a little and settle as they arrive (scale 0.94 -> 1.0, offsetY
/// placed + 0.05 -> placed), the keys placed as fractions of the title's
/// own span so a caption re-timed by the tighten still enters where it
/// now begins. Offset keys are absolute, so the rise is keyed around the
/// clip's own placement - a lower-third stays a lower-third while it
/// settles, instead of being dragged to mid-frame. The settle curve is
/// the engine's own ease-out; a title too short to hold an entrance is
/// left still, and one already keyed is left alone.
fn entrance_keys(clip_id: &str, duration: f64, placed: f64, ease: &[f64]) -> Vec<Value> {
    let span = (0.35 / duration).clamp(0.05, 0.6);
    let ease = json!(ease);
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
            "at": 0.0, "value": round3(placed + 0.05), "ease": ease,
        }),
        json!({
            "op": "setClipKey", "clipId": clip_id, "property": "offsetY",
            "at": round3(span), "value": round3(placed), "ease": ease,
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

/// The profile's own name for a signal family, where the decision log's
/// vocabulary differs from the one `signalConfidence` rates. Without this
/// an entry for a `pause` would read the confidence of nothing and log a
/// zero, which is the opposite of the measurement: the profile rates
/// pauses at 0.75. A name with no mapping is already the profile's.
fn confidence_family(signal: &str) -> &str {
    match signal {
        "pause" => "pauses",
        "sentence" => "sentenceType",
        "speakerChanges" => "speaker",
        "musicSegments" => "forensic",
        "noiseFloor" => "forensic",
        "emphasisWords" => "prosody",
        // The pulse and the density are rated under the families that
        // measured them (prosody's rhythm layers; the pauses the speech
        // ratio is the complement of) - unmapped, their log entries read
        // 0.0 despite full measurements behind them.
        "rhythmLayers" => "prosody",
        "speechRatio" => "pauses",
        _ => signal,
    }
}

/// The integrated loudness the mix is put on: -14 LUFS, the level the
/// platforms normalise a short cut toward, vertical reel or not. `None`
/// when there is no timeline to read, so a run with no runtime mixes
/// nothing rather than guessing a level.
fn platform_target(log: &[Value]) -> Option<f64> {
    latest_view(log).map(|_| -14.0)
}

/// The orientation word the live shelves' own search parameter takes,
/// for the frame the project actually is. `None` when the aspect is not
/// one the shelves name, so the search is left unfiltered rather than
/// shopped with a guess.
fn shelf_orientation(aspect: &str) -> Option<&'static str> {
    match aspect {
        "9:16" | "4:5" => Some("vertical"),
        "16:9" => Some("horizontal"),
        "1:1" => Some("square"),
        _ => None,
    }
}

/// The aspect the project's own frame is nearest: a 9:16 project is a
/// reel, a 16:9 one is not. Every view the program ever reads is a
/// `summary::view` product, and the summary puts the frame under
/// `/project/width|height` - the old `/settings/*` read matched nothing
/// real, so every project read as 16:9 and a vertical reel was shopped,
/// captioned and graded landscape (the hand-built `/settings` views lived
/// on only in the unit tests, codifying the wrong shape). The
/// `/settings` read stays as the fallback for a raw editor view.
fn aspect_of(view: &Value) -> &'static str {
    let frame = frame_of(view);
    let FrameOf { width, height } = frame;
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

/// The frame a view reports, from whichever shape it carries.
struct FrameOf {
    width: f64,
    height: f64,
}

fn frame_of(view: &Value) -> FrameOf {
    let width = view
        .pointer("/project/width")
        .or_else(|| view.pointer("/settings/width"))
        .and_then(Value::as_f64)
        .unwrap_or(1920.0);
    let height = view
        .pointer("/project/height")
        .or_else(|| view.pointer("/settings/height"))
        .and_then(Value::as_f64)
        .unwrap_or(1080.0);
    FrameOf { width, height }
}

/// The three visual directions the one-click edit auditions before it
/// commits. They are deliberately different films, not three grades of the
/// same film: the documentary trusts evidence and archive, the kinetic
/// explainer trusts motion and detail, and the cinematic essay trusts
/// atmosphere and performance. The topic, the frame and the measured
/// delivery pick the strongest; the plan keeps all three and the reason.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VisualDirection {
    Documentary,
    Kinetic,
    Cinematic,
}

impl VisualDirection {
    fn label(self) -> &'static str {
        match self {
            Self::Documentary => "evidence-first documentary",
            Self::Kinetic => "kinetic explainer",
            Self::Cinematic => "cinematic essay",
        }
    }

    fn tone(self) -> &'static str {
        match self {
            Self::Documentary => "measured, investigative, plain-spoken",
            Self::Kinetic => "urgent, clear, forward-leaning",
            Self::Cinematic => "reflective, atmospheric, intimate",
        }
    }

    fn reference(self) -> &'static str {
        match self {
            Self::Documentary => {
                "archive-led public-service documentary: evidence on screen, cuts on the claim"
            }
            Self::Kinetic => {
                "modern explainer channel: one idea per shot, motion carrying the beat"
            }
            Self::Cinematic => {
                "observational film essay: long looks, natural sound, a held emotional turn"
            }
        }
    }

    fn grade(self) -> &'static str {
        match self {
            Self::Documentary => "concat.cool-blue",
            Self::Kinetic => "concat.teal-orange",
            Self::Cinematic => "concat.warm",
        }
    }

    fn palette(self, look: &str) -> [&'static str; 3] {
        match self {
            Self::Documentary => match look {
                "concat.warm" | "concat.golden" => ["#14100b", "#d8c3a5", "#b5763a"],
                "concat.mono" | "concat.noir" => ["#080808", "#e8e4da", "#9a8c7a"],
                "concat.cold" | "concat.blue" => ["#071018", "#c9d6df", "#c08a4e"],
                _ => ["#0b0d12", "#d9d2c3", "#b08d57"],
            },
            Self::Kinetic => match look {
                "concat.warm" | "concat.golden" => ["#1a0f08", "#ffd08a", "#ff5c39"],
                "concat.mono" | "concat.noir" => ["#08080a", "#f2f2f2", "#ff5c39"],
                "concat.cold" | "concat.blue" => ["#08111f", "#9fd3ff", "#ff5c39"],
                _ => ["#08111f", "#9fd3ff", "#ff5c39"],
            },
            Self::Cinematic => match look {
                "concat.warm" | "concat.golden" => ["#1a0f08", "#f2c078", "#d96c4a"],
                "concat.mono" | "concat.noir" => ["#0a0a0a", "#e8e8e8", "#b9a58f"],
                "concat.cold" | "concat.blue" => ["#071018", "#b8d8e8", "#c08a4e"],
                _ => ["#140f0b", "#e8c9a0", "#c96a3a"],
            },
        }
    }

    fn caption_font(self) -> &'static str {
        match self {
            Self::Documentary => "Hanken Grotesk",
            Self::Kinetic => "Inter",
            Self::Cinematic => "Hanken Grotesk",
        }
    }

    fn caption_weight(self) -> i64 {
        match self {
            Self::Documentary => 700,
            Self::Kinetic => 800,
            Self::Cinematic => 600,
        }
    }

    fn motion_language(self) -> &'static str {
        match self {
            Self::Documentary => {
                "observational push-ins, evidence cards and clean cuts that follow the claim"
            }
            Self::Kinetic => "measured push-ins, snap entrances and one accent per beat",
            Self::Cinematic => {
                "slow push-ins, held frames and dissolves that let the performance breathe"
            }
        }
    }

    fn motion_easing(self) -> &'static str {
        match self {
            Self::Documentary => "editorial-settle",
            Self::Kinetic => "spring-pop",
            Self::Cinematic => "soft-ease",
        }
    }

    fn motion_intensity(self) -> f64 {
        match self {
            Self::Documentary => 0.45,
            Self::Kinetic => 0.75,
            Self::Cinematic => 0.35,
        }
    }

    fn sound_texture(self) -> &'static str {
        match self {
            Self::Documentary => "dry room, low evidence-bed, clean transition swells",
            Self::Kinetic => "dry room, low pulse, airy transitions",
            Self::Cinematic => "room tone, sparse low end, long reverb tails",
        }
    }

    fn footage_continuity(self) -> &'static str {
        match self {
            Self::Documentary => {
                "one camera height per section, evidence inserts matched to the claim, no letterbox"
            }
            Self::Kinetic => "one camera height, no letterbox, colour temperature follows the act",
            Self::Cinematic => {
                "one camera height, slow movement, colour temperature follows the performance"
            }
        }
    }

    fn continuity_rules(self) -> [&'static str; 4] {
        [
            "fill every frame; never letterbox a vertical reel",
            "one accent colour per section",
            "no two consecutive shots share a camera height",
            match self {
                Self::Documentary => "every claim's shot changes the evidence, not only the angle",
                Self::Kinetic => {
                    "no B-roll style, camera move, caption animation or transition repeats twice running"
                }
                Self::Cinematic => {
                    "hold a shot only while the performance changes; never repeat a move for filler"
                }
            },
        ]
    }

    fn query_suffix(self, index: usize) -> &'static str {
        let suffixes = match self {
            Self::Documentary => [
                "archival evidence",
                "observational wide",
                "detail insert",
                "location establishing",
            ],
            Self::Kinetic => [
                "macro detail",
                "fast tracking",
                "graphic close-up",
                "wide establishing",
            ],
            Self::Cinematic => [
                "cinematic wide",
                "slow push",
                "intimate close-up",
                "atmospheric detail",
            ],
        };
        suffixes[index % suffixes.len()]
    }

    fn category(self, index: usize) -> &'static str {
        let categories = match self {
            Self::Documentary => ["evidence", "place", "detail", "archive"],
            Self::Kinetic => ["detail", "motion", "graphic", "establishing"],
            Self::Cinematic => ["atmosphere", "performance", "detail", "place"],
        };
        categories[index % categories.len()]
    }

    fn shot_scale(self, index: usize, act: &str) -> &'static str {
        match act {
            "hook" => "wide",
            "turn" | "reveal" => "close_up",
            "tension" => "medium",
            _ => match (index + self as usize) % 3 {
                0 => "wide",
                1 => "medium",
                _ => "close_up",
            },
        }
    }

    fn entrance(self, index: usize, act: &str) -> &'static str {
        match act {
            "hook" | "reveal" | "turn" => "punch",
            "tension" => "whip",
            _ => {
                let entrances = match self {
                    Self::Documentary => ["dissolve", "slide", "pop", "punch"],
                    Self::Kinetic => ["punch", "slide", "pop", "whip"],
                    Self::Cinematic => ["dissolve", "pop", "slide", "punch"],
                };
                entrances[index % entrances.len()]
            }
        }
    }

    fn next_entrance(self, current: &str) -> &'static str {
        let entrances = match self {
            Self::Documentary => ["dissolve", "slide", "pop", "punch"],
            Self::Kinetic => ["punch", "slide", "pop", "whip"],
            Self::Cinematic => ["dissolve", "pop", "slide", "punch"],
        };
        let at = entrances
            .iter()
            .position(|entrance| *entrance == current)
            .unwrap_or(0);
        entrances[(at + 1) % entrances.len()]
    }
}

fn keyword_hits(text: &str, words: &[&str]) -> f64 {
    words
        .iter()
        .map(|word| {
            if word.contains(' ') {
                text.matches(word).count()
            } else {
                text.split(|character: char| !character.is_alphanumeric())
                    .filter(|token| *token == *word)
                    .count()
            }
        })
        .sum::<usize>() as f64
}

/// Score the three directions on the material the run actually has: the
/// transcript's topic words, the frame, the runtime and the measured
/// delivery. A long, evidence-heavy piece is not directed like a vertical
/// tech explainer, and a story with a wide emotional arc is not directed
/// like a finance brief.
fn direction_scores(
    spoken: &[String],
    subjects: &[String],
    read: &[EmotionLine],
    runtime: f64,
    aspect: &str,
) -> [(VisualDirection, f64); 3] {
    let text = format!("{} {}", spoken.join(" "), subjects.join(" ")).to_lowercase();
    let evidence = keyword_hits(
        &text,
        &[
            "evidence",
            "archive",
            "investigation",
            "witness",
            "report",
            "documentary",
            "case",
            "found",
            "hidden",
            "secret",
        ],
    );
    let history = keyword_hits(
        &text,
        &[
            "history",
            "ancient",
            "century",
            "war",
            "empire",
            "king",
            "queen",
            "revolution",
            "artifact",
            "archaeology",
            "civilization",
        ],
    );
    let science = keyword_hits(
        &text,
        &[
            "space",
            "planet",
            "ocean",
            "sea",
            "physics",
            "biology",
            "chemistry",
            "brain",
            "climate",
            "species",
            "energy",
            "star",
            "universe",
            "experiment",
            "research",
        ],
    );
    let finance = keyword_hits(
        &text,
        &[
            "money",
            "market",
            "invest",
            "stock",
            "price",
            "cost",
            "profit",
            "revenue",
            "economy",
            "bank",
            "loan",
            "salary",
            "wage",
            "tax",
            "debt",
            "budget",
            "inflation",
        ],
    );
    let technology = keyword_hits(
        &text,
        &[
            "ai",
            "software",
            "code",
            "computer",
            "algorithm",
            "robot",
            "machine",
            "data",
            "model",
            "internet",
            "app",
            "device",
            "chip",
            "quantum",
            "program",
            "system",
        ],
    );
    let how_to = keyword_hits(
        &text,
        &[
            "how to", "step", "guide", "tutorial", "learn", "build", "make", "start", "beginner",
            "tip", "mistake", "fix", "setup",
        ],
    );
    let gaming = keyword_hits(
        &text,
        &[
            "game", "player", "level", "quest", "boss", "speedrun", "console", "graphics",
        ],
    );
    let fitness = keyword_hits(
        &text,
        &[
            "body", "muscle", "workout", "training", "run", "diet", "weight", "health", "sleep",
            "habit",
        ],
    );
    let story = keyword_hits(
        &text,
        &[
            "story",
            "journey",
            "character",
            "dream",
            "fear",
            "love",
            "loss",
            "remember",
            "childhood",
            "family",
            "home",
            "hope",
            "heart",
        ],
    );
    let luxury = keyword_hits(
        &text,
        &[
            "luxury",
            "watch",
            "car",
            "fashion",
            "brand",
            "design",
            "expensive",
            "gold",
            "premium",
        ],
    );
    let horror = keyword_hits(
        &text,
        &[
            "haunted", "ghost", "dark", "night", "scream", "blood", "dead", "curse", "monster",
            "shadow",
        ],
    );
    let vertical = matches!(aspect, "9:16" | "4:5" | "1:1");
    let punchy = read.iter().filter(|line| line.pace == "punchy").count() as f64;
    let slow = read.iter().filter(|line| line.pace == "slow").count() as f64;
    let distinct_emotions = read
        .iter()
        .map(|line| line.emotion.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as f64;
    let documentary = 1.0
        + evidence * 2.0
        + history * 1.6
        + science * 1.1
        + finance * 0.5
        + slow * 0.6
        + if runtime > 60.0 { 2.0 } else { 0.0 }
        + if vertical { 0.0 } else { 0.6 };
    let kinetic = 1.0
        + technology * 1.7
        + finance * 1.2
        + how_to * 1.7
        + gaming * 1.2
        + fitness * 1.1
        + punchy * 0.7
        + if vertical { 2.2 } else { 0.0 }
        + if runtime <= 60.0 { 1.0 } else { 0.0 };
    let cinematic = 1.0
        + story * 1.8
        + history * 1.0
        + science * 1.2
        + luxury * 1.2
        + horror * 1.2
        + distinct_emotions * 0.7
        + if runtime > 30.0 { 1.0 } else { 0.0 }
        + if vertical { 0.0 } else { 1.0 };
    [
        (VisualDirection::Documentary, documentary),
        (VisualDirection::Kinetic, kinetic),
        (VisualDirection::Cinematic, cinematic),
    ]
}

fn choose_direction(
    spoken: &[String],
    subjects: &[String],
    read: &[EmotionLine],
    runtime: f64,
    aspect: &str,
) -> VisualDirection {
    direction_scores(spoken, subjects, read, runtime, aspect)
        .into_iter()
        .max_by(|(_, left), (_, right)| left.total_cmp(right))
        .map(|(direction, _)| direction)
        .unwrap_or(VisualDirection::Kinetic)
}

fn direction_alternatives(
    spoken: &[String],
    subjects: &[String],
    read: &[EmotionLine],
    runtime: f64,
    aspect: &str,
) -> Vec<Value> {
    let chosen = choose_direction(spoken, subjects, read, runtime, aspect);
    direction_scores(spoken, subjects, read, runtime, aspect)
        .into_iter()
        .map(|(direction, score)| {
            json!({
                "name": direction.label(),
                "score": round3(score),
                "selected": direction == chosen,
                "why": direction.reference(),
            })
        })
        .collect()
}

/// The five-beat skeleton a one-click edit owes: hook, curiosity gap,
/// progression, payoff and ending. The hook is the opening three seconds;
/// the turn is the strongest measured act; the payoff is the last spoken
/// line; the ending is the short landing after it. A reel keeps the shape
/// compact; a longform cut gets an intro, a re-hook at the retention dip,
/// a CTA and an outro.
fn story_sections(runtime: f64, mode: &str, turn_at: f64, payoff_at: f64) -> Value {
    if runtime <= 4.0 {
        return json!([{ "role": "hook", "start": 0.0, "end": round3(runtime) }]);
    }
    let hook_end = 3.0_f64.min(runtime * 0.25);
    let curiosity_end = if mode == "longform" {
        (runtime * 0.2).clamp(5.0, 12.0).min(runtime * 0.35)
    } else {
        (runtime * 0.2).clamp(5.0, 12.0).min(runtime * 0.45)
    }
    .max(hook_end + 0.3)
    .min(runtime - 0.5);
    if mode == "longform" && runtime > 20.0 {
        let rehook = turn_at
            .clamp(8.0, (runtime - 8.0).max(8.0))
            .max(curiosity_end + 0.5);
        let rehook_end = (rehook + 2.0).min(runtime - 4.0).max(rehook + 0.25);
        let payoff_floor = rehook_end + 0.5;
        let payoff_start = payoff_at.clamp(payoff_floor, (runtime - 3.0).max(payoff_floor));
        let payoff_end = (payoff_start + 2.5)
            .min(runtime - 1.5)
            .max(payoff_start + 0.25);
        let ending_start = (runtime - 2.0).max(payoff_end + 0.25).min(runtime - 0.5);
        return json!([
            { "role": "hook", "start": 0.0, "end": round3(hook_end) },
            { "role": "intro", "start": round3(hook_end), "end": 8.0 },
            { "role": "curiosity", "start": 8.0, "end": round3(curiosity_end) },
            { "role": "progression", "start": round3(curiosity_end), "end": round3(rehook) },
            { "role": "rehook", "start": round3(rehook), "end": round3(rehook_end) },
            { "role": "progression", "start": round3(rehook_end), "end": round3(payoff_start) },
            { "role": "payoff", "start": round3(payoff_start), "end": round3(payoff_end) },
            { "role": "cta", "start": round3(payoff_end), "end": round3(ending_start) },
            { "role": "ending", "start": round3(ending_start), "end": round3(runtime) },
        ]);
    }
    let payoff_floor = curiosity_end + 0.5;
    let payoff_start = payoff_at.clamp(payoff_floor, (runtime - 1.0).max(payoff_floor));
    let payoff_end = (payoff_start + (runtime * 0.08).max(0.5))
        .min(runtime - 0.2)
        .max(payoff_start + 0.1)
        .min(runtime);
    json!([
        { "role": "hook", "start": 0.0, "end": round3(hook_end) },
        { "role": "curiosity", "start": round3(hook_end), "end": round3(curiosity_end) },
        { "role": "progression", "start": round3(curiosity_end), "end": round3(payoff_start) },
        { "role": "payoff", "start": round3(payoff_start), "end": round3(payoff_end) },
        { "role": "ending", "start": round3(payoff_end), "end": round3(runtime) },
    ])
}

/// The first `count` words of a line, with an ellipsis when it was cut.
fn first_words(text: &str, count: usize) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() <= count {
        return words.join(" ");
    }
    format!("{}…", words[..count].join(" "))
}

/// A brief field as text; the empty string when the run left it absent.
fn brief_text(brief: &serde_json::Map<String, Value>, key: &str) -> String {
    brief
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim()
        .to_owned()
}

/// The YouTube creative profile the narration's own words support. The
/// profile changes pacing, vocabulary and packaging, not the edit's
/// measured facts; the second value names the words that chose it.
fn youtube_profile(moments: &[(f64, f64, String)]) -> (&'static str, &'static str) {
    let text = moments
        .iter()
        .map(|(_, _, line)| line.as_str())
        .collect::<Vec<&str>>()
        .join(" ")
        .to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|word| text.contains(word));
    if has(&[
        "invest", "market", "stock", "money", "finance", "economy", "bank", "price",
    ]) {
        ("finance", "the narration names money, markets or investing")
    } else if has(&["learn", "explain", "how", "guide", "understand", "lesson"]) {
        ("education", "the narration teaches or explains a process")
    } else if has(&["secret", "history", "story", "happened", "mystery", "wreck"]) {
        ("storytelling", "the narration carries a story with a turn")
    } else if has(&["ai", "software", "technology", "code", "computer", "device"]) {
        ("technology", "the narration is about a tool or a system")
    } else if has(&["report", "news", "today", "latest", "announced"]) {
        ("news", "the narration reports a current event")
    } else if has(&["motivation", "mindset", "discipline", "habit", "success"]) {
        ("motivation", "the narration is a mindset argument")
    } else {
        (
            "general",
            "the narration names no subject narrow enough to profile",
        )
    }
}

/// A line ends a thought when its last non-quote character is punctuation.
fn sentence_ends(text: &str) -> bool {
    text.trim()
        .trim_end_matches(['"', '\'', '”', '’'])
        .chars()
        .last()
        .is_some_and(|last| matches!(last, '.' | '!' | '?'))
}

/// The transcript's own candidate Shorts: sliding windows of whole lines,
/// scored on the measurements the run already has - whether the opening
/// asks a question, how strong the delivery is, whether the window lands
/// on a payoff, and how much of it is actually spoken. The plan keeps the
/// top three and says which measurement gave each its score.
fn short_candidates(
    moments: &[(f64, f64, String)],
    read: &[EmotionLine],
    runtime: f64,
    aspect: &str,
) -> Vec<Value> {
    if moments.is_empty() || runtime < 5.0 {
        return Vec::new();
    }
    let target = if aspect == "9:16" { 30.0 } else { 45.0 };
    let min_len = 12.0_f64.min(runtime);
    let max_len = 60.0_f64.min(runtime);
    let mut candidates = Vec::new();
    for (first, (start, _, opening)) in moments.iter().enumerate() {
        let mut end = *start;
        let mut last = first;
        for (index, (_, stop, line)) in moments.iter().enumerate().skip(first) {
            if *stop - *start > max_len {
                break;
            }
            end = *stop;
            last = index;
            if end - *start >= target && sentence_ends(line) {
                break;
            }
        }
        let duration = end - *start;
        if duration < min_len {
            continue;
        }
        let spoken: String = moments[first..=last]
            .iter()
            .map(|(_, _, line)| line.as_str())
            .collect::<Vec<&str>>()
            .join(" ");
        let hook = first_words(opening, 12);
        let payoff = first_words(&moments[last].2, 12);
        let question = opening.contains('?')
            || hook.split_whitespace().take(3).any(|word| {
                matches!(
                    word.to_ascii_lowercase().as_str(),
                    "why" | "how" | "what" | "who"
                )
            });
        let payoff_signal = {
            let lower = moments[last].2.to_ascii_lowercase();
            lower.contains("that's why")
                || lower.contains("so that")
                || lower.contains("the result")
                || lower.contains("which means")
        };
        let intensity = read
            .iter()
            .filter(|line| line.start >= *start && line.start <= end)
            .map(|line| line.intensity)
            .fold(0.0_f64, f64::max);
        let coverage = moments[first..=last]
            .iter()
            .map(|(from, to, _)| (to - from).max(0.0))
            .sum::<f64>()
            / duration.max(0.1);
        let mut score = 0.15 + coverage.min(1.0) * 0.25 + intensity * 0.25;
        if question {
            score += 0.15;
        }
        if payoff_signal {
            score += 0.15;
        }
        if (20.0..=55.0).contains(&duration) {
            score += 0.05;
        }
        let words = spoken.split_whitespace().count();
        let standalone = words >= 20 && !hook.is_empty() && !payoff.is_empty();
        let standalone_why = if standalone {
            format!("the window opens with a hook and lands a payoff in {words} words")
        } else {
            format!("the window has {words} words; check that it still makes sense alone")
        };
        let why = format!(
            "{} seconds, {:.0}% spoken, delivery {:.2}{}{}",
            duration.round(),
            coverage.min(1.0) * 100.0,
            intensity,
            if question {
                ", opens on a question"
            } else {
                ""
            },
            if payoff_signal {
                ", lands on a payoff"
            } else {
                ""
            }
        );
        candidates.push(json!({
            "start": round3(*start),
            "end": round3(end),
            "duration": round3(duration),
            "hook": hook,
            "payoff": payoff,
            "score": round3(score.min(1.0)),
            "scoreBreakdown": {
                "base": 0.15,
                "spokenCoverage": round3(coverage.min(1.0) * 0.25),
                "delivery": round3(intensity * 0.25),
                "question": if question { 0.15 } else { 0.0 },
                "payoff": if payoff_signal { 0.15 } else { 0.0 },
                "durationBand": if (20.0..=55.0).contains(&duration) { 0.05 } else { 0.0 },
            },
            "standalone": standalone,
            "standaloneWhy": standalone_why,
            "thumbnailAt": round3(*start + duration / 2.0),
            "reframe": "9:16 fill, subject-aware crop",
            "captions": "word-level, centered",
            "why": why,
            "text": spoken,
        }));
    }
    candidates.sort_by(|left, right| {
        right["score"]
            .as_f64()
            .unwrap_or_default()
            .total_cmp(&left["score"].as_f64().unwrap_or_default())
    });
    let mut picked: Vec<Value> = Vec::new();
    for candidate in candidates {
        let start = candidate["start"].as_f64().unwrap_or_default();
        if picked
            .iter()
            .any(|kept| (kept["start"].as_f64().unwrap_or_default() - start).abs() < 5.0)
        {
            continue;
        }
        picked.push(candidate);
        if picked.len() == 3 {
            break;
        }
    }
    picked
}

/// The retention map: every story section, the measured risks inside it,
/// and the one fix the edit vocabulary can honestly apply there.
fn retention_map(sections: &Value, findings: &Value, runtime: f64) -> Value {
    let overlap = |entry: &Value| -> f64 {
        let start = entry["start"].as_f64().unwrap_or_default();
        let end = entry["end"].as_f64().unwrap_or(start);
        (end - start).max(0.0)
    };
    let empty = Vec::new();
    let boring = findings["boringStretches"].as_array().unwrap_or(&empty);
    let repeated = findings["repeatedExplanations"]
        .as_array()
        .unwrap_or(&empty);
    let weak = findings["weakTransitions"].as_array().unwrap_or(&empty);
    let list = sections.as_array().cloned().unwrap_or_default();
    let map: Vec<Value> = list
        .iter()
        .map(|section| {
            let role = section["role"].as_str().unwrap_or("body");
            let start = section["start"].as_f64().unwrap_or_default();
            let end = section["end"].as_f64().unwrap_or(runtime);
            let span = (end - start).max(1.0);
            let inside = |entry: &&Value| {
                let at = entry["start"].as_f64().unwrap_or(-1.0);
                at >= start && at < end
            };
            let boring_here: f64 = boring
                .iter()
                .filter(inside)
                .map(overlap)
                .fold(0.0_f64, f64::max);
            let repeated_here = repeated.iter().filter(inside).count();
            let weak_here = weak.iter().filter(inside).count();
            let score = ((boring_here / span).min(1.0) * 0.7
                + (repeated_here.min(2) as f64) * 0.1
                + (weak_here.min(2) as f64) * 0.1)
                .min(1.0);
            let risk = if score >= 0.5 {
                "high"
            } else if score >= 0.2 {
                "medium"
            } else {
                "low"
            };
            let reason = if boring_here > 0.0 {
                format!(
                    "{:.0} seconds inside this section read as a low-energy or same-mood stretch",
                    boring_here
                )
            } else if repeated_here > 0 {
                "the narration repeats an explanation here".to_owned()
            } else if weak_here > 0 {
                "the cut keeps the same act and mood across a transition here".to_owned()
            } else {
                "no measured retention risk in this section".to_owned()
            };
            let fix = match (risk, role) {
                ("low", _) => "none: measured clean",
                (_, "hook") => "open on the strongest measured question and cut the setup",
                (_, "curiosity") => "move the first value delivery earlier and name the question",
                (_, "progression") => {
                    "rotate shot scale or entrance at the risk time; add a B-roll query change"
                }
                (_, "payoff") => {
                    "hold the reveal long enough to land; do not cut away before the answer"
                }
                (_, "ending") => "end on the payoff and keep the next-video invitation short",
                _ => "add a pattern break at the measured risk time",
            };
            json!({
                "role": role,
                "start": round3(start),
                "end": round3(end),
                "risk": risk,
                "score": round3(score),
                "reason": reason,
                "fix": fix,
            })
        })
        .collect();
    json!(map)
}

/// Five title directions, each tied to a promise the video actually
/// makes. The `promise` is the brief's own payoff or thesis, so a title
/// cannot promise something the edit does not deliver.
fn title_concepts(
    subjects: &[String],
    brief: &serde_json::Map<String, Value>,
    shorts: &[Value],
) -> Value {
    let subject = subjects.first().map(String::as_str).unwrap_or("this story");
    let thesis = brief_text(brief, "thesis");
    let payoff = brief_text(brief, "payoff");
    let gap = brief_text(brief, "curiosityGap");
    let promise = if payoff.is_empty() {
        thesis.clone()
    } else {
        payoff
    };
    let short_hook = shorts
        .first()
        .and_then(|short| short["hook"].as_str())
        .unwrap_or_default();
    let mut titles: Vec<Value> = Vec::new();
    let mut push = |title: String, angle: &str, basis: &str| {
        if title.trim().is_empty()
            || titles
                .iter()
                .any(|kept| kept["title"].as_str() == Some(title.as_str()))
        {
            return;
        }
        titles.push(json!({
            "title": title,
            "angle": angle,
            "promise": promise,
            "basis": basis,
            "honest": true,
        }));
    };
    push(
        format!("Why {subject} is not what you think"),
        "curiosity",
        &gap,
    );
    push(format!("What {subject} actually costs"), "stakes", &promise);
    push(format!("How {subject} really works"), "how-to", &thesis);
    push(
        format!("Nobody explains {subject} like this"),
        "contrarian",
        &gap,
    );
    push(
        if short_hook.is_empty() {
            format!("The {subject} story, from the start")
        } else {
            format!("The moment {}", first_words(short_hook, 8))
        },
        "story",
        &promise,
    );
    push(
        format!("{subject}: the part that matters"),
        "payoff",
        &promise,
    );
    titles.truncate(5);
    json!(titles)
}

/// Three thumbnail concepts. No image is rendered here: each names the
/// frame a `preview_frame` call should sample and the text and
/// composition a designer or the motion layer should apply.
fn thumbnail_concepts(
    sections: &Value,
    subjects: &[String],
    titles: &Value,
    runtime: f64,
) -> Value {
    let section_at = |role: &str, fallback: f64| -> f64 {
        sections
            .as_array()
            .and_then(|list| {
                list.iter()
                    .find(|section| section["role"].as_str() == Some(role))
            })
            .map(|section| {
                let start = section["start"].as_f64().unwrap_or(fallback);
                let end = section["end"].as_f64().unwrap_or(start);
                (start + (end - start) * 0.5).clamp(0.0, runtime)
            })
            .unwrap_or(fallback)
    };
    let subject = subjects
        .first()
        .map(String::as_str)
        .unwrap_or("the speaker");
    let title = |index: usize| -> String {
        titles
            .as_array()
            .and_then(|list| list.get(index))
            .and_then(|entry| entry["title"].as_str())
            .unwrap_or(subject)
            .to_owned()
    };
    let concepts = [
        (
            section_at("hook", runtime * 0.1),
            first_words(&title(0), 4),
            "subject left, text right, high contrast",
            "the opening question is the promise the thumbnail must carry",
        ),
        (
            section_at("payoff", runtime * 0.8),
            first_words(&title(1), 4),
            "subject right, text left, one bright accent",
            "the payoff frame is the proof the title can stand on",
        ),
        (
            section_at("progression", runtime * 0.5),
            first_words(&title(2), 4),
            "centered subject, text top, shallow depth",
            "the middle frame shows the work, not the claim",
        ),
    ];
    json!(
        concepts
            .iter()
            .map(|(frame, text, composition, reason)| json!({
                "frameAt": round3(*frame),
                "subject": subject,
                "text": text.to_uppercase(),
                "composition": composition,
                "reason": reason,
            }))
            .collect::<Vec<Value>>()
    )
}

/// One plan-level quality check: `pass`, `warn` or `fail`, with the
/// measured value that decided it.
fn quality_check(name: &str, ok: bool, warn_only: bool, detail: String) -> Value {
    json!({
        "check": name,
        "verdict": if ok {
            "pass"
        } else if warn_only {
            "warn"
        } else {
            "fail"
        },
        "detail": detail,
    })
}

/// The plan-level quality report: the structure the edit promised, checked
/// before a frame is rendered. It cannot see a black frame or a clipped
/// sample - `watch_render` and `craft_critique` do that - so it says only
/// what the plan can prove.
fn youtube_quality(
    sections: &Value,
    findings: &Value,
    variety: &Value,
    titles: &Value,
    thumbnails: &Value,
    shorts: &[Value],
    segments: &[Value],
    runtime: f64,
) -> Value {
    let section = |role: &str| {
        sections.as_array().and_then(|list| {
            list.iter()
                .find(|entry| entry["role"].as_str() == Some(role))
        })
    };
    let span = |role: &str| -> f64 {
        section(role)
            .map(|entry| {
                let start = entry["start"].as_f64().unwrap_or_default();
                let end = entry["end"].as_f64().unwrap_or(start);
                (end - start).max(0.0)
            })
            .unwrap_or(0.0)
    };
    let payoff_at = section("payoff")
        .and_then(|entry| entry["start"].as_f64())
        .unwrap_or_default();
    let risk_count = findings["boringStretches"]
        .as_array()
        .map(Vec::len)
        .unwrap_or_default()
        + findings["repeatedExplanations"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default()
        + findings["weakTransitions"]
            .as_array()
            .map(Vec::len)
            .unwrap_or_default();
    let repeats = variety["consecutiveShotScaleRepeats"]
        .as_u64()
        .unwrap_or_default()
        + variety["consecutiveEntranceRepeats"]
            .as_u64()
            .unwrap_or_default()
        + variety["consecutiveCategoryRepeats"]
            .as_u64()
            .unwrap_or_default();
    let title_count = titles.as_array().map(Vec::len).unwrap_or_default();
    let thumbnail_count = thumbnails.as_array().map(Vec::len).unwrap_or_default();
    let thumbs_inside = thumbnails.as_array().is_some_and(|frames| {
        frames.iter().all(|frame| {
            frame["frameAt"]
                .as_f64()
                .is_some_and(|at| (0.0..=runtime + 0.05).contains(&at))
        })
    });
    let shorts_inside = shorts.iter().all(|short| {
        short["start"].as_f64().is_some_and(|at| at >= 0.0)
            && short["end"].as_f64().is_some_and(|at| at <= runtime + 0.05)
    });
    let checks = vec![
        quality_check(
            "hook within 3.5 s",
            span("hook") <= 3.5,
            true,
            format!("the hook section is {:.2} s", span("hook")),
        ),
        quality_check(
            "payoff after the turn",
            runtime <= 0.0 || (0.55..=0.99).contains(&(payoff_at / runtime)),
            true,
            format!(
                "the payoff starts at {:.1}% of the runtime",
                if runtime > 0.0 {
                    payoff_at / runtime * 100.0
                } else {
                    0.0
                }
            ),
        ),
        quality_check(
            "no measured retention risk",
            risk_count == 0,
            true,
            format!("{risk_count} measured risk entries remain"),
        ),
        quality_check(
            "no repeated neighbour",
            repeats == 0,
            true,
            format!("{repeats} consecutive scale, entrance or category repeats"),
        ),
        quality_check(
            "five title directions",
            title_count == 5,
            false,
            format!("{title_count} titles are present"),
        ),
        quality_check(
            "three thumbnail frames inside the runtime",
            thumbnail_count == 3 && thumbs_inside,
            false,
            format!("{thumbnail_count} frames, all inside the timeline: {thumbs_inside}"),
        ),
        quality_check(
            "every Short inside the runtime",
            shorts_inside && (runtime < 5.0 || !shorts.is_empty()),
            true,
            format!("{} Short windows", shorts.len()),
        ),
        quality_check(
            "every segment has a picture",
            segments.iter().all(|segment| {
                segment
                    .pointer("/visual/type")
                    .and_then(Value::as_str)
                    .is_some_and(|kind| !kind.is_empty())
            }),
            false,
            format!("{} segments carry a visual type", segments.len()),
        ),
        quality_check(
            "the timeline is covered",
            runtime <= 0.0
                || segments
                    .iter()
                    .map(|segment| {
                        let start = segment["start"].as_f64().unwrap_or_default();
                        let end = segment["end"].as_f64().unwrap_or(start);
                        (end - start).max(0.0)
                    })
                    .sum::<f64>()
                    / runtime
                    >= 0.95,
            false,
            format!(
                "{:.1}% of the runtime is inside a segment",
                if runtime > 0.0 {
                    segments
                        .iter()
                        .map(|segment| {
                            let start = segment["start"].as_f64().unwrap_or_default();
                            let end = segment["end"].as_f64().unwrap_or(start);
                            (end - start).max(0.0)
                        })
                        .sum::<f64>()
                        / runtime
                        * 100.0
                } else {
                    100.0
                }
            ),
        ),
    ];
    let verdict = if checks.iter().any(|check| check["verdict"] == "fail") {
        "fail"
    } else if checks.iter().any(|check| check["verdict"] == "warn") {
        "warn"
    } else {
        "pass"
    };
    json!({
        "verdict": verdict,
        "checks": checks,
        "note": "plan-level checks only; black frames, clipping and caption placement are watch_render and craft_critique's business",
    })
}

/// What a creative profile changes in the measured edit: how often a
/// pattern break lands, how hard the motion hand pushes, how heavy the
/// captions sit, and which second query the stock shelf sees. The
/// profile is a lens on the same signals, never a replacement for them.
fn youtube_profile_plan(
    moments: &[(f64, f64, String)],
    direction: VisualDirection,
    runtime: f64,
) -> Value {
    let (profile, why) = youtube_profile(moments);
    let (motion_delta, breaks_every, qualifier, caption_weight, target_shot_seconds) = match profile
    {
        "finance" => (0.05, 18.0, "financial documentary detail", 750, 3.5),
        "education" => (0.0, 18.0, "clean explanatory visual", 700, 3.0),
        "storytelling" => (-0.05, 30.0, "cinematic story detail", 600, 4.5),
        "technology" => (0.05, 16.0, "technology product detail", 800, 2.8),
        "news" => (0.1, 14.0, "newsroom evidence", 800, 2.5),
        "motivation" => (0.1, 15.0, "motivational cinematic", 800, 2.5),
        _ => (0.0, 20.0, "cinematic establishing detail", 700, 3.5),
    };
    let motion = (direction.motion_intensity() + motion_delta).clamp(0.2, 0.9);
    json!({
        "profile": profile,
        "why": why,
        "pacing": {
            "targetShotSeconds": target_shot_seconds,
            "patternBreakEvery": breaks_every,
            "measuredRuntime": round3(runtime),
        },
        "motion": {
            "intensity": round3(motion),
            "reason": format!("{profile} adds {motion_delta:+.2} to the measured direction's own motion hand"),
        },
        "captions": { "weight": caption_weight },
        "broll": { "qualifier": qualifier },
    })
}

/// Give every B-roll query the profile's own second door. The first
/// query stays the words the span plays, so the shelf still ranks the
/// meaning first; the profile's qualifier is an additional candidate,
/// not a replacement for what the narration said.
fn apply_profile_queries(segments: &mut [Value], profile_plan: &Value) {
    let profile = profile_plan["profile"].as_str().unwrap_or("general");
    let qualifier = profile_plan["broll"]["qualifier"]
        .as_str()
        .unwrap_or("cinematic establishing detail");
    for segment in segments {
        let Some(query) = segment
            .pointer("/visual/query")
            .and_then(Value::as_str)
            .map(str::to_owned)
        else {
            continue;
        };
        let mut queries: Vec<Value> = segment
            .pointer("/visual/queries")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        queries.push(json!(format!("{query}, {qualifier}")));
        segment["visual"]["queries"] = json!(queries);
        segment["visual"]["profile"] = json!(profile);
        if let Some(description) = segment
            .pointer("/visual/description")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            segment["visual"]["description"] = json!(format!("{description}; {qualifier}"));
        }
    }
}

/// The YouTube package the plan carries: mode, creative profile, title
/// and thumbnail directions, the Shorts the transcript supports, and the
/// section-by-section retention map. It is derived from the same
/// measurements as the edit; nothing here invents a claim the plan cannot
/// support.
fn youtube_package(
    mode: &str,
    aspect: &str,
    runtime: f64,
    sections: &Value,
    segments: &[Value],
    brief: &serde_json::Map<String, Value>,
    subjects: &[String],
    moments: &[(f64, f64, String)],
    read: &[EmotionLine],
    findings: &Value,
    variety: &Value,
    profile_plan: &Value,
) -> Value {
    let profile = (
        profile_plan["profile"].as_str().unwrap_or("general"),
        profile_plan["why"]
            .as_str()
            .unwrap_or("the narration names no subject narrow enough to profile"),
    );
    let youtube_mode = if aspect == "9:16" {
        "short"
    } else if runtime > 60.0 {
        "longform"
    } else {
        "reel"
    };
    let mut shorts = short_candidates(moments, read, runtime, aspect);
    if shorts.is_empty() && runtime >= 5.0 {
        let text = moments
            .iter()
            .map(|(_, _, line)| line.as_str())
            .collect::<Vec<&str>>()
            .join(" ");
        shorts.push(json!({
            "start": 0.0,
            "end": round3(runtime),
            "duration": round3(runtime),
            "hook": first_words(&text, 12),
            "payoff": first_words(&text, 12),
            "score": 0.5,
            "why": "the whole take is short enough to stand alone; no longer window was measured",
            "text": text,
        }));
    }
    let titles = title_concepts(subjects, brief, &shorts);
    if let Some(list) = titles.as_array() {
        for (index, short) in shorts.iter_mut().enumerate() {
            if let Some(title) = list.get(index % list.len()) {
                short["title"] = title["title"].clone();
            }
        }
    }
    let thumbnails = thumbnail_concepts(sections, subjects, &titles, runtime);
    let retention = retention_map(sections, findings, runtime);
    let quality = youtube_quality(
        sections,
        findings,
        variety,
        &titles,
        &thumbnails,
        &shorts,
        segments,
        runtime,
    );
    json!({
        "mode": youtube_mode,
        "editMode": mode,
        "profile": profile.0,
        "profileWhy": profile.1,
        "profilePlan": profile_plan.clone(),
        "titles": titles,
        "thumbnails": thumbnails,
        "shorts": shorts,
        "retentionMap": retention,
        "quality": quality,
        "publishingChecklist": [
            "render each thumbnail's frameAt with preview_frame and choose one",
            "check the chosen title against the video's actual payoff",
            "review captions and the first three seconds at full size",
            "confirm the music and stock licences before publishing",
            "read the export back and check duration, aspect and audio",
        ],
        "note": "frameAt and short times are seconds on the finished timeline; no thumbnail image is rendered by this plan",
    })
}

/// The retention pass: find the stretches a viewer would leave, the
/// explanations the script repeats, and the cuts that keep the same act
/// and mood so they read as a continuation rather than a turn. The plan
/// turns the findings into extra pattern breaks and keeps them for the
/// critic; nothing is invented where the measurements are silent.
fn retention_analysis(
    spoken: &[String],
    segments: &[Value],
    read: &[EmotionLine],
    runtime: f64,
) -> Value {
    let mut boring: Vec<Value> = Vec::new();
    let mut repeated: Vec<Value> = Vec::new();
    let mut weak: Vec<Value> = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let start = segment["start"].as_f64().unwrap_or(0.0);
        let end = segment["end"].as_f64().unwrap_or(start);
        let intensity = read
            .iter()
            .filter(|line| line.start >= start - 0.05 && line.start < end)
            .map(|line| line.intensity)
            .fold(0.0_f64, f64::max);
        let same_as_previous = index > 0
            && segments[index - 1]["act"] == segment["act"]
            && segments[index - 1]["emotion"] == segment["emotion"];
        if end - start >= 8.0 && (intensity < 0.55 || same_as_previous) {
            boring.push(json!({
                "start": round3(start),
                "end": round3(end),
                "why": if same_as_previous {
                    "a long stretch that keeps the same act and mood as the shot before it"
                } else {
                    "a long, low-intensity stretch with no measured lift"
                },
            }));
        }
    }
    const STOP_WORDS: &[&str] = &[
        "the", "and", "that", "this", "with", "from", "because", "have", "were", "was", "are",
        "for", "but", "not", "you", "your", "they", "their", "them", "its", "into", "over",
        "under", "about", "when", "what", "which", "who", "how", "why", "will", "can", "just",
        "then", "than", "there", "here", "also", "very", "more", "most", "some", "such",
    ];
    let normalize = |word: &str| {
        word.trim_matches(|character: char| !character.is_alphanumeric())
            .to_lowercase()
    };
    let lines: Vec<Vec<String>> = spoken
        .iter()
        .map(|line| {
            line.split_whitespace()
                .map(normalize)
                .filter(|word| word.len() > 2 && !STOP_WORDS.contains(&word.as_str()))
                .collect()
        })
        .collect();
    let mut counts: std::collections::BTreeMap<String, (usize, f64)> =
        std::collections::BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        if line.len() < 3 {
            continue;
        }
        let at = if lines.is_empty() {
            0.0
        } else {
            runtime * (index as f64 + 0.5) / lines.len() as f64
        };
        for window in line.windows(3) {
            let entry = counts.entry(window.join(" ")).or_insert((0, at));
            entry.0 += 1;
        }
    }
    for (phrase, (count, at)) in counts {
        if count > 1 {
            repeated.push(json!({
                "phrase": phrase,
                "count": count,
                "at": round3(at),
                "why": "the same three-word idea returns, so the second telling needs a new image or a cut",
            }));
        }
    }
    repeated.sort_by_key(|entry| std::cmp::Reverse(entry["count"].as_u64().unwrap_or_default()));
    repeated.truncate(5);
    for pair in segments.windows(2) {
        let left = &pair[0];
        let right = &pair[1];
        if left["act"] == right["act"] && left["emotion"] == right["emotion"] {
            weak.push(json!({
                "at": right["start"],
                "why": "the cut keeps the same act and mood, so it reads as a continuation rather than a turn",
            }));
        }
    }
    boring.truncate(6);
    weak.truncate(6);
    json!({
        "boringStretches": boring,
        "repeatedExplanations": repeated,
        "weakTransitions": weak,
        "summary": format!(
            "{} long stretches, {} repeated explanations and {} weak transitions were found",
            boring.len(),
            repeated.len(),
            weak.len()
        ),
    })
}

/// The variety check the plan carries: the shot scales, entrances and
/// B-roll categories the cut actually uses, and how often two neighbours
/// repeat. A repeated category is a motif only when the plan says so; the
/// default is a bug.
fn visual_variety(segments: &[Value]) -> Value {
    let mut scales = std::collections::BTreeSet::new();
    let mut entrances = std::collections::BTreeSet::new();
    let mut categories = std::collections::BTreeSet::new();
    let mut repeated_scales = 0usize;
    let mut repeated_entrances = 0usize;
    let mut repeated_categories = 0usize;
    let mut last_scale: Option<&str> = None;
    let mut last_entrance: Option<&str> = None;
    let mut last_category: Option<&str> = None;
    for segment in segments {
        let scale = segment
            .pointer("/visual/shotScale")
            .and_then(Value::as_str)
            .unwrap_or("medium");
        let entrance = segment
            .pointer("/in/anim")
            .and_then(Value::as_str)
            .unwrap_or("pop");
        let category = segment
            .pointer("/visual/category")
            .and_then(Value::as_str)
            .unwrap_or("footage");
        scales.insert(scale);
        entrances.insert(entrance);
        categories.insert(category);
        if last_scale == Some(scale) {
            repeated_scales += 1;
        }
        if last_entrance == Some(entrance) {
            repeated_entrances += 1;
        }
        if last_category == Some(category) {
            repeated_categories += 1;
        }
        last_scale = Some(scale);
        last_entrance = Some(entrance);
        last_category = Some(category);
    }
    json!({
        "shotScales": scales.into_iter().collect::<Vec<_>>(),
        "entrances": entrances.into_iter().collect::<Vec<_>>(),
        "categories": categories.into_iter().collect::<Vec<_>>(),
        "consecutiveShotScaleRepeats": repeated_scales,
        "consecutiveEntranceRepeats": repeated_entrances,
        "consecutiveCategoryRepeats": repeated_categories,
        "note": "the plan rotates shot scale, entrance and B-roll category; a repeat is only allowed when the motif is the point",
    })
}

fn next_scale(current: &str) -> &'static str {
    match current {
        "wide" => "medium",
        "medium" => "close_up",
        _ => "wide",
    }
}

/// Rotate the plan's visual choices so no two neighbouring segments wear
/// the same shot scale or entrance, and give every B-roll query a
/// direction-specific second door. The gather stage's own no-repeat rule
/// still owns the clip ids, frame hashes and transitions.
fn vary_segments(segments: &mut [Value], direction: VisualDirection) {
    let mut last_anim: Option<String> = None;
    let mut last_scale: Option<&str> = None;
    for (index, segment) in segments.iter_mut().enumerate() {
        let act = segment
            .get("act")
            .and_then(Value::as_str)
            .unwrap_or("body")
            .to_owned();
        let mut scale = direction.shot_scale(index, &act);
        if last_scale == Some(scale) {
            scale = next_scale(scale);
        }
        last_scale = Some(scale);
        segment["visual"]["shotScale"] = json!(scale);
        if index == 0 {
            segment["visual"]["hero"] = json!(true);
        }
        if let Some(query) = segment
            .pointer("/visual/query")
            .and_then(Value::as_str)
            .map(str::to_owned)
        {
            let decorated = format!("{query}, {}", direction.query_suffix(index));
            segment["visual"]["category"] = json!(direction.category(index));
            segment["visual"]["queries"] = json!([query, decorated.clone()]);
            segment["visual"]["query"] = json!(decorated);
        }
        let mut anim = if segment.get("beat").is_some() {
            if last_anim.as_deref() == Some("punch") {
                "whip".to_owned()
            } else {
                "punch".to_owned()
            }
        } else {
            direction.entrance(index, &act).to_owned()
        };
        if last_anim.as_deref() == Some(anim.as_str()) {
            anim = direction.next_entrance(&anim).to_owned();
        }
        segment["in"]["anim"] = json!(anim);
        last_anim = Some(anim);
    }
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
    // The music bed is the Music stage's own business, not a segment:
    // a bed clip under the narration (a retry that already landed one)
    // planned as B-roll would shop music as footage and overlap the
    // voice's own span, which the validator refuses - refusing a plan
    // for exactly the shape the run itself had built.
    let clips: Vec<&Value> = view["timeline"]["clips"]
        .as_array()?
        .iter()
        .filter(|clip| {
            clip["media"].as_str().is_some_and(|id| {
                media
                    .iter()
                    .find(|(mid, _)| mid == id)
                    .map(|(_, path)| !path.contains("/bgm/"))
                    .unwrap_or(true)
            })
        })
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
    let reads = voice_reads(log, takes);
    // The segments are the spine as the handoff left it: one span per
    // placed clip, MERGED where clips overlap - an a-roll handoff places
    // the voice and its face together, and one span of timeline cannot
    // carry two segments, or the validator refuses the plan for exactly
    // the shape the run was handed. An audio clip is not a picture -
    // claiming its span as `existing` footage was the plan's own
    // black-span defect, the validator refusing exactly that claim - so
    // a narration clip's span is planned as the B-roll the gather will
    // place, shopped with the words the span plays, and only a clip that
    // really puts pixels on screen is `existing` (with its kind stated,
    // so the claim can be checked). Where a picture covers a span, the
    // picture is the segment and the words it plays need no shopping:
    // that is the a-roll's own grammar.
    /// One clip's contribution to the spine: when it opens and closes,
    /// the picture it puts on screen (if it puts one), the read's own
    /// mood at the moment it opens, the strongest stressed word inside
    /// its source span (mapped to timeline seconds), and the words it
    /// plays.
    type Span = (
        f64,
        f64,
        Option<(String, String)>,
        Option<EmotionLine>,
        Option<f64>,
        String,
    );
    let span_of = |clip: &Value| -> Span {
        let start = clip["start"].as_f64().unwrap_or(0.0);
        let end = start + clip["duration"].as_f64().unwrap_or(0.0);
        let kind = clip["kind"].as_str().unwrap_or("audio").to_owned();
        let is_visual = matches!(
            kind.as_str(),
            "video" | "image" | "motion" | "text" | "shape"
        );
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
        // The span's own words, both as the shopping query and as the
        // semantic description the shelf ranks against: what the
        // narration says while the shot plays is what the shot should
        // show. A span too short to carry the description floor states
        // the measured arc it sits in instead - never a padded fake.
        let (source_in, source_out, speed) = clip_file_span(clip);
        let clip_path = clip["media"].as_str().and_then(|id| {
            media
                .iter()
                .find(|(mid, _)| mid == id)
                .map(|(_, path)| path.clone())
        });
        let beat = clip_path.as_deref().and_then(|path| {
            emphasis_moment(&reads, path, source_in, source_out)
                .map(|at| start + (at - source_in) / speed)
        });
        let spoken: String = transcripts_of(log, takes)
            .into_iter()
            .find(|(take_path, _)| clip_path.as_deref() == Some(take_path.as_str()))
            .map(|(_, segments)| {
                segments
                    .iter()
                    .filter_map(|row| {
                        let from = row["start"].as_f64()?;
                        let to = row["end"].as_f64()?;
                        (to > source_in && from < source_out)
                            .then(|| row["text"].as_str().unwrap_or_default().trim())
                    })
                    .collect::<Vec<&str>>()
                    .join(" ")
            })
            .unwrap_or_default();
        let visual = is_visual.then(|| {
            (
                clip["media"].as_str().unwrap_or_default().to_owned(),
                kind.clone(),
            )
        });
        (start, end, visual, mood, beat, spoken)
    };
    let mut spans: Vec<Span> = clips.iter().map(|clip| span_of(clip)).collect();
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Merge the overlapping spans into one window each: sorted by
    // start, a span that opens before the window closes joins it (its
    // end extends the window); a span that opens after the window
    // closes starts the next one.
    let mut windows: Vec<Span> = Vec::new();
    for span in spans {
        match windows.last_mut() {
            Some(window) if span.0 < window.1 => {
                window.1 = window.1.max(span.1);
                // The picture the window shows: real footage beats an
                // overlay - a text or shape clip riding a window never
                // displaces the video, image or motion the window
                // already shows, and a window with no picture takes any
                // picture it is offered. The first span's mood is kept
                // - the mood of the moment the window opens - and the
                // words the spans play join, so a window the gather
                // shops carries everything its narration said.
                let real_footage = span
                    .2
                    .as_ref()
                    .is_some_and(|(_, kind)| matches!(kind.as_str(), "video" | "image" | "motion"));
                let window_is_overlay = window.2.as_ref().is_some_and(|(_, kind)| {
                    !matches!(kind.as_str(), "video" | "image" | "motion")
                });
                if window.2.is_none() || (real_footage && window_is_overlay) {
                    window.2 = span.2.clone();
                }
                if window.4.is_none() {
                    window.4 = span.4;
                }
                if !span.5.trim().is_empty() {
                    if !window.5.trim().is_empty() {
                        window.5.push(' ');
                    }
                    window.5.push_str(span.5.trim());
                }
            }
            _ => windows.push(span),
        }
    }
    let mut segments: Vec<Value> = windows
        .iter()
        .enumerate()
        .map(|(index, (start, end, visual, mood, beat, spoken))| {
            let start = *start;
            let end = *end;
            let beat = *beat;
            let spoken = spoken.trim().to_owned();
            let mood = mood.clone();
            let visual = visual.clone();
            let mut segment = match visual {
                Some((media_id, kind)) => json!({
                    "start": round3(start),
                    "end": round3(end),
                    "visual": {
                        "type": "existing",
                        "mediaId": media_id.as_str(),
                        "kind": kind.as_str(),
                    },
                }),
                None => {
                    let query = if spoken.is_empty() {
                        mood.as_ref()
                            .and_then(|line| line.queries.first().cloned())
                            .unwrap_or_else(|| "cinematic establishing shot".to_owned())
                    } else {
                        spoken.clone()
                    };
                    let description = if spoken.chars().count() >= 40 {
                        spoken.clone()
                    } else {
                        // The measured facts the run can stand behind: the
                        // act, the register and the span's own subject, stated
                        // as the description - padded with facts, never with
                        // filler.
                        let line = mood.as_ref();
                        format!(
                            "the narration's {} {} over {}, a {:.0} second piece",
                            line.map(|line| line.act.as_str()).unwrap_or("body"),
                            line.map(|line| line.emotion.as_str()).unwrap_or("neutral"),
                            query,
                            (end - start).max(1.0)
                        )
                    };
                    json!({
                        "start": round3(start),
                        "end": round3(end),
                        "visual": {
                            "type": "broll",
                            "provider": source.stock_provider,
                            "query": query,
                            "description": description,
                        },
                    })
                }
            };
            // Every clip enters on purpose (floor M): the opening pops,
            // a reveal or a turn punches, tension whips, the landing
            // dissolves, and the calm middle alternates slide and
            // dissolve - an entrance the gate can read, and a vocabulary
            // the timeline's transition pass already speaks.
            let last = index + 1 == windows.len();
            let anim = match mood.as_ref().map(|line| line.act.as_str()) {
                Some("hook") | Some("reveal") | Some("turn") => "punch",
                Some("tension") => "whip",
                _ if index == 0 => "pop",
                _ if last => "dissolve",
                _ if index % 2 == 0 => "slide",
                _ => "dissolve",
            };
            segment["in"] = json!({ "anim": anim });
            if let Some(line) = mood {
                segment["emotion"] = json!(line.emotion);
                segment["colourTemp"] = json!(line.temperature);
                segment["act"] = json!(line.act);
                segment["duckDb"] = json!(line.duck_db);
            }
            if let Some(at) = beat {
                segment["beat"] = json!({
                    "at": round3(at),
                    "signal": "emphasisWords",
                    "why": "the strongest stressed word inside the span; the entrance lands on it",
                });
                segment["in"]["anim"] = json!("punch");
            }
            segment
        })
        .collect();

    // The story skeleton: hook, curiosity gap, progression, payoff and
    // ending. The turn is the strongest measured act, the payoff is the
    // last spoken line, and a longform cut gets the intro and the re-hook
    // at the retention dip the validator asks for.
    let mode = if runtime > 60.0 { "longform" } else { "reel" };
    let turn_at = segments
        .iter()
        .find(|segment| segment.get("act").and_then(Value::as_str) == Some("turn"))
        .and_then(|segment| segment.get("start").and_then(Value::as_f64))
        .unwrap_or(runtime * 0.55);
    let payoff_at = read
        .iter()
        .map(|line| line.end)
        .fold(0.0_f64, f64::max)
        .clamp(runtime * 0.75, runtime);
    let sections = story_sections(runtime, mode, turn_at, payoff_at);
    // The brief, only where the narration answers it: the opening line is
    // the claim, the closing line its payoff, the ordered emotions the
    // arc. The GOAL is the button's own - a one-click edit is a retention
    // edit, and saying so is honest. The AUDIENCE is stated only when the
    // run's own measurements name a subject; a transcript that never says
    // what it is about leaves the field absent and the gate names it,
    // which is worth more than an audience invented to pass.
    let mut brief = serde_json::Map::new();
    brief.insert("goal".to_owned(), json!("watch to the end"));
    // The audience, only when the run's own reading names a subject: the
    // strongest entities the NER timed are what the piece is about, and a
    // transcript about nothing in particular leaves the field absent for
    // the gate to name - an honest blocker, not an invented viewer.
    let subjects: Vec<String> = {
        let mut named: Vec<String> = voice_reads(log, takes)
            .iter()
            .flat_map(|voice| voice.entities.iter())
            .filter(|entity| {
                matches!(
                    entity.kind.as_str(),
                    "ORG" | "PERSON" | "GPE" | "LOC" | "FAC" | "WORK_OF_ART" | "PRODUCT"
                )
            })
            .map(|entity| entity.text.clone())
            .collect();
        named.sort_by_key(|text| std::cmp::Reverse(text.len()));
        named.dedup();
        named.truncate(2);
        named
    };
    if !subjects.is_empty() {
        brief.insert(
            "audience".to_owned(),
            json!(format!("viewers interested in {}", subjects.join(" and "))),
        );
    }
    let transcript_moments: Vec<(f64, f64, String)> = takes
        .iter()
        .flat_map(|take| spoken_lines(log, &take.take))
        .filter_map(|line| {
            Some((
                line["start"].as_f64()?,
                line["end"].as_f64()?,
                line["text"].as_str()?.to_owned(),
            ))
        })
        .collect();
    let spoken: Vec<String> = transcript_moments
        .iter()
        .map(|(_, _, text)| text.clone())
        .collect();
    if let Some(first) = spoken.first() {
        // The opening line is the thesis; the first real question the
        // narration asks is the curiosity gap when one was measured, and
        // the opening line stands in when the script never asked.
        let gap = question_acts(log, takes)
            .into_iter()
            .next()
            .map(|question| question.text)
            .unwrap_or_else(|| first.clone());
        brief.insert("thesis".to_owned(), json!(first));
        brief.insert("curiosityGap".to_owned(), json!(gap));
    }
    if let Some(last) = spoken.last().filter(|_| spoken.len() > 1) {
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
    if arc.len() >= 3 {
        brief.insert("emotionalArc".to_owned(), json!(arc));
    }
    // The three-direction bake-off: score the material against a
    // documentary, a kinetic explainer and a cinematic essay, then let
    // the strongest direct the one look, the motion hand, the sound world
    // and the B-roll vocabulary. The plan keeps all three, so the choice
    // is inspectable rather than a mood in the code.
    let aspect = aspect_of(view);
    let direction = choose_direction(&spoken, &subjects, &read, runtime, aspect);
    let alternatives = direction_alternatives(&spoken, &subjects, &read, runtime, aspect);
    let profile_plan = youtube_profile_plan(&transcript_moments, direction, runtime);
    brief.insert("referenceStyle".to_owned(), json!(direction.reference()));
    brief.insert("tone".to_owned(), json!(direction.tone()));
    vary_segments(&mut segments, direction);
    apply_profile_queries(&mut segments, &profile_plan);
    let retention_findings = retention_analysis(&spoken, &segments, &read, runtime);
    let variety = visual_variety(&segments);
    let youtube = youtube_package(
        mode,
        aspect,
        runtime,
        &sections,
        &segments,
        &brief,
        &subjects,
        &transcript_moments,
        &read,
        &retention_findings,
        &variety,
        &profile_plan,
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
    // voice_signals vocabulary that actually drove it. The names are the
    // ones this run read - a plan that cites `pause` for a boundary no
    // silence measurement opened, or `emotion` for a bed the gate chose on
    // the noise floor, is a log that cannot be audited. So each entry is
    // built from the reading it stands on, and an entry with nothing
    // measured behind it is left out rather than dressed in a plausible
    // signal name.
    let measured_pauses = reads.iter().any(|read| !read.pauses.is_empty());
    // The words are the transcript's, not the profile's: the caption
    // stage sets the timed words whether or not the voice reading ran,
    // so the log owes a caption entry for the same reason.
    let measured_words = !transcripts_of(log, takes).is_empty()
        || reads
            .iter()
            .any(|read| read.lines.iter().any(|line| !line.text.is_empty()));
    let measured_emphasis = reads
        .iter()
        .any(|read| read.lines.iter().any(|line| !line.emphasis.is_empty()));
    let measured_pace = reads
        .iter()
        .any(|read| read.lines.iter().any(|line| !line.pace.is_empty()));
    // The profile's own 6-semitone bar: a wide pitch range is a line
    // *performed*, a narrow one a line *reported*. It is measured per
    // line, so a run where no line was measured must not claim it.
    let measured_performance = reads
        .iter()
        .any(|read| read.lines.iter().any(|line| line.pitch_range.is_some()));
    let measured_loudness = reads
        .iter()
        .any(|read| read.lines.iter().any(|line| line.relative_lu.is_some()));
    let measured_entities = reads.iter().any(|read| !read.entities.is_empty());
    let measured_sentence = reads
        .iter()
        .any(|read| read.lines.iter().any(|line| !line.sentence_type.is_empty()));
    let measured_speakers = reads
        .iter()
        .any(|read| !read.take_wide.speaker_changes.is_empty());
    let measured_motifs = reads.iter().any(|read| !read.motifs.is_empty());
    // The font hint is only a measurement when the read named a family
    // other than the profile's own default: `Inter` is what the producer
    // reports when it detected nothing, so it drives no decision.
    let measured_font = reads
        .iter()
        .any(|read| !read.take_wide.font.is_empty() && read.take_wide.font != "Inter");
    let measured_tempo = reads
        .iter()
        .any(|read| read.take_wide.dominant_period_sec.is_some());
    let measured_speech_ratio = reads
        .iter()
        .any(|read| read.take_wide.speech_ratio.is_some());
    // The reading rate, the delivery's shape and the forensic character
    // are each measured by a different ear, and each is only cited by a
    // decision that actually turned on it.
    let measured_rate = reads.iter().any(|read| {
        read.lines
            .iter()
            .any(|line| line.words_per_minute.is_some() || line.syllables_per_sec.is_some())
    });
    let measured_pitch = reads
        .iter()
        .any(|read| read.lines.iter().any(|line| line.pitch_slope.is_some()));
    let measured_onsets = reads
        .iter()
        .any(|read| !read.take_wide.flux_onsets.is_empty());
    let measured_forensic = reads
        .iter()
        .any(|read| read.take_wide.spectral_flatness.is_some());
    let measured_turns = reads
        .iter()
        .any(|read| !read.take_wide.speaker_turns.is_empty());
    let mut signal_log: Vec<Value> = Vec::new();
    // The retention pass is a decision, not a report: the plan's pattern
    // breaks moved to the stretches the analysis flagged, and the critic
    // is told which signals found them.
    if measured_words || !read.is_empty() {
        let mut signals: Vec<&str> = Vec::new();
        if measured_words {
            signals.push("words");
        }
        if !read.is_empty() {
            signals.push("emotion");
        }
        if measured_tempo {
            signals.push("rhythmLayers");
        }
        signal_log.push(json!({
            "at": retention_findings["boringStretches"]
                .as_array()
                .and_then(|risks| risks.first())
                .and_then(|risk| risk["start"].as_f64())
                .unwrap_or(0.0),
            "decision": "pacing",
            "signals": signals,
            "note": format!(
                "the retention pass found {}; the plan's pattern breaks moved to those stretches",
                retention_findings["summary"].as_str().unwrap_or("no retention risk")
            ),
        }));
    }
    // The variety pass: the plan rotates shot scale, entrance and B-roll
    // category, and the transition stage's no-repeat rule owns the moves.
    if measured_words || !read.is_empty() {
        let mut signals: Vec<&str> = Vec::new();
        if measured_words {
            signals.push("words");
        }
        if !read.is_empty() {
            signals.push("emotion");
        }
        signal_log.push(json!({
            "at": 0.0,
            "decision": "motion",
            "signals": signals,
            "note": format!(
                "the {} direction rotates {} shot scales, {} entrances and {} B-roll categories; no two neighbours repeat",
                direction.label(),
                variety["shotScales"].as_array().map(Vec::len).unwrap_or(0),
                variety["entrances"].as_array().map(Vec::len).unwrap_or(0),
                variety["categories"].as_array().map(Vec::len).unwrap_or(0),
            ),
        }));
    }
    // A cut names the signal that put it there. Every boundary is a gap
    // between two clips of the voiceover, so the word boundary always
    // drove it; the pause reading adds the class when the profile
    // measured the silence and said what kind it was.
    for segment in segments.iter().skip(1) {
        let (signals, note) = if measured_pauses {
            (
                json!(["pause.class=dead_air", "words"]),
                "the boundary is a measured silence between two spoken lines",
            )
        } else {
            (
                json!(["words"]),
                "the boundary is the gap between two timed lines of speech",
            )
        };
        signal_log.push(json!({
            "at": segment["start"],
            "decision": "cut",
            "signals": signals,
            "note": note,
        }));
    }
    if measured_words {
        let mut signals = vec!["words"];
        if reads.iter().any(|read| read.weight("wordConfidence") > 0.0) {
            signals.push("wordConfidence");
        }
        if measured_emphasis {
            signals.push("emphasisWords");
        }
        if measured_font {
            signals.push("language");
        }
        if measured_sentence {
            signals.push("sentence.type");
        }
        if measured_performance {
            signals.push("prosody.range");
        }
        signal_log.push(json!({
            "at": 0.0,
            "decision": "caption",
            "signals": signals,
            "note": if measured_font {
                "captions sit on the measured words, in the family the script's own read names"
            } else if measured_sentence {
                "captions sit on the measured words; a question line holds past its answer"
            } else if measured_emphasis {
                "captions sit on the measured words; a stressed word takes the accent"
            } else {
                "captions sit on the measured words"
            },
        }));
    }
    // The graphic the plan can account for: an entity the narration names,
    // at the moment it says it. The motion stage spends this same reading,
    // so the log and the bake cannot disagree about why a card exists.
    if measured_entities {
        for entity in entity_graphics(log, takes).iter().take(3) {
            let mut signals = vec!["entities"];
            if measured_emphasis {
                signals.push("emphasisWords");
            }
            signal_log.push(json!({
                "at": round3(entity.at.unwrap_or_default()),
                "decision": "graphic",
                "signals": signals,
                "note": format!(
                    "the narration names {} ({}), so its {} card lands where it says it",
                    entity.text, entity.kind, entity.card
                ),
            }));
        }
    }
    // A question the narration asks is a card the act earns, not the
    // mood: the viewer is addressed, and the card holds the ask while
    // the answer plays. This is the entry `speechActs` exists to justify.
    for question in question_acts(log, takes).iter().take(3) {
        signal_log.push(json!({
            "at": round3(question.start),
            "decision": "graphic",
            "signals": ["speechActs", "words"],
            "note": format!(
                "the narration asks a question here, so the card holds the ask: {}",
                question.text
            ),
        }));
    }
    // A claim with no proof is the argument's weak point, and the edit's
    // answer is to show it as the argument: a pull quote, so the line is
    // read rather than buried. This is the `claims` entry.
    for claim in uncovered_claims(log, takes).iter().take(3) {
        signal_log.push(json!({
            "at": round3(claim.start),
            "decision": "graphic",
            "signals": ["claims", "words"],
            "note": format!(
                "the narration asserts this with no proof behind it, so the card sets the claim: {}",
                claim.text
            ),
        }));
    }
    if let Some(peak) = read
        .iter()
        .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
    {
        // The card is the read's own decision, so `emotion` always drove
        // it; the words only appear when the transcript measured them,
        // because a card that cannot say what it is set in has not spent
        // the word reading.
        signal_log.push(json!({
            "at": round3(peak.start),
            "decision": "graphic",
            "signals": if measured_words {
                json!(["emotion", "words"])
            } else {
                json!(["emotion"])
            },
            "note": if measured_words {
                "the piece's strongest read earns its own card, in the words it was read on"
            } else {
                "the title entrance lands on the piece's strongest line"
            },
        }));
    }
    // The motion-design read: one entry naming the twenty readings that
    // actually carried values, so the keyframes, entrances, holds and
    // texture are accounted for in the same log as the cuts and cards.
    if let Some(motion) = motion_design(log) {
        let mut signals: Vec<String> = vec!["motion".to_owned()];
        if let Some(fields) = motion.as_object() {
            for (name, value) in fields {
                let measured = match value {
                    Value::Array(rows) => !rows.is_empty(),
                    Value::Object(map) => !map.is_empty(),
                    Value::Null => false,
                    _ => true,
                };
                if measured {
                    signals.push(format!("motion.{name}"));
                }
            }
        }
        let at = motion
            .pointer("/emphasisHits/0/at")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        signal_log.push(json!({
            "at": round3(at),
            "decision": "motion",
            "signals": signals,
            "note": "the motion-design read sizes the keyframes, entries, holds and texture from the measured delivery",
        }));
    }
    let bed_words = dominant(&read)
        .and_then(|line| line.bgm_words.first().cloned())
        .unwrap_or_else(|| "cinematic".to_owned());
    let duck = dominant(&read).map(|line| line.duck_db).unwrap_or(-18.0);
    // The profile's own words for the verdict, when it reached one: its
    // reason names the measurement (floor, SNR, flatness) that decided,
    // which is more than this program could restate.
    let bed_note = reads
        .iter()
        .map(|read| read.take_wide.bgm_reason.trim())
        .find(|reason| !reason.is_empty())
        .map(|reason| format!("the bed follows the profile's own verdict: {reason}"))
        .unwrap_or_else(|| {
            "the bed is gated on the measured noise floor and music check; \
             its words and duck come from the read's dominant line"
                .to_owned()
        });
    // A low SNR is not a reason to skip the bed, it is a reason to make
    // the bed quieter under a voice the noise is already fighting: the
    // duck deepens by the shortfall against the profile's own 12 dB floor,
    // to a maximum of 6 dB, so a hissy take never has a bed sitting on top
    // of its hiss. A take whose SNR was never measured keeps the read's
    // duck untouched rather than guessing one.
    let duck = reads
        .iter()
        .filter_map(|read| read.take_wide.snr_db)
        .fold(duck, |duck, snr| {
            if snr >= 12.0 {
                duck
            } else {
                (duck - (12.0 - snr).min(6.0)).max(-30.0)
            }
        });
    // The bed's evidence is the gate's own: the noise floor that says the
    // take is dry, the speech ratio that says how much room a bed has, and
    // the music check that says the take carries none. Emotion and the arc
    // choose its words and its duck, which is the second half of the entry.
    {
        let mut signals: Vec<&str> = Vec::new();
        // The floor earns its place only where the profile's own rule -
        // a dry take wants a bed - is what the bed was chosen on.
        if reads.iter().any(|read| read.wants_bed() == Some(true)) {
            signals.push("noiseFloor");
        }
        if measured_speech_ratio {
            signals.push("speechRatio");
        }
        // The voicing rate is the forensic second opinion on density, and
        // a dialogue's speaker shares deepen the duck further.
        if reads
            .iter()
            .any(|read| read.take_wide.voicing_rate.is_some())
        {
            signals.push("forensic.voicingRate");
        }
        if reads.iter().any(|read| read.conversation() == Some(true)) {
            signals.push("speaker");
        }
        // The music check earns its place when the gate measured it, and
        // when the profile's own verdict turned on it: a `skip` is the
        // flatness-plus-onsets rule, which is the music-segment reading,
        // and a `duck_only` is the SNR rule the forensic read carries.
        let verdict = reads
            .iter()
            .filter_map(|read| read.take_wide.bgm_decision.as_deref())
            .find(|decision| *decision != "add");
        // The musicPresent key the gate wants sits on the music gate's
        // OWN reply (detect_music_segments), never on the voice
        // profile's - the raw log read is what fires.
        if log.iter().any(|reply| reply.get("musicPresent").is_some()) || verdict == Some("skip") {
            signals.push("musicSegments");
        }
        // The noise character earns its place only where the profile named
        // one: it is what a denoise would target, and the bed is what a
        // take with a floor this loud gets instead of one.
        let noise = reads.iter().find_map(|read| read.noise_character());
        if noise.is_some() && measured_forensic {
            signals.push("forensic");
        }
        if verdict == Some("duck_only") {
            signals.push("noiseFloor");
        }
        // The SNR is its own reading and earns its own place: when the
        // forensic ear measured it, the duck above was set on it.
        if reads.iter().any(|read| read.take_wide.snr_db.is_some()) {
            signals.push("noiseFloor.snr_db");
        }
        if !read.is_empty() {
            signals.push("emotion");
            signals.push("arc");
        }
        if !signals.is_empty() {
            // The floor's character, in the entry itself: a note that only
            // says "the bed follows the verdict" leaves the reader unable
            // to tell a hiss from a hum, which is the one thing a denoise
            // needs to know.
            let note = match &noise {
                Some(noise) => format!("{bed_note}; the floor measured {noise}"),
                None => bed_note.clone(),
            };
            signal_log.push(json!({
                "at": 0.0,
                "decision": "bgm",
                "signals": signals,
                "note": note,
            }));
        }
    }
    // The mix: the measured integrated loudness against the platform
    // target. Only the profile's own `integratedLufs` can make this entry,
    // so a run that never read the take claims no level.
    if let (Some(target), Some(measured)) = (
        platform_target(log),
        reads
            .iter()
            .filter_map(|read| read.take_wide.integrated_lufs)
            .fold(None::<f64>, |loudest, lufs| {
                Some(loudest.map_or(lufs, |loudest: f64| loudest.max(lufs)))
            }),
    ) && (target - measured).abs() >= 1.0
    {
        let mut signals: Vec<&str> = vec!["loudness.integratedLufs"];
        if reads
            .iter()
            .any(|read| read.take_wide.true_peak_dbfs.is_some())
        {
            signals.push("loudness.truePeakDbfs");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.loudness_range_lu.is_some())
        {
            signals.push("loudness.loudnessRangeLu");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.clipping_ratio.is_some())
        {
            signals.push("forensic.clippingRatio");
        }
        if reads.iter().any(|read| read.take_wide.crest.is_some()) {
            signals.push("forensic.crest");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.dynamic_range_db.is_some())
        {
            signals.push("forensic.dynamicRangeDb");
        }
        if measured_speech_ratio {
            signals.push("speechRatio");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.voicing_rate.is_some())
        {
            signals.push("forensic.voicingRate");
        }
        signal_log.push(json!({
            "at": 0.0,
            "decision": "mix",
            "signals": signals,
            "note": format!(
                "the take measured {measured:.1} LUFS and the platform targets {target:.0}, so the voice takes {:+.1} dB; the peak, clipping, crest and range readings cap that gain",
                target - measured
            ),
        }));
    }
    // The repair: the forensic ear names the target and the line reads
    // name the unstable voice. This is its own decision, not a BGM note:
    // a hum wants a notch, a DC offset a blocker, a muffled take a
    // presence lift, and a hoarse line no accent at all.
    let repair_noise = reads.iter().find_map(|read| read.noise_character());
    let hoarse = reads.iter().any(|read| {
        read.lines
            .iter()
            .any(|line| read.clean_voice(line) == Some(false))
    });
    if repair_noise.is_some() || hoarse {
        let mut signals: Vec<&str> = Vec::new();
        if repair_noise.is_some() {
            signals.push("forensic.noiseType");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.spectral_flatness.is_some())
        {
            signals.push("forensic.spectralFlatness");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.spectral_centroid_hz.is_some())
        {
            signals.push("forensic.spectralCentroidHz");
        }
        if reads.iter().any(|read| read.take_wide.hum_hz.is_some()) {
            signals.push("forensic.humHz");
        }
        if reads.iter().any(|read| read.take_wide.dc_offset.is_some()) {
            signals.push("forensic.dcOffset");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.spectral_rolloff_hz.is_some())
        {
            signals.push("forensic.spectralRolloffHz");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.spectral_bandwidth_hz.is_some())
        {
            signals.push("forensic.spectralBandwidthHz");
        }
        if hoarse {
            signals.push("forensic.hnrDb");
            signals.push("forensic.jitter");
            signals.push("forensic.shimmer");
        }
        let note = match &repair_noise {
            Some(noise) => format!("the forensic ear names the repair: {noise}"),
            None => "the line's voice quality is unstable, so the repair runs before any accent"
                .to_owned(),
        };
        signal_log.push(json!({
            "at": 0.0,
            "decision": "repair",
            "signals": signals,
            "note": note,
        }));
    }
    // The grade: emotion names the temperature, the forensic tone breaks
    // a neutral tie, and the arc decides whether the look lifts at the
    // end. This is the decision the colour stage spends, so it is logged
    // as `grade` rather than as a vague pacing note.
    let grade = grade_decision(log, takes);
    if !grade.signals.is_empty() {
        signal_log.push(json!({
            "at": 0.0,
            "decision": "grade",
            "signals": grade.signals,
            "note": grade.note,
        }));
    }
    // The accents: the prosody ear's emphasis peaks are the hits, and the
    // forensic room/SNR readings decide which peaks can hold one. This is
    // the SFX decision, separate from the bed it sits over.
    let accent_peaks = emphasis_peaks(&reads);
    if !accent_peaks.is_empty() {
        let mut signals: Vec<&str> = vec!["emphasisWords", "prosody"];
        if !read.is_empty() {
            signals.push("emotion");
        }
        if reads.iter().any(|read| read.take_wide.snr_db.is_some()) {
            signals.push("forensic.snrDb");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.reverb_tail_sec.is_some())
        {
            signals.push("forensic.reverbTailSec");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.sibilance_ratio.is_some())
        {
            signals.push("forensic.sibilanceRatio");
        }
        if reads
            .iter()
            .any(|read| !read.take_wide.flux_onsets.is_empty())
        {
            signals.push("forensic.fluxOnsetsSec");
        }
        if reads
            .iter()
            .any(|read| read.take_wide.dominant_period_sec.is_some())
        {
            signals.push("rhythmLayers");
        }
        signal_log.push(json!({
            "at": 0.0,
            "decision": "sfx",
            "signals": signals,
            "note": format!(
                "the prosody ear measured {} emphasis peaks; the room and SNR readings decide which can hold an accent",
                accent_peaks.len()
            ),
        }));
    }
    for slot in broll_slots(log, takes) {
        // The transition into the shot: the mood the gap opens on, the
        // pause that opened the gap, and - where the read measured them -
        // the speaker change, topic boundary or connective pivot the seam
        // crosses. A transition is the one decision that is about the
        // seam, so its evidence is the seam's.
        let mut signals: Vec<&str> = vec!["emotion"];
        if measured_pauses {
            signals.push("pause");
        }
        if measured_speakers {
            signals.push("speakerChanges");
        }
        if topic_boundaries(log, takes)
            .iter()
            .any(|boundary| boundary.take == slot.take && (boundary.at - slot.from).abs() < 3.0)
        {
            signals.push("topics");
        }
        if discourse_pivots(log, takes)
            .iter()
            .any(|(take, at)| take == &slot.take && (*at - slot.from).abs() < 3.0)
        {
            signals.push("discourseMarkers");
        }
        if measured_tempo {
            signals.push("rhythmLayers");
        }
        // The seam's own transient: a flux onset inside the window is a
        // place the cut can land where the take itself has an event,
        // rather than on an arbitrary frame. The forensic ear is the only
        // one that measured it, so the entry cites it only when one was
        // found near the seam.
        let onset = reads
            .iter()
            .find_map(|read| read.onset_near(slot.from, 0.25));
        if onset.is_some() && measured_onsets {
            signals.push("forensic");
        }
        // The turn the seam falls inside, when diarization measured turns:
        // a seam inside a turn is one speaker continuing, and a seam at a
        // turn's edge is the exchange the cut should show.
        let turn_edge = reads.iter().any(|read| {
            read.take_wide.speaker_turns.iter().any(|turn| {
                (turn.start - slot.from).abs() < 0.25 || (turn.end - slot.from).abs() < 0.25
            })
        });
        if turn_edge && measured_turns {
            signals.push("speaker");
        }
        let seam = match (onset, turn_edge) {
            (Some(onset), true) => format!(
                "the cut into the shot takes the mood the gap opens on, lands on the \
                 transient at {onset:.2}s and falls on a speaker turn"
            ),
            (Some(onset), false) => format!(
                "the cut into the shot takes the mood the gap opens on and lands on \
                 the transient at {onset:.2}s"
            ),
            (None, true) => "the cut into the shot takes the mood the gap opens on and \
                             falls on a speaker turn"
                .to_owned(),
            (None, false) => "the cut into the shot takes the mood the gap opens on".to_owned(),
        };
        signal_log.push(json!({
            "at": round3(slot.from),
            "decision": "transition",
            "signals": signals,
            "note": seam,
        }));
    }
    // A question the narration asks wants its answer as the next visual:
    // the one decision that only exists because the sentence's type was
    // read against the pitch's terminal rise. A line whose punctuation
    // says question and whose pitch rose to confirm it is an ask; one the
    // pitch contradicted is a rhetorical line, and the log says which.
    for read in &reads {
        for question in &read.take_wide.questions {
            let Some(line) = read.lines.get(*question) else {
                continue;
            };
            let mut signals = vec!["sentence"];
            let note = match line.rise_confirms {
                Some(true) => {
                    signals.push("prosody");
                    format!(
                        "the line asks a question and its pitch rose to confirm it, so \
                         the answer arrives as the next visual: {}",
                        line.text
                    )
                }
                Some(false) => format!(
                    "the line is punctuated as a question but its pitch fell, so it \
                     reads as rhetorical and the next visual is not an answer: {}",
                    line.text
                ),
                None => format!(
                    "the line asks a question, so the answer arrives as the next visual: {}",
                    line.text
                ),
            };
            signal_log.push(json!({
                "at": round3(line.start),
                "decision": "graphic",
                "signals": signals,
                "note": note,
            }));
        }
    }
    // A chapter card at every topic boundary the tiling found: the one
    // decision that only exists because the script was read as language
    // rather than as sound. Its words are the segment's own headline.
    for boundary in topic_boundaries(log, takes).iter().take(6) {
        signal_log.push(json!({
            "at": round3(boundary.at),
            "decision": "graphic",
            "signals": ["topics", "words"],
            "note": format!(
                "the narration changes chapter here, so a card names it: {}",
                boundary.headline
            ),
        }));
    }
    // The delivery the read measured, named only where it measured one:
    // a pace entry is evidence the run read, and a run with no word
    // timings says so by leaving the entry out rather than claiming a
    // rhythm it never heard. Loudness joins it only when the reading
    // measured a level for the line.
    let paced: Vec<&EmotionLine> = read.iter().filter(|line| !line.pace.is_empty()).collect();
    if measured_pace && !paced.is_empty() {
        let mut seen: Vec<&str> = Vec::new();
        for line in &paced {
            if seen.contains(&line.pace.as_str()) {
                continue;
            }
            seen.push(&line.pace);
            let mut signals = vec!["speechRate", "words"];
            if measured_loudness {
                signals.push("loudness");
            }
            // The line's own reading rate, when the profile measured one:
            // it sizes the caption's hold, which words-per-minute alone
            // gets wrong on a line of long words.
            let hold = reads
                .iter()
                .find_map(|read| read.line_at(line.start).and_then(|l| read.caption_hold(l)));
            if hold.is_some() && measured_rate {
                signals.push("speechRate");
            }
            // The delivery's shape: a line whose pitch rose across it
            // builds into what follows, so the cut into the next shot is
            // the one that carries the build rather than a hold that
            // settles the take early.
            let rises = reads.iter().find_map(|read| {
                read.line_at(line.start)
                    .and_then(|l| read.delivery_rises(l))
            });
            if rises.is_some() && measured_pitch {
                signals.push("prosody");
            }
            let shape = match (hold, rises) {
                (Some(hold), Some(true)) => format!(
                    "the line needs {hold:.1}s to read and its pitch rose, so the shot \
                     holds {hold:.1}s and hands the build to the cut after it"
                ),
                (Some(hold), Some(false)) => format!(
                    "the line needs {hold:.1}s to read and its pitch fell, so the shot \
                     holds {hold:.1}s and settles before the cut"
                ),
                (Some(hold), None) => {
                    format!("the line needs {hold:.1}s to read, so the shot holds that long")
                }
                (None, Some(true)) => {
                    "the line's pitch rose, so the cut after it carries the build".to_owned()
                }
                (None, Some(false)) => {
                    "the line's pitch fell, so the cut after it settles the take".to_owned()
                }
                (None, None) => String::new(),
            };
            let note = if shape.is_empty() {
                format!(
                    "the delivery here measured {}, so the shot's hold and the \
                     caption's size follow it",
                    line.pace
                )
            } else {
                format!("the delivery here measured {}: {shape}", line.pace)
            };
            signal_log.push(json!({
                "at": round3(line.start),
                "decision": "pacing",
                "signals": signals,
                "note": note,
            }));
        }
    }
    // The profile's own cut style, when it derived one: the pace and the
    // levels across the whole take, in one word. This is the signal that
    // decides how long a shot is held, and the gather stage spends it on
    // the drift, so the plan records it rather than leaving the hold
    // unexplained.
    if let Some(style) = reads
        .iter()
        .map(|read| read.take_wide.cut_style.as_str())
        .find(|style| !style.is_empty())
    {
        let mut signals = vec!["speechRate", "loudness"];
        if measured_pauses {
            signals.push("pause");
        }
        signal_log.push(json!({
            "at": 0.0,
            "decision": "pacing",
            "signals": signals,
            "note": format!(
                "the whole take measured a {style} cut style, so every shot's hold follows it"
            ),
        }));
    }
    // A motif is a phrase the narration repeats, and the edit owes it the
    // same visual when it returns - the one decision that only exists
    // because the n-gram reader ran.
    if measured_motifs
        && let Some(motif) = reads
            .iter()
            .flat_map(|read| read.motifs.iter())
            .max_by_key(|motif| motif.count)
    {
        signal_log.push(json!({
            "at": 0.0,
            "decision": "graphic",
            "signals": ["motifs"],
            "note": format!(
                "\"{}\" recurs {} times, so its return takes the same visual",
                motif.phrase, motif.count
            ),
        }));
    }
    // Every entry carries the confidence the profile rated its signals at.
    // The profile answers `signalConfidence.perSignal`, and until this the
    // map was parsed into `VoiceRead::weight` and read by nothing - so a
    // decision taken on a whisper of a measurement looked exactly like one
    // taken on a certain one. The entry's confidence is the weakest family
    // it names, because a chain of evidence is only as strong as its
    // weakest link, and 0 for a family the profile never rated.
    for entry in &mut signal_log {
        let families: Vec<&str> = entry["signals"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|signal| signal.split(['.', '[']).next().unwrap_or(signal))
            .collect();
        let confidence = families
            .iter()
            .map(|family| match *family {
                // The emotional read is not in `voice_signals`'
                // vocabulary - it is the emotion engine's own answer, so
                // it is rated by that engine's confidence, read from the
                // line the entry's moment landed on.
                "emotion" | "arc" => read
                    .iter()
                    .filter(|line| line.confidence.is_some())
                    .map(|line| line.confidence.unwrap_or_default())
                    .fold(0.0f64, f64::max),
                // The discourse families are the discourse engine's own
                // readings, each carrying its own certainty: a speech act
                // at 0.75, a chapter tiling at its cohesion. Rated by
                // `voice_signals`' vocabulary they would read 0.
                "speechActs" | "claims" => question_acts(log, takes)
                    .iter()
                    .chain(uncovered_claims(log, takes).iter())
                    .filter(|line| line.confidence.is_some())
                    .map(|line| line.confidence.unwrap_or_default())
                    .fold(0.0f64, f64::max),
                "topics" => topic_boundaries(log, takes)
                    .iter()
                    .filter(|boundary| boundary.confidence.is_some())
                    .map(|boundary| boundary.confidence.unwrap_or_default())
                    .fold(0.0f64, f64::max),
                // A pivot is a marker the lexicon matched; the profile
                // rates the marker family at 0.65, like the fillers.
                "discourseMarkers" => 0.65,
                // `words` is the transcript's own text; the profile rates
                // the gate that produced them as `wordConfidence`.
                "words" => reads
                    .iter()
                    .map(|read| read.weight("wordConfidence"))
                    .fold(0.0f64, f64::max),
                // Any other family is rated under the profile's own name
                // for it, which is not always the log's.
                family => reads
                    .iter()
                    .map(|read| read.weight(confidence_family(family)))
                    .fold(0.0f64, f64::max),
            })
            .fold(None::<f64>, |weakest, weight| {
                Some(weakest.map_or(weight, |weakest: f64| weakest.min(weight)))
            })
            .unwrap_or_default();
        entry["confidence"] = json!(round3(confidence));
    }
    // The emotional read owns the temperature; when it is neutral (or
    // never measured) the direction's own grade gives the film its topic-
    // specific look instead of a generic teal-and-orange.
    let look = dominant(&read)
        .filter(|line| {
            !matches!(
                line.temperature.trim(),
                "" | "neutral" | "balanced" | "unknown"
            )
        })
        .map(|line| look_for_temperature(&line.temperature))
        .unwrap_or_else(|| direction.grade());
    let (intensity_key, intensity) = grade_param(look);
    let orientation = match aspect {
        "9:16" | "4:5" => "vertical",
        "1:1" => "square",
        _ => "horizontal",
    };
    let palette = direction.palette(look);
    let caption_font = reads
        .iter()
        .map(|read| read.take_wide.font.as_str())
        .find(|font| !font.trim().is_empty() && *font != "Inter")
        .map(bundled_font)
        .unwrap_or_else(|| direction.caption_font());
    // The one look the whole fixed edit wears. The Director writes a
    // richer style bible; this is the floor the deterministic fallback
    // can prove, so the same validator can judge both paths.
    let style = json!({
        "name": format!(
            "{} / {} / {look} / {orientation}",
            direction.label(),
            profile_plan["profile"].as_str().unwrap_or("general")
        ),
        "palette": palette,
        "grade": look,
        "captions": {
            "font": caption_font,
            "weight": profile_plan["captions"]["weight"]
                .as_i64()
                .unwrap_or_else(|| direction.caption_weight()),
            "emphasisColor": palette[1],
            "position": if orientation == "vertical" { "center" } else { "lower-third" },
        },
        "motion": {
            "language": direction.motion_language(),
            "easing": direction.motion_easing(),
            "intensity": profile_plan["motion"]["intensity"].clone(),
        },
        "sound": {
            "duckDb": duck,
            "texture": direction.sound_texture(),
        },
        "footage": {
            "orientation": orientation,
            "treatment": "fill",
            "continuity": direction.footage_continuity(),
        },
        "continuityRules": direction.continuity_rules(),
    });
    // The retention contract: the hook is the first spoken line, the turn
    // is the strongest measured act (or the middle of the cut), and the
    // payoff is the last line. Pattern breaks are spread across the cut,
    // the turn and the retention pass's own boring stretches and repeated
    // explanations, so a clean fallback still has a rhythm a viewer can
    // feel - and a longform cut gets enough of them to pass its own gate.
    let wanted_breaks = ((runtime
        / profile_plan["pacing"]["patternBreakEvery"]
            .as_f64()
            .unwrap_or(20.0))
    .ceil() as usize)
        .max(2);
    let mut pattern_breaks: Vec<f64> = (1..=wanted_breaks)
        .map(|index| round3(runtime * index as f64 / (wanted_breaks + 1) as f64))
        .collect();
    pattern_breaks.push(round3(turn_at));
    for risk in retention_findings["boringStretches"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|risk| risk["start"].as_f64())
    {
        pattern_breaks.push(round3(risk));
    }
    for risk in retention_findings["repeatedExplanations"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|risk| risk["at"].as_f64())
    {
        pattern_breaks.push(round3(risk));
    }
    for risk in retention_findings["weakTransitions"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|risk| risk["at"].as_f64())
    {
        pattern_breaks.push(round3(risk));
    }
    pattern_breaks.retain(|at| *at > 0.0 && *at < runtime);
    pattern_breaks.sort_by(|left, right| left.total_cmp(right));
    pattern_breaks.dedup_by(|left, right| (*left - *right).abs() < 0.25);
    let hook = spoken
        .first()
        .cloned()
        .unwrap_or_else(|| "the opening claim and the question it plants".to_owned());
    let retention = json!({
        "hook": hook,
        "turnAt": round3(turn_at.clamp(runtime * 0.2, runtime * 0.8)),
        "payoffAt": round3(payoff_at.clamp(runtime * 0.75, runtime)),
        "patternBreaks": pattern_breaks,
        "openLoops": [
            brief.get("curiosityGap").and_then(Value::as_str).unwrap_or("what is the claim?"),
            brief.get("payoff").and_then(Value::as_str).unwrap_or("what proves it?"),
        ],
        "analysis": retention_findings,
        "creativeAlternatives": alternatives,
    });
    Some(json!({
        "title": source
            .project
            .rsplit('/')
            .next()
            .unwrap_or("Auto edit"),
        "mode": mode,
        "aspect": aspect,
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
        "style": style,
        "retention": retention,
        "visualVariety": variety,
        "youtube": youtube,
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
                // The timeline view names the lane "track"; "trackId" is
                // the request side's spelling, never the reply's.
                clip["track"]
                    .as_str()
                    .or_else(|| clip["trackId"].as_str())
                    .unwrap_or_default()
                    .to_owned(),
            ))
        })
        .collect()
}

/// The profile's own family names, mapped onto the vocabulary
/// `plan_validate` checks a decision against. The producer names a family
/// for what it measured (`pauses`, `sentenceType`); the vocabulary names
/// it for the decision it serves (`pause`, `sentence`). A brief that
/// handed a critic the producer's name would give it a signal it cannot
/// cite, so every name is translated here and a family with no vocabulary
/// name is left out rather than offered. `wordConfidence` is both: the
/// profile rates it and the vocabulary now cites it under its own name.
fn vocabulary_name(family: &str) -> Option<&'static str> {
    Some(match family {
        "pauses" => "pause",
        "sentenceType" => "sentence",
        "wordConfidence" => "words",
        "formants" => "formants",
        "prosody" => "prosody",
        "forensic" => "forensic",
        "speaker" => "speaker",
        "entities" => "entities",
        "language" => "language",
        "motifs" => "motifs",
        "fillers" => "fillers",
        "speechRate" => "speechRate",
        "loudness" => "loudness",
        "rhythmLayers" => "rhythmLayers",
        "motion" => "motion",
        _ => return None,
    })
}

/// The newest voice profile's motion-design block, when the take was read
/// with the motion family. It is the only place the twenty motion readings
/// live, and the motion stage and the signal log both spend it from here.
fn motion_design(log: &[Value]) -> Option<&Value> {
    log.iter()
        .rev()
        .find_map(|reply| reply.get("motion").filter(|motion| motion.is_object()))
}

/// The motion-design tempo the take measured at a file second, 0.6-1.6.
/// `None` when that take's profile carried no motion reading there, so a
/// card keeps the editorial default instead of a guessed speed.
fn motion_speed_at(log: &[Value], take: &str, at: f64) -> Option<f64> {
    let motion = log.iter().rev().find_map(|reply| {
        (reply.pointer("/audio/path").and_then(Value::as_str) == Some(take))
            .then(|| reply.get("motion"))
            .flatten()
    })?;
    motion.get("rateTempo")?.as_array()?.iter().find_map(|row| {
        let start = row["start"].as_f64()?;
        let end = row["end"].as_f64()?;
        (start <= at + 0.05 && end > at - 0.05)
            .then(|| row["speed"].as_f64())
            .flatten()
    })
}

/// Every signal family the profile answered, with the confidence the
/// profile gave it and the one measured fact behind it. This is the
/// brief's whole reading rather than its highlights: a critic that is
/// told the arc but not that `formants` was rated 0.2 on a phone-band
/// take will ask for a pitch-driven cut the audio cannot support. The
/// digest is built from the profile's own `signalConfidence` rows, so a
/// family the producer did not rate never appears as a measured one.
fn signal_evidence(log: &[Value]) -> Value {
    // The newest profile is the one the run read last; a second take's
    // profile is merged family by family, keeping the higher confidence,
    // because a family measured well on any take is measured.
    let mut families: BTreeMap<String, (f64, String)> = BTreeMap::new();
    // The profile's own account of what it could not measure. It is the
    // only honest source for that: a family missing from `measured` may
    // be missing because the take had none, and the critic cannot tell
    // the two apart without this. Deduplicated and in profile order.
    let mut degraded: Vec<String> = Vec::new();
    for reply in log {
        if let Some(read) = VoiceRead::of(reply) {
            for row in &read.degradations {
                let row = row.trim();
                if !row.is_empty() && !degraded.iter().any(|kept| kept == row) {
                    degraded.push(row.to_owned());
                }
            }
        }
        let Some(rows) = reply
            .pointer("/signalConfidence/perSignal")
            .and_then(Value::as_array)
        else {
            continue;
        };
        for row in rows {
            let (Some(name), Some(confidence)) =
                (row["signal"].as_str(), row["confidence"].as_f64())
            else {
                continue;
            };
            // A 0.0 row is the profile saying it did not measure this at
            // all; carrying it would let a critic spend a signal the run
            // never read, so it is left out of the digest entirely.
            if confidence <= 0.0 {
                continue;
            }
            let Some(name) = vocabulary_name(name) else {
                continue;
            };
            let basis = row["basis"].as_str().unwrap_or_default().to_owned();
            families
                .entry(name.to_owned())
                .and_modify(|kept| {
                    if confidence > kept.0 {
                        *kept = (confidence, basis.clone());
                    }
                })
                .or_insert((confidence, basis));
        }
    }
    if families.is_empty() {
        return json!({});
    }
    let strong: Vec<String> = families
        .iter()
        .filter(|(_, (confidence, _))| *confidence >= 0.7)
        .map(|(name, (confidence, _))| format!("{name} {confidence:.2}"))
        .collect();
    let weak: Vec<String> = families
        .iter()
        .filter(|(_, (confidence, _))| *confidence < 0.7)
        .map(|(name, (confidence, _))| format!("{name} {confidence:.2}"))
        .collect();
    let mut digest = serde_json::Map::new();
    digest.insert(
        "measured".to_owned(),
        json!(families.keys().cloned().collect::<Vec<_>>()),
    );
    if !degraded.is_empty() {
        digest.insert("notMeasured".to_owned(), json!(degraded));
    }
    if !strong.is_empty() {
        digest.insert("leanOn".to_owned(), json!(strong));
    }
    if !weak.is_empty() {
        digest.insert("leanLessOn".to_owned(), json!(weak));
    }
    let basis: serde_json::Map<String, Value> = families
        .iter()
        .filter(|(_, (confidence, _))| *confidence < 0.7)
        .map(|(name, (_, basis))| (name.clone(), json!(basis)))
        .collect();
    if !basis.is_empty() {
        digest.insert("whyWeak".to_owned(), Value::Object(basis));
    }
    // What each measured family can decide, so the critic spends a signal
    // on the choice it actually informs instead of citing it decoratively.
    let decides: serde_json::Map<String, Value> = families
        .keys()
        .filter_map(|name| {
            let decisions = decision_for(name);
            (!decisions.is_empty()).then(|| (name.clone(), json!(decisions)))
        })
        .collect();
    if !decides.is_empty() {
        digest.insert("decides".to_owned(), Value::Object(decides));
    }
    Value::Object(digest)
}

/// Every decision a family informs, in the profile's own decision-policy
/// words. A family may drive more than one choice: `forensic` names the
/// bed, the repair, the mix, the grade's tone and the accent's placement,
/// and a single mapping would make the critic believe it only chose BGM.
/// A family with no entry drives no choice here, and the digest leaves it
/// out of `decides` rather than inventing one.
fn decision_for(family: &str) -> &'static [&'static str] {
    match family {
        "pause" => &["cut", "transition"],
        "emphasisWords" => &["cut", "graphic", "sfx"],
        "speechRate" => &["cut", "caption", "pacing"],
        "rhythmLayers" => &["cut", "sfx"],
        "motifs" => &["cut", "graphic"],
        // A filler is stripped from the caption it sits in, so the
        // decision it informs is the caption's.
        "words" | "wordConfidence" => &["caption", "graphic"],
        "language" => &["caption", "bgm"],
        "sentence" => &["caption", "transition"],
        "fillers" => &["caption"],
        "entities" | "speechActs" | "claims" => &["graphic"],
        "noiseFloor" => &["bgm", "repair", "mix"],
        "musicSegments" => &["bgm"],
        "speechRatio" => &["bgm", "mix"],
        "forensic" => &["bgm", "repair", "mix", "grade", "sfx"],
        "speakerChanges" | "discourseMarkers" => &["transition"],
        "speaker" => &["transition", "bgm"],
        "topics" => &["transition", "graphic"],
        "loudness" => &["caption", "mix", "pacing"],
        "prosody" => &["cut", "caption", "graphic", "sfx", "pacing"],
        "arc" => &["grade", "pacing", "graphic"],
        "emotion" => &["bgm", "grade", "graphic", "sfx", "transition", "pacing"],
        // The motion-design family is the one that turns the voice read
        // into keyframes, entries, holds and texture; every one of its
        // twenty readings informs that single decision.
        "motion" => &["motion"],
        // The picture's own families. Motion picks the in-point and vetoes
        // the redundant push; scale sets the drift and the cut rhythm; the
        // camera's travel is the continuity gate's direction; the verdict
        // is the stock veto; saliency moves the caption band; and a clip
        // that cuts inside itself is a montage the stock veto sinks.
        "visualMotion" => &["cut", "motion", "sfx"],
        "shotScale" => &["cut", "motion", "caption"],
        "camera" => &["cut", "transition", "motion"],
        "visualQuality" => &["cut", "motion"],
        "saliency" => &["cut", "caption", "motion"],
        "scene" => &["cut", "transition"],
        _ => &[],
    }
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
    // The whole reading, not only its highlights: every family the
    // profile answered, with its confidence, so the critic spends the
    // signals the run actually measured and knows which ones to lean on.
    let evidence = signal_evidence(log);
    if evidence.as_object().is_some_and(|map| !map.is_empty()) {
        brief.insert("signalEvidence".to_owned(), evidence);
    }
    // The measured values themselves, so the critic judges against numbers
    // rather than family names. A critic told "noiseFloor 0.80" can say
    // the bed was chosen well; a critic told "-38 dBFS, hiss" can say the
    // bed should have been quieter. Only the readings the profile actually
    // returned are carried, and a reading it never took stays absent.
    let measured = measured_values(log);
    if measured.as_object().is_some_and(|map| !map.is_empty()) {
        brief.insert("measured".to_owned(), measured);
    }
    // The picture's own read, as the critic's evidence: how many
    // candidates the frames measured, how many moved, how many passed the
    // technical read, how many were montages the stock veto sank, and the
    // scales the cut actually plays. The names the log can carry are the
    // families the visual vocabulary added, so a critic reading a
    // confidence can see which picture signal it stands on.
    let reads = visual_reads(log);
    if !reads.is_empty() {
        let mut scales: Vec<String> = Vec::new();
        for read in &reads {
            if let Some(scale) = read.shot_scale()
                && let Ok(word) = serde_json::to_value(scale)
                && let Some(word) = word.as_str()
                && !scales.iter().any(|known| known == word)
            {
                scales.push(word.to_owned());
            }
        }
        brief.insert(
            "picture".to_owned(),
            json!({
                "clips": reads.len(),
                "moving": reads.iter().filter(|read| read.moves()).count(),
                "passing": reads.iter().filter(|read| read.passes()).count(),
                "montages": reads.iter().filter(|read| read.montage()).count(),
                "scales": scales,
                "signals": [
                    "visualMotion",
                    "shotScale",
                    "camera",
                    "visualQuality",
                    "saliency",
                    "scene",
                ],
            }),
        );
    }
    Value::Object(brief)
}

/// The profile's own numbers, as the brief carries them: the take-wide
/// readings and the per-line shape, keyed the way the profile keys them.
/// Every value here is one the profile returned; a family it could not
/// measure is left out rather than filled with a default, so the critic
/// can tell "measured quiet" from "never measured".
fn measured_values(log: &[Value]) -> Value {
    let mut out = serde_json::Map::new();
    let mut take = serde_json::Map::new();
    for reply in log {
        let Some(whole) = reply.get("wholeTake") else {
            continue;
        };
        let forensic = &whole["forensic"];
        let mut insert = |key: &str, value: Option<Value>| {
            if let Some(value) = value
                && !take.contains_key(key)
            {
                take.insert(key.to_owned(), value);
            }
        };
        insert(
            "noiseFloorDbfs",
            forensic
                .get("noiseFloorDbfs")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "snrDb",
            forensic
                .get("snrDb")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "noiseType",
            forensic
                .get("noiseType")
                .and_then(Value::as_str)
                .filter(|kind| !kind.is_empty())
                .map(|kind| json!(kind)),
        );
        insert(
            "spectralFlatness",
            forensic
                .get("spectralFlatness")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "spectralCentroidHz",
            forensic
                .get("spectralCentroidHz")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "crest",
            VoiceRead::of(reply)
                .and_then(|read| read.take_wide.crest)
                .map(|v| json!(v)),
        );
        insert(
            "dynamicRangeDb",
            forensic
                .get("dynamicRangeDb")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "voicingRate",
            forensic
                .get("voicingRate")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "fluxOnsets",
            forensic
                .get("fluxOnsetsSec")
                .and_then(Value::as_array)
                .map(|onsets| json!(onsets.len())),
        );
        // The structure the profile measured, as counts: how many speaker
        // turns and how many questions the take holds. A critic told
        // "3 speaker turns" can ask whether the cut changed the angle on
        // them; a bare family name cannot be checked against the sheet.
        insert(
            "speakerTurns",
            reply
                .pointer("/speaker/turns")
                .and_then(Value::as_array)
                .map(|turns| json!(turns.len())),
        );
        insert(
            "questions",
            reply
                .pointer("/directorHints/questions")
                .and_then(Value::as_array)
                .map(|questions| json!(questions.len())),
        );
        insert(
            "bgmVerdict",
            reply
                .pointer("/directorHints/bgm/recommendation")
                .and_then(Value::as_str)
                .filter(|verdict| !verdict.is_empty())
                .map(|verdict| json!(verdict)),
        );
        insert(
            "integratedLufs",
            whole
                .pointer("/loudness/integratedLufs")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "speechRatio",
            whole
                .get("speechRatio")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "dominantPeriodSec",
            whole
                .get("dominantPeriodSec")
                .and_then(Value::as_f64)
                .map(|v| json!(v)),
        );
        insert(
            "rhythmLayers",
            whole
                .get("prosody")
                .and_then(Value::as_array)
                .map(|layers| {
                    json!(
                        layers
                            .iter()
                            .filter_map(|layer| Some(json!({
                                "periodSec": layer["periodSec"].as_f64()?,
                                "strength": layer["strength"].as_f64()?,
                            })))
                            .collect::<Vec<Value>>()
                    )
                }),
        );
    }
    if !take.is_empty() {
        out.insert("take".to_owned(), Value::Object(take));
    }
    // The per-line shape, as ranges rather than a row per line: a critic
    // reads "wpm 120-190, pitch slope -2.1..+1.8 st/s" and knows the take
    // moved, where a list of forty rows is not a brief.
    let lines: Vec<&Value> = log
        .iter()
        .flat_map(|reply| reply["lines"].as_array().into_iter().flatten())
        .collect();
    let span = |pointer: &str| -> Option<Value> {
        let values: Vec<f64> = lines
            .iter()
            .filter_map(|line| line.pointer(pointer).and_then(Value::as_f64))
            .collect();
        let low = values
            .iter()
            .copied()
            .fold(None::<f64>, |low: Option<f64>, v| {
                Some(low.map_or(v, |low| low.min(v)))
            })?;
        let high = values
            .iter()
            .copied()
            .fold(None::<f64>, |high: Option<f64>, v| {
                Some(high.map_or(v, |high| high.max(v)))
            })?;
        Some(json!({ "low": round3(low), "high": round3(high), "lines": values.len() }))
    };
    let mut shape = serde_json::Map::new();
    for (name, pointer) in [
        ("wordsPerMinute", "/speechRate/wordsPerMinute"),
        ("syllablesPerSec", "/speechRate/syllablesPerSec"),
        ("relativeLu", "/loudness/relativeLu"),
        (
            "pitchSlopeSemitonesPerSec",
            "/prosody/pitchSlopeSemitonesPerSec",
        ),
        ("pitchRangeSemitones", "/prosody/pitchRangeSemitones"),
    ] {
        if let Some(span) = span(pointer) {
            shape.insert(name.to_owned(), span);
        }
    }
    if !shape.is_empty() {
        out.insert("lines".to_owned(), Value::Object(shape));
    }
    Value::Object(out)
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
            // The sheet labels its tiles one-based ("01", "02", ...), so
            // the number the critic cites is the LABEL, and the tile
            // array is zero-based - the parsed label steps down one
            // before it indexes, or "frame 3" lands on the shot the
            // sheet calls 04.
            .filter(|label| *label >= 1)
            .and_then(|label| tiles.get(label - 1).cloned())
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

/// The fixed-spine source an agent's `auto_edit` call is written from:
/// the project's timeline view supplies every narration clip and its
/// placement, so the same program the window's button runs can run
/// headlessly over an MCP call.
///
/// A narration clip is audio with real media, outside the project's
/// `bgm/` folder - the same test the window's handoff makes. A project
/// with no such clip is refused with a sentence rather than run over
/// silence; the talking-head door remains the GUI's, because extracting
/// a video's audio and rebuilding its picture is the handoff's work.
pub fn auto_edit_source_from_view(
    project: &str,
    view: &Value,
    script: Option<String>,
    stock_provider: String,
) -> Result<AutoEditProgramSource, String> {
    let media = media_paths(view);
    let clips = view["timeline"]["clips"]
        .as_array()
        .ok_or_else(|| "the timeline view carries no clips array".to_owned())?;
    let mut ordered: Vec<&Value> = clips.iter().collect();
    ordered.sort_by(|a, b| {
        a["start"]
            .as_f64()
            .unwrap_or(0.0)
            .total_cmp(&b["start"].as_f64().unwrap_or(0.0))
    });
    let mut takes: Vec<TakePaths> = Vec::new();
    let mut placements: Vec<TakePlacement> = Vec::new();
    for clip in ordered {
        if clip["kind"].as_str() != Some("audio") {
            continue;
        }
        let Some(media_id) = clip["media"].as_str() else {
            continue;
        };
        let Some((_, path)) = media.iter().find(|(id, _)| id == media_id) else {
            continue;
        };
        if path.trim().is_empty() || path.contains("/bgm/") {
            continue;
        }
        let (source_in, source_out, speed) = clip_file_span(clip);
        let take = path.clone();
        if !takes.iter().any(|known| known.take == take) {
            let index = takes.len();
            let name = std::path::Path::new(&take)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("voiceover.wav");
            let enhanced = std::path::Path::new(project)
                .join("auto-edit")
                .join(format!("enhanced-{index:03}-{name}"))
                .to_string_lossy()
                .into_owned();
            takes.push(TakePaths {
                take: take.clone(),
                enhanced,
                aroll: None,
            });
        }
        placements.push(TakePlacement {
            take,
            start: clip["start"].as_f64().unwrap_or(0.0),
            source_in,
            source_out,
            speed,
        });
    }
    if takes.is_empty() {
        return Err(
            "the project has no narration audio clip on its timeline (an audio clip outside \
             the project's bgm/ folder); headless auto_edit edits narration audio, while a \
             talking-head video is the GUI's AUTO EDIT path"
                .to_owned(),
        );
    }
    Ok(AutoEditProgramSource {
        project: project.to_owned(),
        takes,
        placements,
        export: std::path::Path::new(project)
            .join("export")
            .join("auto-edit.mp4")
            .to_string_lossy()
            .into_owned(),
        sheet: "auto-edit-sheet.png".to_owned(),
        stock_provider,
        script: script.filter(|script| !script.trim().is_empty()),
    })
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
    let takes = source.takes.clone();
    let call = |tool: &str, arguments: Value| ToolCall {
        tool: tool.to_owned(),
        arguments,
    };
    let script = source
        .script
        .clone()
        .filter(|script| !script.trim().is_empty());
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
    // 2. The repair, per distinct take, on its own lane: measured first,
    //    then only the chain the measurements ordered. The old spine ran
    //    the same generic clean on every take - a hum paid for a
    //    broadband pass that barely touched it, a DC offset paid for a
    //    denoise that cannot remove a bias, and a muffled take was
    //    denoised into a deeper muffle. The diagnosis names the repair:
    //    the mains hum gets its notch, the DC offset its blocker, the
    //    muffled take a presence lift instead of the denoise. The
    //    enhanced chain still lands on the take the timeline plays, so
    //    the export ships the sound the repair made; one file's failure
    //    warns and skips the rest - the take ships as recorded.
    let repair: Vec<Vec<ToolCall>> = takes
        .iter()
        .map(|take| {
            vec![
                // The measurement is a call of its own so the reply - the
                // hum frequency, the DC offset, the flatness, the rolloff
                // - sits in the log where the critic and the signal brief
                // can read what the repair was chosen on.
                call(
                    "sense_audio_repair",
                    json!({
                        "action": "diagnose_take",
                        "audio": take.take,
                    }),
                ),
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
                        "action": "repair_by_diagnosis",
                        "audio": take.enhanced,
                        "out": take.take,
                        "overwrite": true,
                        // The same invariant the forced clean always kept:
                        // a declined repair writes nothing, and the
                        // timeline would go on playing the raw recording
                        // while the run believed it had shipped the
                        // enhanced one. Forced, the repair lands - and on
                        // a clean take what lands is the conservative
                        // 60 Hz door and nothing else, the diagnosis's
                        // own word that the take is cleaner than the cure.
                        "force": true,
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
    // The waits give up with "still running" rather than an error when
    // their budget ends; without a gate the stage then "completes" over a
    // transcription that never arrived, and every word-derived stage
    // downstream silently ships defaults - no captions, no B-roll slots,
    // no emotion lines. The gate names the transcription (the job-start
    // replies carry no state, so only the waits are judged).
    steps.push(
        FanStep::fan(Stage::Transcribe, words)
            .gate(ReplyGate::Word {
                field: "state",
                equals: "done",
                what: "a take's transcription was still running when its waits ran out",
            })
            .into(),
    );
    // 4. The delivery: pauses, emphasis, the read itself - one lane per
    //    take, each reply carrying its own audio path so the fan's
    //    landing order is no one's guess. The script, when the run was
    //    handed one, rides along: the reading aligns the transcript to
    //    the author's wording (misheard words corrected, timings kept)
    //    and its reply carries the alignment verdict beside the words.
    let signals: Vec<Vec<ToolCall>> = takes
        .iter()
        .map(|take| {
            let mut arguments = json!({ "audio": take.take });
            if let Some(script) = script.as_deref() {
                arguments["script"] = json!(script);
            }
            vec![call("voice_signals", arguments)]
        })
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
                            // The script read as language: the topic
                            // tiling names the chapter boundaries, and
                            // the connective analysis names the pivots
                            // (`but`, `however`) and the abrupt seams.
                            // Both are signals the vocabulary lists and
                            // nothing measured before this call, so the
                            // transition stage had to decide a seam on
                            // the mood alone. The tiling needs at least
                            // three lines to segment (a shorter take is
                            // one topic by definition), and its refusal
                            // would break the derive loop before the
                            // take's own emotion read - so a take this
                            // short simply skips the tiling, never the
                            // reads after it.
                            if lines.len() >= 3 {
                                calls.push(call(
                                    "sense_discourse",
                                    json!({ "action": "segment_topics", "lines": lines.clone() }),
                                ));
                            }
                            calls.push(call(
                                "sense_discourse",
                                json!({ "action": "analyze_discourse", "lines": lines.clone() }),
                            ));
                            // The surface act of each line: a question
                            // asks the viewer something, an imperative
                            // addresses them. Both earn a card the mood
                            // alone cannot justify, and the profile's
                            // graphic policy names this signal.
                            calls.push(call(
                                "sense_discourse",
                                json!({ "action": "detect_speech_acts", "lines": lines.clone() }),
                            ));
                            // The argument's own skeleton: which lines
                            // assert, which back an assertion, which hedge
                            // it. The profile's graphic policy names
                            // `claims` for the proof card, and nothing in
                            // the program produced it before this call.
                            calls.push(call(
                                "sense_discourse",
                                json!({ "action": "extract_claim_graph", "lines": lines.clone() }),
                            ));
                            calls.push(call(
                                "emotion",
                                json!({ "action": "extract", "audio": take.take, "lines": lines }),
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
    let plan_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Plan,
            Box::new({
                // The beat analysis must finish BEFORE the planner call is
                // built: constructing both calls in one queue used to bake
                // an empty beat list into the planner request.
                //
                // The grid's source is whichever music the cut will
                // actually carry: a take that already holds its own music
                // is analyzed as it stands, and a dry voiceover gets the
                // BED this run will ship - shopped, imported and trimmed
                // here, before the tighten and the grade read a runtime
                // the bed would otherwise inflate. A cold shelf (missing,
                // still loading, failed) is not waited on here: the music
                // stage owns the shelf's preparation, and a run on a cold
                // machine plans without a grid - the coverage floor still
                // tiles the picture, only the beat-snapping goes without.
                let mut phase = 0u8;
                let mut queue: Vec<ToolCall> = Vec::new();
                // The bed shopping's own cursor and findings: which door
                // of the query chain is open, and the bed file the import
                // landed (the beats' clock).
                let mut query_cursor = 0usize;
                let mut shelf_polls = 0usize;
                let mut bed_path: Option<String> = None;
                move |log| {
                    if phase == 0 {
                        phase = 1;
                        if let Some(music) = musical_take(log, &plan_source) {
                            return Some(call("analyze_beats", json!({ "path": music })));
                        }
                        // A dry voiceover: the bed is the beat source, so
                        // the shelf's state is the first question.
                        return Some(call("bgm_status", json!({})));
                    }
                    if phase == 1 {
                        // A take that carries its own music: the analysis
                        // is a job, and the grid lands when the wait
                        // returns.
                        if let Some(music) = musical_take(log, &plan_source) {
                            phase = 2;
                            return Some(call(
                                "analysis_wait",
                                json!({ "path": music, "timeoutSeconds": 590 }),
                            ));
                        }
                        // The bed question's answer: only a ready shelf is
                        // shopped here. A loading one is given a few short
                        // looks (a warm shelf that is finishing its fetch
                        // should not cost the run its grid), and anything
                        // else - missing, failed - moves the plan on.
                        let state = log.last().and_then(|reply| {
                            reply
                                .get("freepd")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                        });
                        match state.as_deref() {
                            Some("ready") => {
                                phase = 10;
                                let query = bed_query_chain(log, &plan_source)
                                    .get(query_cursor)
                                    .cloned()
                                    .flatten();
                                let mut arguments = json!({
                                    "source": "freepd",
                                    "limit": 5,
                                    "sort": "relevance",
                                });
                                if let Some(query) = query {
                                    arguments["query"] = json!(query);
                                }
                                return Some(call("bgm_search", arguments));
                            }
                            Some(word) if word.starts_with("loading") => {
                                shelf_polls += 1;
                                if shelf_polls > 4 {
                                    // A cold shelf is the music stage's to
                                    // warm: the plan moves on without a
                                    // grid, falling through to the
                                    // planner below in this same call.
                                    phase = 2;
                                } else {
                                    std::thread::sleep(Duration::from_millis(3000));
                                    return Some(call("bgm_status", json!({})));
                                }
                            }
                            _ => {
                                // Missing or failed: the same fall-through,
                                // in the same call.
                                phase = 2;
                            }
                        }
                    }
                    if phase == 10 {
                        // The search's answer: the bed is the track that
                        // fits the cut. An empty page walks the chain's
                        // next door; a chain with no doors left leaves the
                        // plan without a grid, and the music stage shops
                        // for itself later.
                        let tracks = match log
                            .last()
                            .and_then(|reply| reply["tracks"].as_array().cloned())
                        {
                            Some(tracks) => tracks,
                            None => {
                                // A reply that is not a search answer ends
                                // the bed question the same way the missing
                                // shelf did: the fall-through below plans
                                // without a grid, in this same call - never
                                // `return None`, which would end the STAGE
                                // and take the planner and `plan_validate`
                                // with it.
                                phase = 2;
                                Vec::new()
                            }
                        };
                        let runtime = narration_runtime(log);
                        let chosen = best_bed(&tracks, runtime)
                            .or_else(|| tracks.first().cloned())
                            .and_then(|track| track["id"].as_str().map(str::to_owned));
                        match chosen {
                            Some(id) => {
                                phase = 11;
                                return Some(call(
                                    "bgm_import",
                                    json!({
                                        "path": plan_project,
                                        "ids": [id],
                                        "start": 0.0,
                                        "monetized": true,
                                    }),
                                ));
                            }
                            None => {
                                query_cursor += 1;
                                let chain = bed_query_chain(log, &plan_source);
                                if query_cursor < chain.len() {
                                    let query = chain.get(query_cursor).cloned().flatten();
                                    let mut arguments = json!({
                                        "source": "freepd",
                                        "limit": 5,
                                        "sort": "relevance",
                                    });
                                    if let Some(query) = query {
                                        arguments["query"] = json!(query);
                                    }
                                    return Some(call("bgm_search", arguments));
                                }
                                // The chain's last door answered nothing:
                                // the plan falls through below, without a
                                // grid - never `return None`, which would
                                // end the STAGE and silently drop the
                                // planner's ranked B-roll and the
                                // `plan_validate` gate with it.
                                phase = 2;
                            }
                        }
                    }
                    if phase == 11 {
                        // The import's reply: read the timeline back to
                        // find the bed clip the import placed.
                        phase = 12;
                        return Some(call("timeline_read", json!({ "path": plan_project })));
                    }
                    if phase == 12 {
                        // The bed clip, found by its folder: trimmed to
                        // the narration's own extent so no later runtime
                        // read is inflated by a three-minute bed under a
                        // thirty-second cut, then analyzed for its beats.
                        let view = latest_view(log);
                        let media = view.map(media_paths);
                        let bed = view.and_then(|view| {
                            view["timeline"]["clips"]
                                .as_array()
                                .into_iter()
                                .flatten()
                                .find(|clip| {
                                    clip["media"].as_str().is_some_and(|id| {
                                        media.as_ref().is_some_and(|media| {
                                            media
                                                .iter()
                                                .find(|(mid, _)| mid == id)
                                                .is_some_and(|(_, path)| path.contains("/bgm/"))
                                        })
                                    })
                                })
                                .cloned()
                        });
                        if let Some(bed) = bed {
                            let bed_file = bed["media"].as_str().and_then(|id| {
                                media
                                    .as_ref()
                                    .and_then(|media| media.iter().find(|(mid, _)| mid == id))
                                    .map(|(_, path)| path.clone())
                            });
                            bed_path = bed_file;
                            let runtime = narration_runtime(log);
                            let duration = bed["duration"].as_f64().unwrap_or(0.0);
                            if runtime > 0.5 && duration > runtime + 0.05 {
                                phase = 13;
                                return Some(call(
                                    "edit_apply",
                                    json!({
                                        "path": plan_project,
                                        "command": {
                                            "op": "trimClip",
                                            "clipId": bed["id"],
                                            "edge": "end",
                                            "delta": round3(runtime - duration),
                                        },
                                    }),
                                ));
                            }
                            // Already the right length: the analysis is
                            // the next question, in this same call.
                            phase = 13;
                        } else {
                            // The import landed no bed clip: no grid, and
                            // the fall-through plans on.
                            phase = 2;
                        }
                    }
                    if phase == 13 {
                        // The trim's own reply, or the fall-through from a
                        // bed already the right length: analyze the bed
                        // file the import landed.
                        if let Some(bed) = bed_path.clone() {
                            phase = 14;
                            return Some(call("analyze_beats", json!({ "path": bed })));
                        }
                        // No bed file to analyze: the fall-through plans
                        // on without a grid.
                        phase = 2;
                    }
                    if phase == 14 {
                        if let Some(bed) = bed_path.clone() {
                            phase = 2;
                            return Some(call(
                                "analysis_wait",
                                json!({ "path": bed, "timeoutSeconds": 590 }),
                            ));
                        }
                        phase = 2;
                    }
                    if phase == 2 {
                        phase = 3;
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
                            if let Some(beats) = beats.clone() {
                                if music.as_deref() == Some(take.take.as_str()) {
                                    // The take's own music: its beats are
                                    // already in this take's clock.
                                    spec["beats"] = beats;
                                } else if bed_path.is_some() {
                                    // The bed's beats are in the timeline's
                                    // clock (the bed plays from zero):
                                    // translated into this take's own file
                                    // seconds, the clock its segments are
                                    // read in.
                                    spec["beats"] =
                                        beats_in_take_clock(&beats, &plan_settings, &take.take);
                                }
                            }
                            queue.push(call("footage_plan_suggest", spec));
                        }
                    }
                    if !queue.is_empty() {
                        return Some(queue.remove(0));
                    }
                    if phase == 3 {
                        phase = 4;
                        let plan = auto_plan(log, &plan_source, &plan_settings)?;
                        return Some(call("plan_validate", json!({ "plan": plan })));
                    }
                    None
                }
            }),
        )
        .warn()
        // The plan's STRUCTURE is a hard gate, not a warning: a plan the
        // validator refuses is an edit that cannot run - sections that do
        // not cover the runtime, segments claiming footage over audio, a
        // signalLog that names nothing. The creative gate stays a
        // warning: an audience the transcript never named is a real
        // limitation the run reports, not one it hides or invents
        // around. plan_validate runs in-process, so the gate cannot
        // fail on an environment.
        .gate(concat_director::ReplyGate::True {
            field: "valid",
            what: "the plan the run wrote did not validate",
        })
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
    let level_source = takes.clone();
    let level_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Timeline,
            Box::new({
                let mut queue: Option<Vec<ToolCall>> = None;
                let mut graded = false;
                let mut keyed = false;
                let mut mixed = false;
                let mut mixes: Option<Vec<ToolCall>> = None;
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
                        // The grade's own arc, on the layer it just landed
                        // on: the hook carries the look at full strength,
                        // the body settles under it, and the call to act
                        // lifts back - the plan's own per-act intensity
                        // variation, keyed on the package's own parameter.
                        // Values only ever move DOWN from the base the
                        // package ships, so no key can leave the range.
                        if !keyed {
                            keyed = true;
                            let layer = log
                                .last()
                                .and_then(|reply| reply.get("createdId").and_then(Value::as_str));
                            let runtime = latest_view(log).and_then(|view| {
                                view["timeline"]["clips"].as_array().map(|clips| {
                                    clips
                                        .iter()
                                        .map(|clip| {
                                            clip["start"].as_f64().unwrap_or(0.0)
                                                + clip["duration"].as_f64().unwrap_or(0.0)
                                        })
                                        .fold(0.0_f64, f64::max)
                                })
                            });
                            if let (Some(layer), Some(runtime)) = (layer, runtime)
                                && runtime > 1.0
                            {
                                let choice = grade_decision(log, &grade_source);
                                let (key, base) = grade_param(choice.look);
                                let body = base * 0.85;
                                let mut commands = vec![json!({
                                    "op": "setEffectKey",
                                    "clipId": layer,
                                    "entry": 0,
                                    "key": key,
                                    "at": 0.0,
                                    "value": base,
                                })];
                                let hook_share = (3.0 / runtime).clamp(0.05, 0.4);
                                commands.push(json!({
                                    "op": "setEffectKey",
                                    "clipId": layer,
                                    "entry": 0,
                                    "key": key,
                                    "at": round3(hook_share),
                                    "value": round3(body),
                                }));
                                if runtime > 12.0 && choice.builds {
                                    commands.push(json!({
                                        "op": "setEffectKey",
                                        "clipId": layer,
                                        "entry": 0,
                                        "key": key,
                                        "at": round3(
                                            ((runtime - 2.0) / runtime).max(0.0)
                                        ),
                                        "value": base,
                                    }));
                                }
                                return Some(call(
                                    "edit_apply",
                                    json!({
                                        "path": grade_project,
                                        "command": {
                                            "op": "batch",
                                            "commands": commands,
                                        },
                                    }),
                                ));
                            }
                        }
                        // The mix: the profile measured the take's own
                        // integrated loudness, and the plan already names
                        // the platform the cut ships to. The one number the
                        // two give is the gain that puts the voice on the
                        // platform's target - so the verify stage's
                        // loudness gate is met by measurement rather than
                        // by luck. A take already within a LU of target
                        // takes no edit.
                        //
                        // The gain the target asks for is then capped by
                        // the safety readings on the loudest take: a take
                        // that already clips is never raised, a take whose
                        // peaks sit near full scale only rises to the -1
                        // dB ceiling (the true peak when the loudness pass
                        // measured one), and a heavily compressed take
                        // takes at most +6 dB. Without the cap the stage
                        // could meet the platform target by pushing the
                        // mix into overs the verify gate would own later.
                        if mixed {
                            // One level call per clip, one invocation each:
                            // the queue drains after the gain each clip's
                            // own take asked for.
                            return mixes
                                .as_mut()
                                .and_then(|queue| (!queue.is_empty()).then(|| queue.remove(0)));
                        }
                        mixed = true;
                        let target = match platform_target(log) {
                            Some(target) => target,
                            None => return None,
                        };
                        let view = latest_view(log)?;
                        let media = media_paths(view);
                        let reads = voice_reads(log, &level_source);
                        // The gain belongs on EVERY clip that plays a
                        // measured take, each from that take's own
                        // reading - not the one clip that plays the
                        // loudest take: with multiple placements, or
                        // takes of differing loudness, the other clips
                        // kept their uncorrected level, and the export's
                        // integrated loudness missed the platform target
                        // the verify stage then failed the whole run for.
                        let levels: Vec<ToolCall> = view["timeline"]["clips"]
                            .as_array()?
                            .iter()
                            .filter_map(|clip| {
                                let path = clip["media"].as_str().and_then(|id| {
                                    media
                                        .iter()
                                        .find(|(mid, _)| mid == id)
                                        .map(|(_, path)| path.clone())
                                })?;
                                let read = reads.iter().find(|read| {
                                    read.take == path && read.take_wide.integrated_lufs.is_some()
                                })?;
                                let measured = read.take_wide.integrated_lufs?;
                                let gain = round3(read.safe_gain(target - measured))
                                    // clip_audio_levels_db refuses baseDb outside
                                    // -60..+24; a very quiet take's wanted gain
                                    // (e.g. -40 LUFS asking for +26) used to get
                                    // the mix call refused after the grade had
                                    // already landed.
                                    .min(24.0);
                                if gain.abs() < 1.0 {
                                    return None;
                                }
                                let clip_id = clip["id"].as_str()?.to_owned();
                                Some(call(
                                    "clip_audio_levels_db",
                                    json!({
                                        "path": level_project.clone(),
                                        "clipId": clip_id,
                                        "baseDb": gain,
                                        "replaceAutomation": false,
                                    }),
                                ))
                            })
                            .collect();
                        if levels.is_empty() {
                            return None;
                        }
                        mixes = Some(levels);
                        return mixes
                            .as_mut()
                            .and_then(|queue| (!queue.is_empty()).then(|| queue.remove(0)));
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
                    let look = grade_decision(log, &grade_source).look;
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
                // How many of one slot's hits the run actually looks at:
                // enough alternates for the continuity pass to swap to,
                // without fetching a whole page.
                const LOOK_AT: usize = 3;
                let mut slots: Option<Vec<BrollSlot>> = None;
                // Where the shopping stands: which slot, and which of its
                // phases - search, fetch, sheet - is next.
                let mut shopping = 0usize;
                let mut phase = GatherPhase::Search;
                let mut fetch_queue: Vec<Value> = Vec::new();
                let mut fetched: Vec<StockCandidate> = Vec::new();
                // The sheet-ranked candidates awaiting the emotion gate's
                // word - the choice is spent only after the register
                // allowed it.
                let mut pending_gate: Vec<StockCandidate> = Vec::new();
                // The slots whose choice the sheet made, in play order:
                // the continuity pass reads them, the import pass spends
                // them. A slot the continuity pass downgraded holds
                // instead - the A-roll covers its span, which is the
                // remedy the gate itself names.
                let mut shopped: Vec<SlotChoice> = Vec::new();
                // The clip ids and frame hashes already chosen: a shelf
                // can return the same clip for two neighbouring gaps, and
                // two visually identical clips with different ids cut
                // back to back read as a mistake to anyone watching.
                let mut used_ids: Vec<String> = Vec::new();
                let mut used_hashes: Vec<String> = Vec::new();
                // The continuity check's pass: the first asks, the second
                // records the state the repairs left.
                let mut continuity_pass = 0u8;
                // The slot being imported and placed, by index into
                // `shopped`.
                let mut placing = 0usize;
                // The placement's own facts, drawn from the import's
                // reply and spent by the styling batch.
                let mut pending_media: Option<String> = None;
                let mut pending_len = 0.0f64;
                let mut pending_pace = String::new();
                // The take's own cut style, read once from the profile's
                // derived hint: it decides how much every shot moves when
                // the delivery over its gap was not itself measured.
                let mut pending_style = String::new();
                // The energy curve's own reading under the gap, 0..1: a
                // shot laid over a loud stretch moves a touch more than
                // the same-paced line over a quiet one.
                let mut pending_energy: Option<f64> = None;
                // The take's measured tempo, when the prosody read found
                // one: the seam's blend is a fraction of the period the
                // energy actually repeats on, so a slow take dissolves
                // slowly and a clipped one snaps.
                let mut pending_period: Option<f64> = None;
                let mut pending_transition: Option<&'static str> = None;
                // The gap's own timeline position, held across the lane
                // minting call: the placement that follows the lane's
                // reply places at the gap the import mapped, not at
                // whatever the intervening reply left in flight.
                let mut pending_at: f64 = 0.0;
                // The transition the last placed shot cut in with: the
                // next shot never wears the same move twice running.
                let mut last_transition: Option<&'static str> = None;
                // The one lane every B-roll clip is placed on: a seam's
                // transition only renders between same-lane neighbours
                // (the export's resolver matches the outgoing clip that
                // ends where this one starts, on the SAME track), so a
                // first-free walk that spread the shots over whatever
                // lanes had room orphaned every transitionIn this
                // program ever wrote - the Python director's dedicated
                // overlay lane is the shape that works, and this is it.
                let mut broll_lane: Option<String> = None;
                // The takes' readings, read once: the seam of every gap is
                // judged against the same measurement.
                let mut voice: Option<Vec<VoiceRead>> = None;
                // The script's own seams: a topic boundary and a
                // connective pivot are the places a visual changes
                // because the argument did, not because the mood did.
                let mut chapters: Option<Vec<TopicBoundary>> = None;
                let mut pivots: Option<Vec<(String, f64)>> = None;
                move |log| loop {
                    let voice = voice.get_or_insert_with(|| voice_reads(log, &gather_emotion));
                    let chapters =
                        chapters.get_or_insert_with(|| topic_boundaries(log, &gather_emotion));
                    let pivots =
                        pivots.get_or_insert_with(|| discourse_pivots(log, &gather_emotion));
                    let slots = slots.get_or_insert_with(|| broll_slots(log, &gather_source));
                    match phase {
                        GatherPhase::Search => {
                            if shopping >= slots.len() {
                                phase = GatherPhase::Continuity;
                                continue;
                            }
                            phase = GatherPhase::ReadSearch;
                            // The shelf is shopped for the frame the
                            // project actually is: a vertical reel asks
                            // the shelf for vertical clips, so the page
                            // the sheet looks at is not full of clips
                            // the fill would have to crop against their
                            // grain.
                            let mut arguments = json!({
                                "provider": gather_provider.clone(),
                                "query": slots[shopping].query.clone(),
                                "limit": 6,
                            });
                            if let Some(orientation) =
                                latest_view(log).map(|view| shelf_orientation(aspect_of(view)))
                            {
                                arguments["orientation"] = json!(orientation);
                            }
                            return Some(call("stock_search", arguments));
                        }
                        GatherPhase::ReadSearch => {
                            // Only live-shelf hits with an explicit
                            // provider licence are eligible to be looked
                            // at. OpenVid has no per-clip licence record
                            // and is never imported here.
                            let span = slots[shopping].to - slots[shopping].from;
                            let aspect = latest_view(log)
                                .map(|view| aspect_of(view).to_owned())
                                .unwrap_or_else(|| "16:9".to_owned());
                            let hits = log
                                .last()
                                .and_then(|reply| reply["hits"].as_array())
                                .cloned()
                                .unwrap_or_default();
                            let fits = fitting_hits(&hits, span, &aspect, &used_ids, LOOK_AT);
                            if fits.is_empty() {
                                // A page with no eligible hit is a slot
                                // left empty - the honest answer - not a
                                // clip that will not fit.
                                shopping += 1;
                                phase = GatherPhase::Search;
                                continue;
                            }
                            fetch_queue = fits;
                            fetched = Vec::new();
                            phase = GatherPhase::Fetch;
                            continue;
                        }
                        GatherPhase::Fetch => {
                            let Some(hit) = fetch_queue.first().cloned() else {
                                if fetched.is_empty() {
                                    // Every fetch refused: the slot is
                                    // left empty, like an empty page.
                                    shopping += 1;
                                    phase = GatherPhase::Search;
                                    continue;
                                }
                                phase = GatherPhase::Sheet;
                                continue;
                            };
                            phase = GatherPhase::ReadFetch;
                            return Some(call(
                                "stock_fetch",
                                json!({
                                    "provider": hit["provider"].clone(),
                                    "id": hit["id"].clone(),
                                    "url": hit["url"].clone(),
                                }),
                            ));
                        }
                        GatherPhase::ReadFetch => {
                            let hit = fetch_queue.remove(0);
                            let path = log
                                .last()
                                .and_then(|reply| reply["path"].as_str())
                                .map(str::to_owned)
                                .filter(|path| !path.trim().is_empty());
                            if let Some(path) = path {
                                fetched.push(StockCandidate {
                                    provider: hit["provider"]
                                        .as_str()
                                        .unwrap_or_default()
                                        .to_owned(),
                                    id: hit["id"].as_str().unwrap_or_default().to_owned(),
                                    url: hit["url"].as_str().unwrap_or_default().to_owned(),
                                    caption: hit["caption"]
                                        .as_str()
                                        .or_else(|| hit["name"].as_str())
                                        .unwrap_or_default()
                                        .to_owned(),
                                    local_path: path,
                                    width: hit["width"].as_f64().unwrap_or(0.0),
                                    height: hit["height"].as_f64().unwrap_or(0.0),
                                    tile: Value::Null,
                                });
                            }
                            phase = GatherPhase::Fetch;
                            continue;
                        }
                        GatherPhase::Sheet => {
                            // Look at the candidates before choosing one:
                            // one real frame each, measured - the sheet is
                            // the approval record the choice stands on.
                            let paths: Vec<Value> = fetched
                                .iter()
                                .map(|candidate| json!(candidate.local_path))
                                .collect();
                            let aspect = latest_view(log)
                                .map(|view| aspect_of(view).to_owned())
                                .unwrap_or_else(|| "16:9".to_owned());
                            phase = GatherPhase::ReadSheet;
                            return Some(call(
                                "stock_contact_sheet",
                                json!({
                                    "paths": paths,
                                    "cols": LOOK_AT,
                                    "aspect": aspect,
                                    "autoSetup": false,
                                }),
                            ));
                        }
                        GatherPhase::ReadSheet => {
                            let tiles = log
                                .last()
                                .and_then(|reply| reply["tiles"].as_array())
                                .cloned()
                                .unwrap_or_default();
                            for tile in &tiles {
                                let Some(path) = tile["localPath"].as_str() else {
                                    continue;
                                };
                                if let Some(candidate) = fetched
                                    .iter_mut()
                                    .find(|candidate| candidate.local_path == path)
                                {
                                    candidate.tile = tile.clone();
                                }
                            }
                            // The sheet's own measurements reorder the
                            // page: chroma, contrast and detail for, and
                            // black frames, blown ones, letterboxing and
                            // visual duplicates against. A candidate the
                            // sheet could not measure keeps its shelf
                            // order behind every measured one - the old
                            // unexamined choice, not a refused slot, when
                            // nothing at all was measured.
                            let mut candidates = std::mem::take(&mut fetched);
                            candidates.sort_by(|left, right| {
                                let left = left.visual_score(&used_hashes);
                                let right = right.visual_score(&used_hashes);
                                match (left, right) {
                                    (Some(left), Some(right)) => right.total_cmp(&left),
                                    (Some(_), None) => std::cmp::Ordering::Less,
                                    (None, Some(_)) => std::cmp::Ordering::Greater,
                                    (None, None) => std::cmp::Ordering::Equal,
                                }
                            });
                            if candidates.is_empty() {
                                shopping += 1;
                                phase = GatherPhase::Search;
                                continue;
                            }
                            // The emotion gate rides between the sheet and
                            // the choice: the pixels were the sheet's job,
                            // the register is the gate's (EMOTION.md's
                            // "gate included" - which the one-click path
                            // never ran, so a candidate no line's register
                            // allowed took the slot on looks alone).
                            // The picture's own read rides between the
                            // sheet and the gate: the sheet judged one
                            // frame per candidate, and the frames a clip
                            // actually plays are the ones the cut spends.
                            pending_gate = candidates;
                            phase = GatherPhase::Visual;
                            continue;
                        }
                        GatherPhase::Visual => {
                            let paths: Vec<Value> = pending_gate
                                .iter()
                                .map(|candidate| json!(candidate.local_path))
                                .collect();
                            phase = GatherPhase::ReadVisual;
                            return Some(call(
                                "sense_visual",
                                json!({
                                    "action": "measure_clips",
                                    "paths": paths,
                                    "intent": "cut",
                                    "samples": 6,
                                    "width": 64,
                                    "maxClips": 3,
                                }),
                            ));
                        }
                        GatherPhase::ReadVisual => {
                            let reads = visual_reads(log);
                            // A candidate the picture refused - soft,
                            // noisy, letterboxed, flat, dark, blown, or a
                            // montage that cuts inside itself - sinks
                            // behind one it passed, when a passing
                            // alternative exists. The sheet's own order
                            // survives inside each class: this is a veto,
                            // not a re-ranking.
                            let refused = |candidate: &StockCandidate| {
                                visual_read_of(&reads, &candidate.local_path)
                                    .is_some_and(|read| !read.passes() || read.montage())
                            };
                            let any_pass = pending_gate.iter().any(|candidate| !refused(candidate));
                            if any_pass {
                                pending_gate.sort_by(|left, right| {
                                    let class = refused(left).cmp(&refused(right));
                                    class.then_with(|| {
                                        let left = visual_read_of(&reads, &left.local_path)
                                            .and_then(|read| read.score())
                                            .unwrap_or(0.0);
                                        let right = visual_read_of(&reads, &right.local_path)
                                            .and_then(|read| read.score())
                                            .unwrap_or(0.0);
                                        right.total_cmp(&left)
                                    })
                                });
                            }
                            phase = GatherPhase::Gate;
                            continue;
                        }
                        GatherPhase::Gate => {
                            let slot = &slots[shopping];
                            // Bound once: the gate's answer and the segment
                            // it gated both read these lines.
                            let lines = emotion_lines(log, &gather_source);
                            let line = emotion_at(&lines, &slot.take, slot.from);
                            let segment = json!({
                                "emotion": line
                                    .map(|line| line.emotion.clone())
                                    .unwrap_or_else(|| "neutral".to_owned()),
                                "act": line
                                    .map(|line| line.act.clone())
                                    .unwrap_or_else(|| "setup".to_owned()),
                            });
                            let gate_candidates: Vec<Value> = pending_gate
                                .iter()
                                .enumerate()
                                .map(|(index, candidate)| {
                                    // The candidate's OWN words go to the
                                    // gate: the shelf's caption names what
                                    // the clip shows, and a caption-less
                                    // hit falls back to the slot's query
                                    // rather than a bare tile ordinal
                                    // ("1 ", "2 ") that judges nothing.
                                    let text = if candidate.caption.trim().is_empty() {
                                        slot.query.clone()
                                    } else {
                                        candidate.caption.clone()
                                    };
                                    json!({
                                        "index": index,
                                        "id": candidate.id,
                                        "caption": text,
                                    })
                                })
                                .collect();
                            phase = GatherPhase::ReadGate;
                            return Some(call(
                                "emotion",
                                json!({
                                    "action": "gate",
                                    "segment": segment,
                                    "candidates": gate_candidates,
                                }),
                            ));
                        }
                        GatherPhase::ReadGate => {
                            // Among the candidates the gate passed, the
                            // sheet's own ranking decides: visual quality
                            // picks within the register the gate allowed.
                            // A gate that ANSWERED is authoritative: its
                            // `order` (possibly empty - every candidate
                            // failed 2+ checks, and the beat is dropped,
                            // never padded) decides among the visually
                            // qualified candidates. A reply that did not
                            // answer (a refusal shape, a hosted-model
                            // error riding the log) is a DEGRADED gate,
                            // not a rejected beat: the sheet's own ranking
                            // decides, the way it did before the gate
                            // existed, among candidates the pixels did
                            // not disqualify.
                            let gate_answered = log.last().is_some_and(|reply| {
                                reply.get("order").is_some() || reply["ok"].as_bool() == Some(true)
                            });
                            let passing: Vec<usize> = log
                                .last()
                                .and_then(|reply| reply["order"].as_array())
                                .map(|order| {
                                    order
                                        .iter()
                                        .filter_map(Value::as_i64)
                                        .map(|index| index as usize)
                                        .collect()
                                })
                                .unwrap_or_default();
                            let visual: Vec<usize> = pending_gate
                                .iter()
                                .enumerate()
                                .filter(|(_, candidate)| {
                                    candidate.visual_score(&used_hashes).is_some()
                                })
                                .map(|(index, _)| index)
                                .collect();
                            // The picture's veto stands again at the
                            // choice: a candidate the frames refused never
                            // takes the slot while one they passed is on
                            // the page, whatever the gate's order says.
                            let reads = visual_reads(log);
                            let picture_ok = |index: usize| {
                                visual_read_of(&reads, &pending_gate[index].local_path)
                                    .is_none_or(|read| read.passes() && !read.montage())
                            };
                            let any_picture_ok = (0..pending_gate.len()).any(picture_ok);
                            let chosen = if gate_answered {
                                passing
                                    .into_iter()
                                    .filter(|index| visual.contains(index))
                                    .filter(|index| !any_picture_ok || picture_ok(*index))
                                    .min()
                            } else {
                                visual
                                    .into_iter()
                                    .filter(|index| !any_picture_ok || picture_ok(*index))
                                    .min()
                            };
                            if let Some(chosen) = chosen {
                                let best = &pending_gate[chosen];
                                used_ids.push(best.id.clone());
                                if let Some(hash) =
                                    best.tile["hash"].as_str().filter(|hash| !hash.is_empty())
                                {
                                    used_hashes.push(hash.to_owned());
                                }
                                shopped.push(SlotChoice {
                                    id: format!("slot-{:02}", shopped.len()),
                                    slot: slots[shopping].clone(),
                                    candidates: std::mem::take(&mut pending_gate),
                                    chosen,
                                    held: false,
                                    force_push: false,
                                });
                            }
                            // Every candidate rejected (or no gate answer):
                            // the beat is dropped - the A-roll covers the
                            // span, the way the gate's own recommendation
                            // reads. Never pad with a bad match.
                            shopping += 1;
                            phase = GatherPhase::Search;
                            continue;
                        }
                        GatherPhase::Continuity => {
                            // The gate needs adjacencies to judge: one
                            // shot has no neighbour to jump against.
                            if shopped.iter().filter(|shop| !shop.held).count() < 2 {
                                phase = GatherPhase::Import;
                                continue;
                            }
                            phase = GatherPhase::ReadContinuity;
                            continuity_pass += 1;
                            return Some(call(
                                "footage_continuity",
                                json!({ "clips": continuity_manifest(&shopped, &visual_reads(log)) }),
                            ));
                        }
                        GatherPhase::ReadContinuity => {
                            let flags = log
                                .last()
                                .and_then(|reply| reply["flags"].as_array())
                                .cloned()
                                .unwrap_or_default();
                            for flag in &flags {
                                let Some(slot_id) = flag["slotId"].as_str() else {
                                    continue;
                                };
                                let rule = flag["rule"].as_str().unwrap_or_default();
                                let Some(shop) = shopped.iter_mut().find(|shop| shop.id == slot_id)
                                else {
                                    continue;
                                };
                                match rule {
                                    // A colour jump the sheet measured:
                                    // the swap the alternates exist for -
                                    // and the new choice is SPENT, so a
                                    // second flagged slot can never swap
                                    // onto the same clip.
                                    "colour" => {
                                        if let Some((id, hash)) =
                                            shop.swap_past(&used_ids, &used_hashes)
                                        {
                                            used_ids.push(id);
                                            if !hash.is_empty() {
                                                used_hashes.push(hash);
                                            }
                                        }
                                    }
                                    // A category run: the gate's own
                                    // remedy is the treatment's, and the
                                    // treatment the program has is the
                                    // hold - the A-roll covers the span.
                                    "monotony" => shop.held = true,
                                    // A travel reversal or a repeated
                                    // scale: the alternate is the search's
                                    // own remedy - a different picture,
                                    // spent so no other slot takes it. The
                                    // second pass re-reads the manifest and
                                    // records whatever is left.
                                    "motion" | "scale" => {
                                        if let Some((id, hash)) =
                                            shop.swap_past(&used_ids, &used_hashes)
                                        {
                                            used_ids.push(id);
                                            if !hash.is_empty() {
                                                used_hashes.push(hash);
                                            }
                                        }
                                    }
                                    // The clip is too still for its slot.
                                    // The picture stays - it passed the
                                    // read - and the drift carries the
                                    // movement the shot does not.
                                    "static" => shop.force_push = true,
                                    _ => {}
                                }
                            }
                            if !flags.is_empty() && continuity_pass == 1 {
                                // One repair pass, then the state the
                                // repairs left is what the record shows.
                                phase = GatherPhase::ReadContinuity;
                                continuity_pass += 1;
                                return Some(call(
                                    "footage_continuity",
                                    json!({ "clips": continuity_manifest(&shopped, &visual_reads(log)) }),
                                ));
                            }
                            phase = GatherPhase::Import;
                            continue;
                        }
                        GatherPhase::Import => {
                            while placing < shopped.len() && shopped[placing].held {
                                placing += 1;
                            }
                            let Some(shop) = shopped.get(placing) else {
                                // Every slot imported, held or empty: the
                                // picture stage is done.
                                return None;
                            };
                            let candidate = &shop.candidates[shop.chosen];
                            phase = GatherPhase::ReadImport;
                            return Some(call(
                                "stock_import",
                                json!({
                                    "path": gather_project,
                                    "provider": candidate.provider.clone(),
                                    "id": candidate.id.clone(),
                                    "url": candidate.url.clone(),
                                }),
                            ));
                        }
                        GatherPhase::ReadImport => {
                            // The import's own reply (`{mediaIds, view}`):
                            // the clip is in the bin, and the view maps
                            // the gap onto the timeline. Lay the clip
                            // there.
                            let media_id = log
                                .last()
                                .and_then(|reply| reply.pointer("/mediaIds/0"))
                                .and_then(|id| id.as_str())
                                .map(str::to_owned);
                            let slot = shopped[placing].slot.clone();
                            // The bounds map through the SURVIVING pieces:
                            // a bound that lands inside a span the tighten
                            // removed maps to the join the cut made, so a
                            // slot planned over a dead-air gap is placed
                            // at the seam instead of being silently
                            // dropped - the shot survives the cut that
                            // took its span.
                            let at = latest_view(log).and_then(|view| {
                                let media = media_paths(view);
                                timeline_at_or_after(view, &media, &slot.take, slot.from)
                            });
                            let to = latest_view(log).and_then(|view| {
                                let media = media_paths(view);
                                timeline_at_or_before(view, &media, &slot.take, slot.to)
                            });
                            match (media_id, at, to) {
                                (Some(media_id), Some(at), Some(to)) if to - at > 0.4 => {
                                    pending_media = Some(media_id.clone());
                                    pending_len = round3(to - at);
                                    // The delivery over the gap sets how
                                    // much the shot moves: a punchy
                                    // stretch takes a livelier drift, a
                                    // slow one a quieter, longer push.
                                    pending_pace =
                                        delivery_pace(log, &slot.take, slot.from, slot.to);
                                    // The take's derived cut style stands
                                    // behind a gap whose own delivery was
                                    // too short to measure: a profile
                                    // that read the whole take knows its
                                    // rhythm even where one span did not.
                                    let take_wide = voice_read_of(voice, &slot.take)
                                        .map(|read| read.take_wide.clone())
                                        .unwrap_or_default();
                                    // The profile's derived style, or - when
                                    // it derived none - the honest fallback
                                    // the measurements themselves answer,
                                    // each in its turn: the onsets' rate
                                    // (one a second is a take that moves),
                                    // then the VAD's rhythm (six short
                                    // utterances is machine-gun delivery),
                                    // then the flux's mean against the
                                    // level it moved from. A take no ear
                                    // could read keeps the default arm.
                                    let take_read = voice_read_of(voice, &slot.take);
                                    pending_style = take_wide.cut_style.clone();
                                    if pending_style.is_empty()
                                        && take_wide
                                            .onset_rate_per_min
                                            .is_some_and(|rate| rate >= 60.0)
                                    {
                                        pending_style = "fast".to_owned();
                                    }
                                    if pending_style.is_empty()
                                        && take_read.is_some_and(|read| {
                                            read.punchy_delivery() == Some(true)
                                        })
                                    {
                                        pending_style = "fast".to_owned();
                                    }
                                    if pending_style.is_empty()
                                        && take_read
                                            .as_ref()
                                            .is_some_and(|read| read.moves_hard() == Some(true))
                                    {
                                        pending_style = "fast".to_owned();
                                    }
                                    // The arc the emotion read measured is the
                                    // one input the fallback never had: a
                                    // take whose feeling climbs - the last
                                    // third reading clearly over the first -
                                    // is a building cut even when no rate,
                                    // rhythm or flux reading said "fast".
                                    // The drift already knows what building
                                    // means; the profile just could not
                                    // derive it without the emotion read the
                                    // unattended spine hands it late.
                                    if pending_style.is_empty()
                                        && arc_building(
                                            &emotion_lines(log, &gather_emotion),
                                            &slot.take,
                                        )
                                    {
                                        pending_style = "building".to_owned();
                                    }
                                    pending_period = take_read
                                        .as_ref()
                                        // The period the pulse has
                                        // strength to stand behind, from
                                        // the derived key or the strongest
                                        // layer's own row: a period without
                                        // its strength is an
                                        // autocorrelation artefact, and it
                                        // must not size a seam.
                                        .and_then(|read| read.pulse_period())
                                        // The breath-group the VAD measured
                                        // sizes the blend when the prosody
                                        // read found no period worth its
                                        // strength: an utterance length is a
                                        // measured rhythm, not an artefact,
                                        // so it stands without the guard.
                                        .or_else(|| {
                                            take_read.as_ref().and_then(|read| read.breath_period())
                                        });
                                    pending_energy = voice_read_of(voice, &slot.take)
                                        .and_then(|read| read.local_energy(slot.from));
                                    pending_transition = Some(seam_transition(
                                        voice,
                                        chapters,
                                        pivots,
                                        &emotion_lines(log, &gather_emotion),
                                        &slot,
                                        last_transition,
                                    ));
                                    phase = if broll_lane.is_some() {
                                        GatherPhase::Place
                                    } else {
                                        // The lane every shot shares, minted
                                        // before the first placement: a
                                        // transition renders between
                                        // same-lane neighbours, so this one
                                        // lane is what makes the seam moves
                                        // below more than decoration.
                                        GatherPhase::ReadTrack
                                    };
                                    pending_at = round3(at);
                                    return Some(if let Some(lane) = broll_lane.clone() {
                                        call(
                                            "edit_apply",
                                            json!({
                                                "path": gather_project,
                                                "command": {
                                                    "op": "addClip",
                                                    "mediaId": media_id,
                                                    "trackId": lane,
                                                    "start": round3(at),
                                                },
                                            }),
                                        )
                                    } else {
                                        call(
                                            "edit_apply",
                                            json!({
                                                "path": gather_project,
                                                "command": { "op": "addTrack" },
                                            }),
                                        )
                                    });
                                }
                                _ => {
                                    placing += 1;
                                    phase = GatherPhase::Import;
                                    continue;
                                }
                            }
                        }
                        GatherPhase::ReadTrack => {
                            // The lane's own reply: the id every B-roll
                            // placement uses from here on. A reply without
                            // one falls back to the first-free walk for
                            // this shot - and the next slot's placement
                            // re-attempts the minting, so a transient
                            // failure costs one shot's neighbour, not the
                            // whole program's seams.
                            let lane = log
                                .last()
                                .and_then(|reply| reply.get("createdId"))
                                .and_then(|id| id.as_str())
                                .map(str::to_owned);
                            let media_id = pending_media.clone();
                            let at = pending_at;
                            phase = GatherPhase::Place;
                            return Some(match lane {
                                Some(lane) => {
                                    broll_lane = Some(lane.clone());
                                    call(
                                        "edit_apply",
                                        json!({
                                            "path": gather_project,
                                            "command": {
                                                "op": "addClip",
                                                "mediaId": media_id,
                                                "trackId": lane,
                                                "start": at,
                                            },
                                        }),
                                    )
                                }
                                None => call(
                                    "edit_apply",
                                    json!({
                                        "path": gather_project,
                                        "command": {
                                            "op": "addClipAtFirstFree",
                                            "mediaId": media_id,
                                            "start": at,
                                        },
                                    }),
                                ),
                            });
                        }
                        GatherPhase::Place => {
                            // The placement's own reply: the clip the
                            // import just added, by the media it plays.
                            // Trim it to the gap, fill the frame, drift
                            // it, dissolve it in and follow the subject
                            // the sheet found - one undoable batch.
                            let media_id = pending_media.clone();
                            let shop = &shopped[placing];
                            let candidate = &shop.candidates[shop.chosen];
                            let aspect = latest_view(log)
                                .map(|view| aspect_of(view).to_owned())
                                .unwrap_or_else(|| "16:9".to_owned());
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
                            placing += 1;
                            phase = GatherPhase::Import;
                            let Some((clip_id, current)) = found else {
                                continue;
                            };
                            // The picture's own read of the chosen
                            // candidate: how it moves, how wide it is and
                            // where its subject sits. Every arm below is a
                            // decision this read made.
                            let reads = visual_reads(log);
                            let read = visual_read_of(&reads, &candidate.local_path);
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
                            // The shot's entrance lands on the movement the
                            // read found, not on its first frame: the clip
                            // opens a quarter second before its motion peak,
                            // inside the media it can still play.
                            if let Some(read) = read {
                                let limit = (current - pending_len).max(0.0);
                                let lead = round3(read.in_point(limit));
                                if lead > 0.05 {
                                    commands.push(json!({
                                        "op": "setClipSource",
                                        "clipId": clip_id,
                                        "sourceStart": lead,
                                    }));
                                }
                            }
                            commands.push(json!({
                                "op": "frameClip",
                                "clipId": clip_id,
                                "framing": "fill",
                            }));
                            // The drift is for a still frame. A shot whose
                            // camera or subject already travels gets no
                            // added push - the redundant Ken Burns the
                            // picture read exists to prevent - and a
                            // close-up already fills the frame, so it
                            // holds. The continuity gate's stillness flag
                            // overrides both: that slot asked for motion.
                            let moving = read.is_some_and(VisualRead::moves) && !shop.force_push;
                            let hold_scale = read.and_then(VisualRead::shot_scale)
                                == Some(concat_motion::continuity::ShotScale::CloseUp)
                                && !shop.force_push;
                            let drift_enabled = !moving && !hold_scale;
                            if drift_enabled {
                                commands.push(json!({
                                    "op": "setClipKey",
                                    "clipId": clip_id,
                                    "property": "scale",
                                    "at": 0.0,
                                    "value": 1.0,
                                    "ease": [0.0, 0.0, 0.58, 1.0],
                                }));
                            }
                            // The delivery over the gap sets how much the
                            // shot moves: a punchy stretch takes a livelier
                            // drift, a slow one a quieter, longer push. The
                            // style arms match the profile's own cut-style
                            // words - `fast`, `hold`, `building`, `mixed` -
                            // which is what the hint carries; the pace arms
                            // answer the gap's own delivery.
                            let mut drift = match pending_pace.as_str() {
                                "punchy" => 1.10,
                                "conversational" => 1.07,
                                "slow" => 1.03,
                                _ => match pending_style.as_str() {
                                    // The gap's own delivery was not
                                    // measured, so the take's derived cut
                                    // style decides the move.
                                    "fast" | "building" => 1.10,
                                    "hold" => 1.03,
                                    "mixed" => 1.07,
                                    _ => 1.06,
                                },
                            };
                            // The hook's shot pushes harder: a clip that
                            // opens the cut is the one a viewer decides
                            // to stay on, and a stronger push-in is the
                            // cheapest retention the catalogue has.
                            let hook_shot = latest_view(log)
                                .and_then(|view| {
                                    view["timeline"]["clips"].as_array().map(|clips| {
                                        clips.iter().any(|clip| {
                                            clip["media"].as_str() == media_id.as_deref()
                                                && clip["start"].as_f64().is_some_and(|at| at < 3.0)
                                        })
                                    })
                                })
                                .unwrap_or(false);
                            if hook_shot {
                                drift += 0.04;
                            }
                            // The envelope's own reading adjusts the drift:
                            // a shot laid over the take's loud stretch
                            // moves a touch more, over a quiet one a touch
                            // less, at the pace the delivery itself set.
                            if let Some(energy) = pending_energy {
                                drift += (energy - 0.5) * 0.04;
                            }
                            // A wide shot has room to travel; a push reads
                            // on it where the same push on a medium is
                            // barely seen. The scale is the picture's own
                            // reading, not the query's word.
                            if drift_enabled
                                && read.and_then(VisualRead::shot_scale)
                                    == Some(concat_motion::continuity::ShotScale::Wide)
                            {
                                drift += 0.02;
                            }
                            let drift = round3(drift);
                            if drift_enabled {
                                commands.push(json!({
                                    "op": "setClipKey",
                                    "clipId": clip_id,
                                    "property": "scale",
                                    "at": 1.0,
                                    "value": drift,
                                    "ease": [0.0, 0.0, 0.58, 1.0],
                                }));
                            }
                            // The crop hint the sheet measured, spent on
                            // the axis the fill actually crops: the
                            // subject stands where the frame found it,
                            // not where a centre crop guessed it.
                            if let Some((offset_x, offset_y)) = reframe_offsets(
                                &candidate.tile,
                                &aspect,
                                candidate.width,
                                candidate.height,
                            ) {
                                commands.push(json!({
                                    "op": "setClipTransform",
                                    "clipId": clip_id,
                                    "offsetX": round3(offset_x),
                                    "offsetY": round3(offset_y),
                                }));
                            }
                            let chosen = pending_transition.unwrap_or("concat.dissolve");
                            last_transition = Some(chosen);
                            // The blend is a share of the tempo: a third of
                            // the period the take repeats on, held between
                            // a snap and a breath so no measured period can
                            // make a seam that reads as a stall.
                            let blend = pending_period
                                .map(|period| round3((period / 3.0).clamp(0.2, 0.6)))
                                .unwrap_or(0.3);
                            commands.push(json!({
                                "op": "updateClip",
                                "clipId": clip_id,
                                "patch": {
                                    "transitionIn": { "id": chosen, "duration": blend },
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
    //    undoable batch of titles in the reading's house style, then a
    //    second pass that gives the words that carried the delivery a
    //    look of their own.
    //
    //    A caption is not a transcript: the groups are two to four words
    //    long, cut on the delivery's own seams (the recogniser's word
    //    times, the sentence's own punctuation, the breath gaps the
    //    profile measured), so a viewer reads a phrase, not a paragraph.
    //    The style follows the read at each group: a line that reads loud
    //    is set heavier, a hot or split line takes the accent colour on
    //    the word that carried it, and a calm informational line stays
    //    plain - the same words do not all earn the same look.
    //
    //    The frame the project is shapes the band the captions sit in: a
    //    vertical reel keeps its words clear of the platform UI at the
    //    bottom, and a horizontal cut keeps the classic lower third.
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
                let mut phase = 0u8;
                // The groups the batch placed, kept for the accent pass:
                // each carries the timeline span it landed at and the
                // character ranges of its own words.
                let mut placed_groups: Vec<(f64, CaptionGroup)> = Vec::new();
                move |log| {
                    if phase == 2 {
                        return None;
                    }
                    let view = latest_view(log)?;
                    let aspect = aspect_of(view).to_owned();
                    let portrait = matches!(aspect.as_str(), "9:16" | "4:5" | "1:1");
                    let media = media_paths(view);
                    if phase == 0 {
                        let voice = voice_reads(log, &caption_source);
                        let mut groups: Vec<(f64, f64, CaptionGroup)> = Vec::new();
                        for (take, segments) in transcripts_of(log, &caption_source) {
                            let take_read = voice_read_of(&voice, &take);
                            for segment in segments {
                                // The safe-remove fillers the profile timed
                                // for this segment: `um`, `uh` and their kin
                                // are pure noise in a caption - the viewer
                                // hears the pause, not the syllable. The
                                // discourse markers (`like`, `you know`) are
                                // context calls the profile does not
                                // decide, so they stay.
                                let fillers: Vec<String> = take_read
                                    .map(|take_read| {
                                        let start = segment["start"].as_f64().unwrap_or(0.0);
                                        let end = segment["end"].as_f64().unwrap_or(0.0);
                                        take_read
                                            .fillers_between(start, end)
                                            .into_iter()
                                            .map(|filler| filler.word.to_lowercase())
                                            .collect()
                                    })
                                    .unwrap_or_default();
                                for mut group in caption_groups(&segment, &fillers) {
                                    group.take = take.clone();
                                    // The opening bound must land inside a
                                    // surviving piece (a caption whose
                                    // first word was cut has no start to
                                    // show at), but the closing bound is
                                    // allowed to sit at the join the cut
                                    // made: a group whose last word ends
                                    // exactly at the piece's edge - a take
                                    // trimmed to its last word, or a
                                    // trailing pause the tighten took -
                                    // still shows its words, so the bound
                                    // maps to the nearest surviving second
                                    // at or before it, the same mercy the
                                    // B-roll slot bounds already get.
                                    let (Some(at), Some(to)) = (
                                        timeline_at(view, &media, &take, group.start),
                                        timeline_at_or_before(view, &media, &take, group.end),
                                    ) else {
                                        continue;
                                    };
                                    if to - at < 0.25 {
                                        continue;
                                    }
                                    groups.push((at, to, group));
                                }
                            }
                        }
                        if groups.is_empty() {
                            phase = 2;
                            return None;
                        }
                        // Overlaps cannot happen on a clean transcript, but a
                        // merged one can carry them: the later group waits
                        // for the one before it to leave. The walk is by
                        // timeline position, not take order - groups are
                        // built take-major, and an interleaved multi-take
                        // run would otherwise set `until` from take A and
                        // drop every one of take B's groups as "overlaps".
                        groups.sort_by(|left, right| {
                            left.0.total_cmp(&right.0).then(left.1.total_cmp(&right.1))
                        });
                        let mut kept: Vec<(f64, f64, CaptionGroup)> = Vec::new();
                        let mut until = 0.0;
                        for (at, to, group) in groups {
                            if at < until - 0.05 {
                                continue;
                            }
                            until = to;
                            kept.push((at, to, group));
                        }
                        kept.truncate(240);
                        // The script's own font: the profile's language read
                        // names the family a non-Latin script needs, and the
                        // house face is only the fallback when the read did
                        // not name one. A caption set in Hanken Grotesk over
                        // a Devanagari or Cyrillic take renders as boxes.
                        let font = voice
                            .iter()
                            .map(|read| read.take_wide.font.as_str())
                            .find(|font| !font.is_empty())
                            .map(bundled_font)
                            .unwrap_or("Hanken Grotesk");
                        let read = emotion_lines(log, &caption_emotion);
                        let mut commands: Vec<Value> = Vec::new();
                        placed_groups.clear();
                        let next_starts: Vec<f64> = kept.iter().map(|(at, _, _)| *at).collect();
                        for (index, (at, to, group)) in kept.into_iter().enumerate() {
                            let take_of = group.take.clone();
                            // The line the group sits in: its read decides
                            // the group's look, exactly as it decided the
                            // old whole-segment caption's.
                            let line = emotion_at(&read, &take_of, group.start);
                            let intensity = line.map(|line| line.intensity).unwrap_or(0.5);
                            // The word-confidence gate: a group most of
                            // whose words the gate could not hear is not a
                            // group to set heavy or accent - it is a group
                            // to show plainly, so a misheard word does not
                            // become the caption's loudest claim.
                            let take_read = voice_read_of(&voice, &take_of);
                            let unsure_share = take_read
                                .map(|take_read| {
                                    let flagged =
                                        take_read.unsure_between(group.start, group.end).len();
                                    let total = group.words.len();
                                    if total == 0 {
                                        0.0
                                    } else {
                                        flagged as f64 / total as f64
                                    }
                                })
                                .unwrap_or(0.0);
                            let gated = unsure_share > 0.5
                                // A line with barely any voice under it is a
                                // line the transcript probably misaligned:
                                // the voiced ratio is the ear's own account
                                // of how much speech it found, and under a
                                // fifth it sides with the gate rather than
                                // the words.
                                || take_read
                                    .and_then(|take_read| take_read.line_at(group.start))
                                    .and_then(|line| line.voiced_ratio)
                                    .is_some_and(|ratio| ratio < 0.2);
                            // A line delivered above the take's own
                            // integrated level is a line the speaker
                            // lifted, so the caption carries weight even
                            // when the mood is mild. This is the one place
                            // the profile's per-line `relativeLufs` reaches
                            // the picture. The contour's own hook lift is
                            // its pitch twin: a delivery that rose two
                            // semitones or more across the opening three
                            // seconds performed the hook whatever the level
                            // meter read, and the hook's captions carry the
                            // weight the rise earned.
                            let lifted = take_read.is_some_and(|take_read| {
                                take_read.line_at(group.start).is_some_and(|line| {
                                    // 1.5 LU is the profile's own
                                    // definition of a lifted line.
                                    take_read.line_lift(line).is_some_and(|lu| lu >= 1.5)
                                }) || (at < 3.0
                                    && take_read.hook_lift().is_some_and(|lift| lift >= 2.0))
                            });
                            let performed = take_read.and_then(|take_read| {
                                take_read
                                    .line_at(group.start)
                                    .and_then(|line| take_read.performs(line))
                            });
                            // The pitch twin of the level lift: a line the
                            // speaker raised over the take's own register
                            // is a line the delivery performed, even when
                            // the loudness meter saw no change.
                            let register = take_read
                                .and_then(|take_read| {
                                    take_read
                                        .line_at(group.start)
                                        .and_then(|line| take_read.register_lift(line))
                                })
                                .is_some_and(|lift| lift >= 2.0);
                            // A fast line is read at a glance, so its
                            // caption is set a touch smaller; a slow line
                            // can carry a larger one.
                            let mut size = match line.map(|line| line.pace.as_str()) {
                                Some("punchy") => 0.036,
                                Some("slow") => 0.046,
                                _ => 0.04,
                            };
                            // The sentence's own shape is a caption cue:
                            // an exclamation carries, a fragment is quiet.
                            match take_read
                                .and_then(|take_read| take_read.line_at(group.start))
                                .map(|line| line.sentence_type.as_str())
                            {
                                Some("exclamation") => size += 0.002,
                                Some("fragment") => size -= 0.002,
                                _ => {}
                            }
                            // A clean voice carries its own weight: a
                            // line the forensic ear read as stable (an
                            // HNR at 10 dB or better, a jitter under 5%
                            // and a shimmer under 6%) keeps the heavier
                            // setting its performance earned, while a
                            // hoarse line settles to the plain setting -
                            // weight on an unstable voice is emphasis the
                            // delivery cannot hold. `None` (no
                            // voice-quality reading) changes nothing.
                            let steady = take_read.and_then(|take_read| {
                                take_read
                                    .line_at(group.start)
                                    .and_then(|line| take_read.clean_voice(line))
                            });
                            if performed == Some(true) && !gated && steady != Some(false) {
                                size += 0.003;
                            }
                            // Two-factor emphasis, the catalogue's own
                            // pairing: a line BOTH lifted (its level over
                            // the take's integrated one) and performed (a
                            // six-semitone range) is the delivery's own
                            // highlight - the voice raised it and performed
                            // it, so the caption carries one more notch of
                            // size. One factor alone keeps its single arm
                            // above: a lifted line that reports and a
                            // performed line that settles each stay at the
                            // one-notch bump.
                            if lifted && performed == Some(true) && !gated && steady != Some(false)
                            {
                                size += 0.003;
                            }
                            if register && !gated && steady != Some(false) {
                                size += 0.002;
                            }
                            // The vertical band is narrower and sits higher
                            // than the horizontal one: the words stay clear
                            // of the platform UI a reel carries at the
                            // bottom, and a touch larger because the frame
                            // they sit in is narrower.
                            let mut offset_y = if portrait { 0.22 } else { 0.35 };
                            // The words move away from where the eye already
                            // is: a subject in the lower third lifts the
                            // band, one in the upper third lowers it, and a
                            // moment no measured clip covers keeps the band
                            // the aspect gave it.
                            match visual_focus_at(log, at).as_deref() {
                                Some("lower") => offset_y = if portrait { 0.10 } else { 0.16 },
                                Some("upper") => offset_y = if portrait { 0.34 } else { 0.48 },
                                _ => {}
                            }
                            if portrait {
                                size *= 1.12;
                            }
                            // The hook is the only part of a short cut a
                            // viewer is guaranteed to see: its words are a
                            // notch bigger, whatever the mood.
                            if at < 3.0 {
                                size += 0.006;
                            }
                            // The reading rate sets a floor on the hold:
                            // a line of long words needs longer on screen
                            // than its spoken span, but the extension is
                            // capped so it never runs into the next line.
                            let mut duration = to - at;
                            if let Some(hold) = take_read.and_then(|take_read| {
                                take_read
                                    .line_at(group.start)
                                    .and_then(|line| take_read.caption_hold(line))
                            }) {
                                duration = duration.max(hold.min(duration + 0.4));
                            }
                            // Never run into the next caption: the hold's
                            // extension is capped by the next group's own
                            // start when one is already placed.
                            if let Some(next_at) = next_starts.get(index + 1).copied()
                                && next_at > to
                            {
                                duration = duration.min(next_at - at);
                            }
                            commands.push(json!({
                                "op": "addTextClip",
                                "trackId": null,
                                "above": true,
                                "start": round3(at),
                                "duration": round3(duration),
                                "style": {
                                    "fontFamily": font,
                                    "fontSize": round3(size),
                                    "fontWeight": if (intensity >= 0.65 || lifted)
                                        && !gated
                                        && steady != Some(false)
                                    {
                                        800
                                    } else if performed == Some(true)
                                        && !gated
                                        && steady != Some(false)
                                    {
                                        700
                                    } else {
                                        600
                                    },
                                    "content": group.text,
                                },
                                "offsetY": offset_y,
                            }));
                            placed_groups.push((at, group));
                        }
                        if commands.is_empty() {
                            phase = 2;
                            return None;
                        }
                        phase = 1;
                        return Some(ToolCall {
                            tool: "edit_apply".to_owned(),
                            arguments: json!({
                                "path": caption_project,
                                "command": { "op": "batch", "commands": commands },
                            }),
                        });
                    }
                    // The accent pass: the batch's own reply is a view
                    // carrying the titles just placed, in order. The words
                    // the delivery actually stressed - or, on a hot line
                    // with no stressed word, the group's own longest word -
                    // take the accent colour and a touch of scale, one
                    // patch per word, through the title's own range
                    // styling.
                    let view = latest_view(log)?;
                    let mut titles: Vec<(f64, String)> = view["timeline"]["clips"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|clip| clip["kind"] == json!("text"))
                        .filter_map(|clip| {
                            Some((clip["start"].as_f64()?, clip["id"].as_str()?.to_owned()))
                        })
                        .collect();
                    titles.sort_by(|left, right| left.0.total_cmp(&right.0));
                    let emphasis = stressed_words(log, &caption_source);
                    let read = emotion_lines(log, &caption_emotion);
                    let voice = voice_reads(log, &caption_source);
                    let mut commands: Vec<Value> = Vec::new();
                    for (at, group) in &placed_groups {
                        let Some((_, clip_id)) =
                            titles.iter().find(|(start, _)| (start - at).abs() < 0.1)
                        else {
                            continue;
                        };
                        let line = emotion_at(&read, &group.take, group.start);
                        let take_read = voice_read_of(&voice, &group.take);
                        let unsure_share = take_read
                            .map(|take_read| {
                                let flagged =
                                    take_read.unsure_between(group.start, group.end).len();
                                let total = group.words.len();
                                if total == 0 {
                                    0.0
                                } else {
                                    flagged as f64 / total as f64
                                }
                            })
                            .unwrap_or(0.0);
                        if unsure_share > 0.5 {
                            continue;
                        }
                        let hot = line.is_some_and(|line| {
                            matches!(line.temperature.as_str(), "hot" | "split")
                        });
                        // The words the profile's own emphasis read named:
                        // the delivery's, not the mood's - and the take's
                        // own delivery, never another take's.
                        let take_emphasis = emphasis
                            .iter()
                            .find(|(take, _)| *take == group.take)
                            .map(|(_, words)| words);
                        let mut accents: Vec<&CaptionWord> = group
                            .words
                            .iter()
                            .filter(|word| {
                                let bare = word
                                    .text
                                    .trim_matches(|c: char| !c.is_alphanumeric())
                                    .to_lowercase();
                                !bare.is_empty()
                                    && take_emphasis.is_some_and(|words| words.contains(&bare))
                            })
                            .collect();
                        // A hot line with no stressed word still earns its
                        // accent: the group's own longest word is the one
                        // carrying the claim.
                        if accents.is_empty() && hot {
                            let longest = group
                                .words
                                .iter()
                                .max_by_key(|word| word.text.chars().count());
                            if let Some(longest) = longest {
                                accents.push(longest);
                            }
                        }
                        for word in accents {
                            let chars = word.text.chars().count();
                            if chars == 0 || word.at + chars > group.text.chars().count() {
                                continue;
                            }
                            commands.push(json!({
                                "op": "styleTextRange",
                                "clipId": clip_id,
                                "start": word.at,
                                "end": word.at + chars,
                                "patch": {
                                    "color": "#ffd400",
                                    "fontScale": 1.18,
                                },
                            }));
                        }
                    }
                    phase = 2;
                    if commands.is_empty() {
                        return None;
                    }
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
                // The preset library's own shape, read once: the first
                // text slot (a preset's text slot is not called the same
                // thing in every card, and guessing the label is how a
                // bake is refused), every slot label the preset carries
                // (the bake fills or blanks each one), and the comp's own
                // duration (a hold longer than the comp freezes its last
                // frame).
                let mut labels: Option<Vec<MotionPresetShape>> = None;
                // Where the stage is: which card, and which of its calls
                // (the library, the bake, then the placement) is next.
                let mut at = 0usize;
                let mut phase = 0u8;
                let mut entrances = false;
                move |log| loop {
                    // The captions' entrance keys land FIRST: under the
                    // warn policy one failed bake used to abort the whole
                    // derive loop and take the entrance pass with it - a
                    // machine without the Geneva sidecar lost every card
                    // AND every caption entrance in one refusal.
                    if !entrances {
                        entrances = true;
                        let targets = entrance_targets(log, &motion_source);
                        if !targets.is_empty() {
                            let mut commands: Vec<Value> = Vec::new();
                            for (id, duration, placed, ease) in &targets {
                                commands.extend(entrance_keys(id, *duration, *placed, ease));
                            }
                            return Some(call(
                                "edit_apply",
                                json!({
                                    "path": motion_project,
                                    "command": { "op": "batch", "commands": commands },
                                }),
                            ));
                        }
                    }
                    let specs = specs.get_or_insert_with(|| {
                        let reads = voice_reads(log, &motion_source);
                        let motifs: Vec<Motif> = reads
                            .iter()
                            .flat_map(|read| read.motifs.iter().cloned())
                            .collect();
                        motion_specs(
                            log,
                            &emotion_lines(log, &motion_source),
                            &reads,
                            &entity_graphics(log, &motion_source),
                            &topic_boundaries(log, &motion_source),
                            &motifs,
                            &question_acts(log, &motion_source),
                            &uncovered_claims(log, &motion_source),
                            // Captions land before motion does: a timeline
                            // that already carries text clips answers the
                            // question-card collision by not asking it.
                            latest_view(log).is_some_and(|view| {
                                view["timeline"]["clips"].as_array().is_some_and(|clips| {
                                    clips.iter().any(|clip| clip["kind"] == json!("text"))
                                })
                            }),
                        )
                    });
                    if at >= specs.len() {
                        // The cards are placed and the entrances were keyed
                        // before the first bake: the stage is done.
                        return None;
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
                            // A card that names its own slot uses it; the
                            // rest take the preset's first text slot, which
                            // is what the library's own labels say.
                            let shape = labels.as_ref().and_then(|labels| {
                                labels.iter().find(|(id, ..)| id == spec.preset)
                            });
                            let chosen = spec
                                .slot
                                .map(str::to_owned)
                                .or_else(|| shape.map(|(_, slot, ..)| slot.clone()));
                            if let Some((_, _, all, _)) = shape {
                                // One op per slot the preset carries: the
                                // extra fills (a chapter's Number), the
                                // words in their own slot, and a BLANK
                                // everywhere else - the sample text never
                                // ships as a fact the video never said.
                                let ops: Vec<Value> = all
                                    .iter()
                                    .map(|label| {
                                        let value = spec
                                            .slots
                                            .iter()
                                            .find(|(fill, _)| fill == label)
                                            .map(|(_, value)| value.clone())
                                            .or_else(|| {
                                                (chosen.as_deref() == Some(label.as_str()))
                                                    .then(|| spec.words.clone())
                                            })
                                            .unwrap_or_default();
                                        json!({
                                            "op": "set-slot",
                                            "slot": label,
                                            "value": value,
                                        })
                                    })
                                    .collect();
                                arguments["ops"] = json!(ops);
                            } else if let Some(slot) = chosen {
                                arguments["ops"] = json!([{
                                    "op": "set-slot", "slot": slot, "value": spec.words,
                                }]);
                            }
                            return Some(call("motion_bake", arguments));
                        }
                        1 => {
                            // The library's reply, read once and kept: the
                            // first text slot, every label, and the
                            // duration of each preset, by id. A reply that
                            // carries no preset at all is a refusal, not a
                            // library: baking on with no shapes would ship
                            // the presets' own sample text as fabricated
                            // facts - the one thing the BLANK rule exists
                            // to prevent - so no card is placed and the
                            // stage's warn policy records the skipped work.
                            let shapes: Vec<MotionPresetShape> = log
                                .last()
                                .and_then(|reply| reply["presets"].as_array())
                                .into_iter()
                                .flatten()
                                .filter_map(|preset| {
                                    let id = preset["id"].as_str()?.to_owned();
                                    let slots = preset["slots"].as_array()?;
                                    let slot = slots
                                        .iter()
                                        .find(|slot| slot["field"] == json!("text"))?["label"]
                                        .as_str()?
                                        .to_owned();
                                    let all = slots
                                        .iter()
                                        .filter_map(|slot| {
                                            slot["label"].as_str().map(str::to_owned)
                                        })
                                        .collect();
                                    let duration = preset["duration"].as_f64()?;
                                    Some((id, slot, all, duration))
                                })
                                .collect();
                            if shapes.is_empty() {
                                return None;
                            }
                            labels = Some(shapes);
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
                                    // path, which the command refuses. The
                                    // engine rides along when the bake said
                                    // one: the clip records the engine that
                                    // drew its frames, so a native bake is
                                    // never recorded as geneva's pixels.
                                    let mut command = json!({
                                        "op": "addMotionClip",
                                        "start": round3(start),
                                        "comp": format!("{hash}.json"),
                                        "hash": hash,
                                    });
                                    if let Some(engine) =
                                        baked["engine"].as_str().filter(|engine| {
                                            matches!(*engine, "geneva" | "native" | "lottie")
                                        })
                                    {
                                        command["engine"] = json!(engine);
                                    }
                                    return Some(call(
                                        "edit_apply",
                                        json!({
                                            "path": motion_project,
                                            "command": command,
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
                                    // A hold longer than the comp's own
                                    // duration freezes the last frame; the
                                    // comp's duration is the ceiling.
                                    let comp = labels
                                        .as_ref()
                                        .and_then(|labels| {
                                            labels.iter().find(|(id, ..)| id == spec.preset)
                                        })
                                        .map(|(.., duration)| *duration)
                                        .unwrap_or(f64::MAX);
                                    let hold = spec.hold().min(comp);
                                    let delta = hold - duration;
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
                                            "edge": "end",
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
                // Whether the generator was asked for a bed: one ask a run,
                // then the shelf if it answered nothing.
                let mut ai_bed = false;
                // Whether the licence-clean pad was asked for, after the
                // music model refused a monetized project: one ask, then
                // the shelf. TinyMusician's weights are CC-BY-NC, so the
                // plan's own monetized brief rules that bed out; the pad
                // is Stable Audio's, which the community licence allows.
                let mut pad_bed = false;
                // How many replies the run had drawn before this stage's
                // first call: everything before that index belongs to the
                // earlier stages, and the stage reads only what it drew
                // itself. The captions stage leaves a timeline view behind,
                // and reading it as this stage's own is how the bed was
                // never shopped.
                let mut base: Option<usize> = None;
                // Where the mood-query chain stands: each door is walked
                // only when the one before it answered nothing.
                let mut query_cursor = 0usize;
                // The bed is ducked once; the duck's own reply is another
                // timeline view, and the stage must not duck it again.
                let mut ducked = false;
                // The duck's own shape: after the base level lands, one
                // batch of volume keys follows the voice's own envelope -
                // down under every line, back up in the gaps that can
                // hold it - and the stage ends when it has.
                let mut shaped = false;
                // The bed the duck landed on: its clip id, start and
                // duration, kept for the shape's key positions.
                let mut bed_span: Option<(String, f64, f64)> = None;
                // The base level the duck rides at, as the linear gain the
                // keys carry: volume keys are absolute, so a shape keyed
                // at unity would discard the base the levels call just
                // set and blast the bed at full source level.
                let mut bed_level: Option<f64> = None;
                // The cut's own length, measured before the bed was laid:
                // a bed longer than the cut is trimmed to it, or the
                // export gains a black, scored tail.
                let mut bed_runtime: Option<f64> = None;
                move |log| {
                    // The gate's own answer first: music already in the
                    // take means no bed, ever. The reading's own forensic
                    // check is the second door - a take whose noise floor
                    // says it is already wet, or whose music gate found
                    // segments, gets no bed stacked under it either. Both
                    // answers come from measurement; a run that read
                    // neither still shops a bed, because "not measured"
                    // is not "already scored".
                    if log
                        .iter()
                        .any(|reply| reply.get("musicPresent") == Some(&json!(true)))
                    {
                        return None;
                    }
                    let reads = voice_reads(log, &music_source);
                    if reads
                        .iter()
                        .any(|read| read.take_wide.bgm_decision == Some("skip".to_owned()))
                    {
                        return None;
                    }
                    // The floor's own answer: a take measured above the
                    // dry threshold already carries room tone or a score,
                    // and a bed stacked on it doubles the bed. The log
                    // claimed this gate and the stage never read it, so a
                    // wet take was shopped a bed it did not want.
                    if reads.iter().any(|read| read.wants_bed() == Some(false)) {
                        return None;
                    }
                    // The profile's own verdict, when it reached one: its
                    // BGM decision weighs the floor, the SNR and the
                    // spectral flatness together, which is a judgement no
                    // single field here can repeat. `skip` means the take
                    // already carries music-like content; `duck_only` means
                    // the voice needs cleaning before anything sits under
                    // it, so bedding now would stack a bed on a noisy take.
                    if reads.iter().any(|read| {
                        matches!(
                            read.take_wide.bgm_decision.as_deref(),
                            Some("skip") | Some("duck_only")
                        )
                    }) {
                        return None;
                    }
                    // The forensic ear's own second opinion: dense, regular
                    // onsets that survive the pure-tone veto are a beat,
                    // and a bed stacked on a beat is the doubling the skip
                    // rule exists to stop - even when the floor read dry
                    // and the profile's own verdict said add.
                    if reads
                        .iter()
                        .any(|read| read.take_wide.music_likelihood.is_some_and(|l| l >= 0.6))
                    {
                        return None;
                    }
                    if ducked {
                        // The duck's own shape, once: the voice's own
                        // lines, mapped onto the timeline the bed sits
                        // in, become volume keys - the bed drops under
                        // the words and lifts back in the gaps that can
                        // hold it, which is what a mix engineer rides by
                        // hand and what one static level never was.
                        if shaped {
                            return None;
                        }
                        shaped = true;
                        let (bed_id, bed_start, bed_len) = bed_span.clone()?;
                        let view = latest_view(log)?;
                        let media = media_paths(view);
                        // The lines the voice measured, as timeline
                        // spans, merged where they run together: a duck
                        // that lifts between two words of one breath is
                        // a stutter, not a shape.
                        let mut spans: Vec<(f64, f64)> = Vec::new();
                        for read in voice_reads(log, &music_source) {
                            for line in &read.lines {
                                if let (Some(at), Some(to)) = (
                                    timeline_at(view, &media, &read.take, line.start),
                                    timeline_at(view, &media, &read.take, line.end),
                                ) && to > at
                                {
                                    spans.push((at, to));
                                }
                            }
                        }
                        spans.sort_by(|left, right| {
                            left.0.total_cmp(&right.0).then(left.1.total_cmp(&right.1))
                        });
                        let mut merged: Vec<(f64, f64)> = Vec::new();
                        for (at, to) in spans {
                            match merged.last_mut() {
                                Some((_, end)) if at <= *end + 0.35 => {
                                    *end = (*end).max(to);
                                }
                                _ => merged.push((at, to)),
                            }
                        }
                        const DUCK: f64 = 0.45;
                        const MOST_KEYS: usize = 28;
                        // Volume keys are absolute linear values, so the
                        // shape rides at the base the levels call set:
                        // unity keys would cancel that base and the bed
                        // would play at its source's full level. The duck
                        // is the base dropped by the shape's own share.
                        let lift = bed_level.unwrap_or(0.1);
                        let duck = lift * DUCK;
                        let mut commands: Vec<Value> = vec![json!({
                            "op": "setClipKey", "clipId": bed_id,
                            "property": "volume", "at": 0.0, "value": round3(lift),
                        })];
                        let at_fraction =
                            |moment: f64| ((moment - bed_start) / bed_len.max(0.1)).clamp(0.0, 1.0);
                        for (at, to) in &merged {
                            if commands.len() >= MOST_KEYS {
                                break;
                            }
                            let duck_at = at_fraction(*at);
                            let lift_at = at_fraction(to + 0.2);
                            if duck_at > 0.0 && duck_at < 1.0 {
                                commands.push(json!({
                                    "op": "setClipKey", "clipId": bed_id,
                                    "property": "volume",
                                    "at": round3(duck_at), "value": round3(duck),
                                }));
                            }
                            // Lift only where the gap can hold it: the
                            // next line more than half a second away, or
                            // the bed's own end.
                            let next = merged
                                .iter()
                                .find(|(next_at, _)| *next_at > to + 0.05)
                                .map(|(next_at, _)| *next_at);
                            if lift_at < 1.0 && next.is_none_or(|next| next > to + 0.6) {
                                commands.push(json!({
                                    "op": "setClipKey", "clipId": bed_id,
                                    "property": "volume",
                                    "at": round3(lift_at), "value": round3(lift),
                                }));
                            }
                        }
                        if commands.len() < 2 {
                            return None;
                        }
                        return Some(call(
                            "edit_apply",
                            json!({
                                "path": music_project,
                                "command": { "op": "batch", "commands": commands },
                            }),
                        ));
                    }
                    let base = *base.get_or_insert(log.len());
                    if log.len() == base {
                        // A bed the plan stage already landed - shopped
                        // early so its beats could drive the planner - is
                        // this stage's bed: no second search, no second
                        // import. The timeline is read back and the
                        // existing-bed path below trims and ducks it.
                        let bed_already = log.iter().any(|reply| {
                            reply.pointer("/timeline/clips").is_some_and(|clips| {
                                let media = media_paths(reply);
                                clips.as_array().is_some_and(|rows| {
                                    rows.iter().any(|clip| {
                                        clip["media"]
                                            .as_str()
                                            .and_then(|id| media.iter().find(|(mid, _)| mid == id))
                                            .is_some_and(|(_, path)| path.contains("/bgm/"))
                                    })
                                })
                            })
                        });
                        if bed_already {
                            return Some(call("timeline_read", json!({ "path": music_project })));
                        }
                        // The generator first, for a cut short enough that
                        // one written bed covers it: this decoder has no
                        // usable cache, so its cost is the square of the
                        // length and eight seconds of bed is already most
                        // of the call's budget. A longer cut keeps the
                        // shelf, which carries tracks of every length.
                        let runtime = latest_view(log)
                            .and_then(|view| {
                                view["timeline"]["clips"].as_array().map(|clips| {
                                    clips
                                        .iter()
                                        .map(|clip| {
                                            clip["start"].as_f64().unwrap_or(0.0)
                                                + clip["duration"].as_f64().unwrap_or(0.0)
                                        })
                                        .fold(0.0_f64, f64::max)
                                })
                            })
                            .unwrap_or(0.0);
                        if !ai_bed && runtime > 0.5 && runtime <= 8.0 {
                            ai_bed = true;
                            return Some(call(
                                "sense_sound_model",
                                json!({
                                    "action": "generate",
                                    "kind": "bgm",
                                    "prompt": bed_prompt(log, &music_source),
                                    "seconds": round3(runtime.min(8.0)),
                                    "project": music_project,
                                    // The plan this run wrote says
                                    // monetized, and a run must not ship
                                    // a bed its own plan would refuse:
                                    // the music model answers with the
                                    // licence instead of a file, and the
                                    // pad below is written instead.
                                    "monetized": true,
                                }),
                            ));
                        }
                        // The stage's own first call: the shelf's state.
                        return Some(call("bgm_status", json!({})));
                    }
                    // The log only grows past the base this stage drew,
                    // but a future call shape must not take the runner
                    // down with it for an invariant it cannot see.
                    let last = log.last()?;
                    // The generator's own reply, success or refusal: the
                    // sidecar stamps every one of them with the tool it
                    // answered, so a model that is not installed is read
                    // as this stage's answer and not as somebody else's.
                    if last.get("tool") == Some(&json!("generate")) {
                        // The generator's reply: a written bed is imported
                        // into the project's own `bgm` folder, which is
                        // where the timeline read below finds it. A model
                        // that is not installed sends the stage to the
                        // shelf, and every later reply is the shelf's.
                        if last.get("ok") == Some(&json!(true))
                            && let Some(path) = last.get("path").and_then(|path| path.as_str())
                        {
                            return Some(call(
                                "bgm_import",
                                json!({
                                    "path": music_project,
                                    "file": path,
                                    "start": 0.0,
                                }),
                            ));
                        }
                        // The music model refused on licence: this project
                        // is monetized and TinyMusician's weights are
                        // CC-BY-NC. A bed is still wanted, and Stable
                        // Audio's community licence allows one, so the
                        // same words are asked of it as a texture - no
                        // drums, so it sits under a voice instead of
                        // fighting it.
                        let runtime = latest_view(log)
                            .and_then(|view| {
                                view["timeline"]["clips"].as_array().map(|clips| {
                                    clips
                                        .iter()
                                        .map(|clip| {
                                            clip["start"].as_f64().unwrap_or(0.0)
                                                + clip["duration"].as_f64().unwrap_or(0.0)
                                        })
                                        .fold(0.0_f64, f64::max)
                                })
                            })
                            .unwrap_or(0.0);
                        if last.get("reason") == Some(&json!("licence"))
                            && !pad_bed
                            && runtime > 0.5
                        {
                            pad_bed = true;
                            return Some(call(
                                "sense_sound_model",
                                json!({
                                    "action": "generate",
                                    "kind": "sfx-large",
                                    "prompt": format!(
                                        "warm ambient pad, {}, no drums, no melody",
                                        bed_prompt(log, &music_source)
                                    ),
                                    "seconds": round3(runtime.min(11.5)),
                                    "project": music_project,
                                }),
                            ));
                        }
                        return Some(call("bgm_status", json!({})));
                    }
                    if last.get("freepd").is_some() {
                        // A bgm_status reply: prepare the missing shelf,
                        // poll the loading one, shop the ready one.
                        return match last["freepd"].as_str() {
                            Some("ready") => {
                                // The bed's words: the emotional read's
                                // own, from the piece's dominant line -
                                // the mood and the act name the genre -
                                // then the reading's own language hint,
                                // which the profile derived from the
                                // script the narration is written in.
                                // Each door is walked only when the one
                                // before it answered nothing: the read's
                                // three-word primaries usually match no
                                // whole title, and the bare category is
                                // what the shelf's folders actually
                                // answer. The chain's last door is the
                                // browse - a ready shelf is never left
                                // unheard because three mood words
                                // matched nothing.
                                let query = bed_query_chain(log, &music_source)
                                    .get(query_cursor)
                                    .cloned()
                                    .flatten();
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
                            // The shelf's status word is "loading (N%)",
                            // not "loading": the prefix is what a poll
                            // must match, or the stage ends the moment a
                            // catalog load starts and the run ships with
                            // no bed on every cold machine.
                            Some(word) if word.starts_with("loading") => {
                                polls += 1;
                                if polls > 200 {
                                    return None;
                                }
                                // The shelf's one-time fetch is real work:
                                // the poll waits between looks.
                                std::thread::sleep(Duration::from_millis(3000));
                                Some(call("bgm_status", json!({})))
                            }
                            // A failed shelf is an outage the run must
                            // say, not one it shops around in silence:
                            // the search on a failed shelf is refused
                            // with the shelf's own reason, which the warn
                            // policy records as the run's warning.
                            Some(word) if word.starts_with("failed") => Some(call(
                                "bgm_search",
                                json!({
                                    "source": "freepd",
                                    "limit": 5,
                                    "sort": "relevance",
                                }),
                            )),
                            _ => None,
                        };
                    }
                    if last.get("tracks").is_some() {
                        // A bgm_search reply: the bed is the track that
                        // actually fits the cut, not the shelf's first
                        // answer. A track shorter than the runtime leaves
                        // the ending silent; among the ones that cover,
                        // the shortest wastes the least; when none
                        // covers, the longest covers the most. A track
                        // whose duration the census could not measure is
                        // not guessed at.
                        let runtime = latest_view(log)
                            .and_then(|view| {
                                view["timeline"]["clips"].as_array().map(|clips| {
                                    clips
                                        .iter()
                                        .map(|clip| {
                                            clip["start"].as_f64().unwrap_or(0.0)
                                                + clip["duration"].as_f64().unwrap_or(0.0)
                                        })
                                        .fold(0.0_f64, f64::max)
                                })
                            })
                            .unwrap_or(0.0);
                        let tracks = last["tracks"].as_array().cloned().unwrap_or_default();
                        if tracks.is_empty() {
                            // The word matched nothing: the chain's next
                            // door, and the browse after the last of them
                            // - an empty answer is a door to walk past,
                            // not a cut scored in silence.
                            let chain = bed_query_chain(log, &music_source);
                            query_cursor += 1;
                            if query_cursor >= chain.len() {
                                return None;
                            }
                            let query = chain.get(query_cursor).cloned().flatten();
                            let mut arguments = json!({
                                "source": "freepd",
                                "limit": 5,
                                "sort": "relevance",
                            });
                            if let Some(query) = query {
                                arguments["query"] = json!(query);
                            }
                            return Some(call("bgm_search", arguments));
                        }
                        bed_runtime = Some(runtime);
                        let id = best_bed(&tracks, runtime)
                            .and_then(|track| track["id"].as_str().map(str::to_owned))
                            .or_else(|| {
                                last.pointer("/tracks/0/id")
                                    .and_then(|id| id.as_str())
                                    .map(str::to_owned)
                            });
                        return id.map(|id| {
                            call(
                                "bgm_import",
                                json!({
                                    "path": music_project,
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
                        let bed = bed?;
                        // A bed longer than the cut is trimmed to it: the
                        // export's length is the longest clip's end, so a
                        // three-minute bed under a thirty-second cut would
                        // ship a black, scored tail. The trim lands before
                        // the level so the shape's key positions are
                        // computed on the bed the cut actually keeps, and
                        // the trim's own view is what the next pass reads
                        // the trimmed duration from.
                        let bed_start = bed["start"].as_f64().unwrap_or(0.0);
                        let bed_duration = bed["duration"].as_f64().unwrap_or(0.0);
                        // A bed the plan landed (not this stage) has no
                        // shopped runtime to trim to, and the tighten has
                        // since shortened the cut under it: the trim is
                        // sized by the narration the timeline actually
                        // plays now - the latest view's own longest
                        // non-bed end - never the pre-tighten length the
                        // bed was first cut to, or the export ships the
                        // bed's scored tail past the words' last second.
                        let runtime = bed_runtime.unwrap_or_else(|| narration_runtime(log));
                        if runtime > 0.5 && bed_duration > runtime + 0.05 {
                            return Some(call(
                                "edit_apply",
                                json!({
                                    "path": music_project,
                                    "command": {
                                        "op": "trimClip",
                                        "clipId": bed["id"],
                                        "edge": "end",
                                        "delta": round3(runtime - bed_duration),
                                    },
                                }),
                            ));
                        }
                        ducked = true;
                        bed_span = Some((
                            bed["id"].as_str().unwrap_or_default().to_owned(),
                            bed_start,
                            bed_duration,
                        ));
                        // The bed sits at the level the piece's own
                        // act profile sets: a payoff may carry a
                        // louder bed than a quiet setup, and the
                        // emotional read is what knows which it is.
                        // The measured speech ratio moves it from
                        // there: a take that is almost all speech has
                        // no room for a bed at the act's level, and a
                        // sparse one would lose the bed entirely, so
                        // the duck deepens as the speech fills the
                        // timeline. Unmeasured leaves the act's level
                        // alone - a run must not invent a density.
                        let act_db = dominant(&emotion_lines(log, &music_source))
                            .map(|line| line.duck_db)
                            .unwrap_or(-20.0);
                        let density_db = voice_reads(log, &music_source)
                            .iter()
                            .filter_map(|read| {
                                // Speech ratio and voicing rate are two
                                // ears on the same question - how much of
                                // the take is voice - so the denser of the
                                // two decides the duck. A take with either
                                // reading is measured; a take with neither
                                // keeps the act's level.
                                read.take_wide
                                    .speech_ratio
                                    .into_iter()
                                    .chain(read.take_wide.voicing_rate)
                                    .reduce(f64::max)
                            })
                            .fold(None::<f64>, |worst, ratio| {
                                Some(worst.map_or(ratio, |worst: f64| worst.max(ratio)))
                            })
                            .map(|ratio| -6.0 * ratio)
                            .unwrap_or(0.0);
                        // A conversation leaves less room for a bed than a
                        // monologue: the speakers' own shares are the
                        // reading that says the take is a dialogue, and
                        // the bed drops another 2 dB under it.
                        let conversation_db = if voice_reads(log, &music_source)
                            .iter()
                            .any(|read| read.conversation() == Some(true))
                        {
                            -2.0
                        } else {
                            0.0
                        };
                        // The room and the band: a reverberant tail
                        // smears the voice over whatever sits under
                        // it, and a sibilant voice lives in the very
                        // band a bed's own brightness occupies - each
                        // deepens the duck a little, measured, not
                        // guessed. A take with neither reading keeps
                        // the act-and-density level alone.
                        let room_db = if voice_reads(log, &music_source)
                            .iter()
                            .filter_map(|read| read.take_wide.reverb_tail_sec)
                            .any(|tail| tail >= 0.4)
                        {
                            -3.0
                        } else {
                            0.0
                        };
                        let band_db = if voice_reads(log, &music_source)
                            .iter()
                            .filter_map(|read| read.take_wide.sibilance_ratio)
                            .any(|share| share >= 0.15)
                        {
                            -2.0
                        } else {
                            0.0
                        };
                        let base_db = round3(
                            (act_db + density_db + conversation_db + room_db + band_db)
                                .clamp(-30.0, -6.0),
                        );
                        // The shape's keys ride at this base: kept as
                        // the linear gain the volume keys carry.
                        bed_level = Some(f64::powf(10.0, base_db / 20.0));
                        return Some(call(
                            "clip_audio_levels_db",
                            json!({
                                "path": music_project,
                                "clipId": bed["id"],
                                "baseDb": base_db,
                                "replaceAutomation": true,
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
    /// Where the accent shopping stands: the reply each state reads is its
    /// own, so a stage never has to guess from a reply's shape what it was
    /// waiting for.
    enum SfxPending {
        /// The generator's reply is in: place the file it wrote, or fall
        /// back to the shelf when the model is not installed.
        Generate {
            /// The peak's timeline moment, already resolved.
            at: f64,
            /// The level the accent lands at, in dB.
            gain_db: f64,
            /// The family the peak's emotion named, for the fallback.
            queries: Vec<&'static str>,
            /// How many of those words have already come back spent.
            tried: usize,
        },
        /// The search's reply is in: pick a hit this run has not spent, or
        /// walk the family's next word.
        Pick {
            /// The peak's timeline moment, already resolved.
            at: f64,
            /// The level the accent lands at, in dB: the strength's own
            /// bucket, already adjusted for the take's SNR - resolved
            /// here because the take is in hand where the peak was
            /// picked, not where the reply comes back.
            gain_db: f64,
            /// The family the peak's emotion named, walked word by word.
            queries: Vec<&'static str>,
            /// How many of those words have already come back spent.
            tried: usize,
            /// The exact shelf word whose results this state is reading.
            query: &'static str,
        },
        /// The add's reply is in: the clip is on the lane. The placement
        /// is verified against the file the timeline will actually play
        /// before the accent is gained - the manifest's `syncAt` is
        /// metadata, and the peak is where the file says it is.
        Add {
            /// The peak's timeline moment, already resolved.
            at: f64,
            /// The level the accent lands at, in dB.
            gain_db: f64,
            /// Where the clip was laid: what a correction moves from.
            start: f64,
            /// The sound's identity - a shelf id or a written file's path -
            /// spent for anti-repetition.
            id: String,
            /// The family the peak's emotion named, for a re-shop when
            /// the check drops what was placed.
            queries: Vec<&'static str>,
            /// How many of those words have already come back spent.
            tried: usize,
            /// The exact shelf word that selected this file.
            query: &'static str,
        },
        /// The check's reply is in: move the clip onto the moment, drop
        /// it as the wrong sound, or let it stand and gain it.
        Verify {
            /// The peak's timeline moment, already resolved.
            at: f64,
            /// The level the accent lands at, in dB.
            gain_db: f64,
            /// Where the clip was laid: what a correction moves from.
            start: f64,
            /// The family the peak's emotion named, for a re-shop.
            queries: Vec<&'static str>,
            /// How many of those words have already come back spent.
            tried: usize,
            /// The clip the add placed.
            clip_id: String,
        },
        /// The move's reply is in: gain the clip it moved.
        Level {
            /// The level the accent lands at, in dB.
            gain_db: f64,
            /// The clip the move kept.
            clip_id: String,
        },
        /// The remove's reply is in: the family's next word, or the peak
        /// goes unaccented.
        Reshop {
            /// The peak's timeline moment, already resolved.
            at: f64,
            /// The level the accent lands at, in dB.
            gain_db: f64,
            /// The family the peak's emotion named, walked word by word.
            queries: Vec<&'static str>,
            /// How many of those words have already come back spent.
            tried: usize,
        },
    }
    steps.push(
        DerivedStep::react(
            Stage::Sfx,
            Box::new({
                let mut peaks: Option<Vec<(String, f64, f64)>> = None;
                // The per-take accent attenuation, resolved once with the
                // peaks: a take whose SNR sits under 15 dB is a take whose
                // floor lives close under the voice, and an accent at its
                // full bucket level over that floor is mud. The catalogue's
                // own rule - low SNR down-weights the reading by a fifth -
                // and a fifth in amplitude is just under 2 dB, spent as an
                // attenuation and not as a multiplier on a negative gain
                // (which would make the accent louder). A take with no SNR
                // reading keeps the full level: unmeasured is not the same
                // answer as noisy.
                let mut accent_attenuation: Option<Vec<(String, f64)>> = None;
                let mut placed = 0usize;
                let mut pending: Option<SfxPending> = None;
                // Every accent the run lands is spent: the same id twice
                // inside half a minute is the repetition the plan validator
                // warns the Director about, and the rotation walks each
                // peak's family from a different word so neighbouring
                // accents do not all shop the same shelf answer.
                let mut used: Vec<String> = Vec::new();
                let mut rotation = 0usize;
                // Whether a generator answered this run: Some(true) after a
                // file was written, Some(false) after the model said it was
                // absent - which stops every later peak from asking again.
                let mut ai_accent: Option<bool> = None;
                move |log| {
                    let peaks = peaks.get_or_insert_with(|| {
                        // Each peak carries the take it was measured on:
                        // a file-second only means anything inside its own
                        // take's clock, and every consumer below - the
                        // placement, the emotion read, the register lift,
                        // the shopping - must answer for that take, never
                        // the first take that happens to span the same
                        // file-second.
                        let voice = voice_reads(log, &sfx_source);
                        let mut found = emphasis_peaks(&voice);
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
                            found.retain(|(take, time, _)| {
                                emotion_at(&read, take, *time).is_some_and(|line| {
                                    line.emotion != "neutral" && line.intensity >= 0.5
                                })
                            });
                        }
                        // The register the peak's own line sits in: a peak
                        // on a line the delivery lifted over the take's own
                        // register is a peak the take itself raised, so it
                        // outranks a peak of the same measured strength on
                        // a settled line. The lift is worth a tenth of its
                        // semitones - a full octave's lift adds two points
                        // of strength, a modest rise adds a fraction.
                        for peak in &mut found {
                            let lift = voice_read_of(&voice, &peak.0).and_then(|voice_read| {
                                voice_read
                                    .line_at(peak.1)
                                    .and_then(|line| voice_read.register_lift(line))
                            });
                            if let Some(lift) = lift {
                                peak.2 += lift * 0.1;
                            }
                        }
                        // A hoarse line does not carry an accent: the
                        // forensic ear's own voice-quality reading (an HNR
                        // under 10 dB or a jitter over 5%) retires the
                        // peak, because a hit under an unstable voice is
                        // emphasis the delivery cannot hold. A line with no
                        // voice-quality reading keeps its peak - unmeasured
                        // is not the same answer as hoarse.
                        found.retain(|(take, time, _)| {
                            voice_read_of(&voice, take)
                                .and_then(|voice_read| {
                                    voice_read
                                        .line_at(*time)
                                        .and_then(|line| voice_read.clean_voice(line))
                                })
                                .is_none_or(|clean| clean)
                        });
                        // A reverberant take smears anything tight-synced to
                        // a word: the forensic ear's own tail estimate over
                        // 0.4 s retires the peak, because an accent timed
                        // to the sample under a room's tail is a smear, not
                        // a hit - the accent the ear cannot hold is an
                        // accent the mix must not place. A take with no
                        // tail reading keeps its peaks: unmeasured is not
                        // the same answer as reverberant.
                        found.retain(|(take, _, _)| {
                            voice_read_of(&voice, take)
                                .and_then(|voice_read| voice_read.take_wide.reverb_tail_sec)
                                .is_none_or(|tail| tail <= 0.4)
                        });
                        // The strongest six, earliest first: the run's
                        // accent budget, spent where the read peaked.
                        found.sort_by(|left, right| right.2.total_cmp(&left.2));
                        found.truncate(6);
                        found.sort_by(|left, right| left.1.total_cmp(&right.1));
                        // The attenuation lands beside the peaks it
                        // softens, from the same readings, in the same
                        // pass.
                        let penalties = voice
                            .iter()
                            .map(|read| {
                                let penalty = match read.take_wide.snr_db {
                                    Some(snr) if snr < 15.0 => 1.94,
                                    _ => 0.0,
                                };
                                (read.take.clone(), penalty)
                            })
                            .collect::<Vec<_>>();
                        accent_attenuation.get_or_insert(penalties);
                        found
                    });
                    // A reply is waiting to be read: the pending state says
                    // which one, and each state reads only its own reply.
                    match pending.take() {
                        Some(SfxPending::Generate {
                            at,
                            gain_db,
                            queries,
                            tried,
                        }) => {
                            // The generator's own reply: a written file is
                            // placed the way a shelf hit is, and a model
                            // that is not installed sends this peak - and
                            // every peak after it - to the shelf instead.
                            let written = log
                                .last()
                                .filter(|reply| reply.get("ok") == Some(&json!(true)));
                            if let Some(path) = written
                                .and_then(|reply| reply.get("path"))
                                .and_then(|path| path.as_str())
                            {
                                ai_accent = Some(true);
                                // The model's own reply measured the file it
                                // wrote: `syncOffset` is where the peak sits
                                // in it, and the placement backs that lead-up
                                // out - the accent's own transient meets the
                                // moment the emphasis measured, not the
                                // file's first byte.
                                let sync = written
                                    .and_then(|reply| reply.get("syncOffset"))
                                    .and_then(|sync| sync.as_f64())
                                    .filter(|sync| sync.is_finite() && *sync >= 0.0)
                                    .unwrap_or(0.0);
                                let start = round3((at - sync).max(0.0));
                                pending = Some(SfxPending::Add {
                                    at,
                                    gain_db,
                                    start,
                                    id: path.to_owned(),
                                    queries: queries.clone(),
                                    tried,
                                    query: queries[0],
                                });
                                return Some(call(
                                    "sounds_add",
                                    json!({
                                        "path": sfx_project,
                                        "file": path,
                                        "start": start,
                                    }),
                                ));
                            }
                            ai_accent = Some(false);
                            let query = queries[tried % queries.len()];
                            pending = Some(SfxPending::Pick {
                                at,
                                gain_db,
                                queries: queries.clone(),
                                tried,
                                query,
                            });
                            return Some(call(
                                "sounds_search",
                                json!({
                                    "query": query,
                                    "limit": 3,
                                }),
                            ));
                        }
                        Some(SfxPending::Pick {
                            at,
                            gain_db,
                            queries,
                            mut tried,
                            query,
                        }) => {
                            // The search's answer: the first hit this run
                            // has not already spent, accents being short -
                            // a five-second ambience is a bed, not an
                            // accent, whatever word the query shared.
                            let hits: Vec<&Value> = log
                                .last()
                                .and_then(|reply| {
                                    reply.get("sounds").and_then(|sounds| sounds.as_array())
                                })
                                .into_iter()
                                .flatten()
                                .collect();
                            let fresh: Vec<&Value> = hits
                                .iter()
                                .copied()
                                .filter(|hit| {
                                    hit.get("id")
                                        .and_then(|id| id.as_str())
                                        .is_some_and(|id| !used.iter().any(|spent| spent == id))
                                })
                                .collect();
                            // An accent is a hit, not a bed: when every
                            // unspent hit runs past three seconds the word
                            // moves on to its next query rather than
                            // placing a six-second ambience at -9 dB (the
                            // shelf carries sounds up to 40 s; "drone" and
                            // "ambience" families return nothing shorter).
                            let chosen = fresh.iter().copied().find(|hit| {
                                hit["duration"].as_f64().is_none_or(|secs| secs <= 3.0)
                            });
                            if let Some(hit) = chosen {
                                let id = hit["id"].as_str().unwrap_or_default().to_owned();
                                // The manifest's own `syncAt` says how far
                                // into the file the sound lands its peak:
                                // the add is offset by exactly that, so the
                                // peak meets the moment the emphasis
                                // measured - not the file's first byte. The
                                // check below then holds that placement
                                // against the file itself, which is where a
                                // stale manifest answer is caught.
                                let sync = hit["syncAt"]
                                    .as_f64()
                                    .filter(|sync| sync.is_finite() && *sync >= 0.0)
                                    .unwrap_or(0.0);
                                let start = round3((at - sync).max(0.0));
                                pending = Some(SfxPending::Add {
                                    at,
                                    gain_db,
                                    start,
                                    id: id.clone(),
                                    queries,
                                    tried,
                                    query,
                                });
                                return Some(call(
                                    "sounds_add",
                                    json!({
                                        "path": sfx_project,
                                        "id": id,
                                        "start": start,
                                    }),
                                ));
                            }
                            // Every hit the word answered is spent or absent:
                            // the family's next word, and the peak is dropped
                            // only when the family is exhausted - one empty
                            // shelf answer is not a peak without a sound.
                            tried += 1;
                            if tried < queries.len() {
                                let query = queries[(rotation + tried) % queries.len()];
                                pending = Some(SfxPending::Pick {
                                    at,
                                    gain_db,
                                    queries,
                                    tried,
                                    query,
                                });
                                return Some(call(
                                    "sounds_search",
                                    json!({
                                        "query": query,
                                        "limit": 3,
                                    }),
                                ));
                            }
                            // The family is spent: the peak goes unaccented,
                            // the way a mixer with a thin shelf leaves it.
                            placed += 1;
                            rotation = rotation.wrapping_add(1);
                        }
                        Some(SfxPending::Add {
                            at,
                            gain_db,
                            start,
                            id,
                            queries,
                            tried,
                            query,
                        }) => {
                            // The accent is spent the moment it is placed:
                            // the id goes on the `used` list whether the
                            // check below keeps it or drops it, because a
                            // hit that measured badly is not a hit this
                            // run shops again for the next peak.
                            used.push(id.clone());
                            // Rotation records each spent sound. The peak
                            // cursor advances only when this peak is finally
                            // gained, refused or exhausted: a wrong-sound
                            // removal re-shops this same peak.
                            rotation = rotation.wrapping_add(1);
                            let added = log.last();
                            let clip = added
                                .and_then(|reply| reply.pointer("/clipId"))
                                .and_then(|clip| clip.as_str());
                            let file = added
                                .and_then(|reply| reply.get("file"))
                                .and_then(|file| file.as_str());
                            if clip.is_none() {
                                placed += 1;
                                // No clip came back (a refused add under the
                                // warn policy): the id still counts as
                                // spent. Falling through, not returning
                                // `None` - the runner would read that as the
                                // accents being done and drop every peak
                                // after this one.
                            } else if file.is_none() {
                                placed += 1;
                                // An add that names no copy cannot be
                                // verified: the accent is gained at the
                                // level the mix can hold, and the placement
                                // stands on the add's own say-so.
                                return Some(call(
                                    "clip_audio_levels_db",
                                    json!({
                                        "path": sfx_project,
                                        "clipId": clip.unwrap_or_default(),
                                        "baseDb": gain_db,
                                        "replaceAutomation": true,
                                    }),
                                ));
                            } else {
                                let clip_id = clip.unwrap_or_default().to_owned();
                                let file = file.unwrap_or_default().to_owned();
                                // The placement is held against the file
                                // itself: the check measures the copy the
                                // timeline will actually play, so a manifest
                                // `syncAt` that drifted - or was never there
                                // - is caught here and corrected, not
                                // inherited by the viewer.
                                pending = Some(SfxPending::Verify {
                                    at,
                                    gain_db,
                                    start,
                                    queries,
                                    tried,
                                    clip_id: clip_id.clone(),
                                });
                                return Some(call(
                                    "sense_sound",
                                    json!({
                                        "action": "check_placement",
                                        "role": sfx_role_for_query(query),
                                        "path": file,
                                        "clipStart": start,
                                        "intendedAt": at,
                                        "gainDb": gain_db,
                                        "toleranceMs": 40,
                                        "clipId": clip_id,
                                    }),
                                ));
                            }
                        }
                        Some(SfxPending::Verify {
                            at,
                            gain_db,
                            start,
                            queries,
                            tried,
                            clip_id,
                        }) => {
                            let checked = log.last();
                            let issues = checked
                                .and_then(|reply| reply.get("issues"))
                                .and_then(|issues| issues.as_array())
                                .cloned()
                                .unwrap_or_default();
                            let codes = issues
                                .iter()
                                .filter_map(|issue| issue.get("code"))
                                .filter_map(|code| code.as_str())
                                .collect::<Vec<_>>();
                            if checked.and_then(|reply| reply.get("ok")) != Some(&json!(true)) {
                                placed += 1;
                                // The check itself could not answer: the
                                // placement stands as laid, said plainly -
                                // unverified is not the same claim as
                                // verified, and neither is a refusal to
                                // place. The accent is gained and the run
                                // moves on.
                                return Some(call(
                                    "clip_audio_levels_db",
                                    json!({
                                        "path": sfx_project,
                                        "clipId": clip_id,
                                        "baseDb": gain_db,
                                        "replaceAutomation": true,
                                    }),
                                ));
                            }
                            if codes.contains(&"peak_cut") || codes.contains(&"wrong_sound") {
                                // The transient was trimmed out, or the file
                                // measures as another family's sound: no move
                                // can put that right. The clip comes off, and
                                // the family's next word shops the peak
                                // again - a broken accent is not left on the
                                // lane to be heard as one.
                                pending = Some(SfxPending::Reshop {
                                    at,
                                    gain_db,
                                    queries,
                                    tried,
                                });
                                return Some(call(
                                    "clips_remove",
                                    json!({
                                        "path": sfx_project,
                                        "clipIds": [clip_id],
                                        "ripple": false,
                                    }),
                                ));
                            }
                            if codes.contains(&"sync_off") {
                                // The peak measures off the moment: the check
                                // says by how much, and the move lands the
                                // transient on the moment the emphasis
                                // measured. One move, clamped to the
                                // timeline's own start, then the level.
                                let move_by = checked
                                    .and_then(|reply| reply.pointer("/measured/moveBy"))
                                    .and_then(|shift| shift.as_f64())
                                    .filter(|shift| shift.is_finite())
                                    .unwrap_or(0.0);
                                let corrected = round3((start + move_by).max(0.0));
                                pending = Some(SfxPending::Level {
                                    gain_db,
                                    clip_id: clip_id.clone(),
                                });
                                return Some(call(
                                    "clips_move",
                                    json!({
                                        "path": sfx_project,
                                        "moves": [
                                            { "clipId": clip_id, "start": corrected },
                                        ],
                                    }),
                                ));
                            }
                            // As directed: the placement stands, and the
                            // accent is gained at the level the mix can
                            // hold - the dB-aware levels call is what gives
                            // it a gain instead of the source's own unity.
                            placed += 1;
                            return Some(call(
                                "clip_audio_levels_db",
                                json!({
                                    "path": sfx_project,
                                    "clipId": clip_id,
                                    "baseDb": gain_db,
                                    "replaceAutomation": true,
                                }),
                            ));
                        }
                        Some(SfxPending::Level { gain_db, clip_id }) => {
                            // The move is in: the clip sits on the moment, and
                            // the level is what it was waiting for.
                            placed += 1;
                            return Some(call(
                                "clip_audio_levels_db",
                                json!({
                                    "path": sfx_project,
                                    "clipId": clip_id,
                                    "baseDb": gain_db,
                                    "replaceAutomation": true,
                                }),
                            ));
                        }
                        Some(SfxPending::Reshop {
                            at,
                            gain_db,
                            queries,
                            mut tried,
                        }) => {
                            // The broken accent is off the lane; the family's
                            // next word shops the peak again, and the peak
                            // goes unaccented when the family is spent - the
                            // bookkeeping was done at the add, so this walk
                            // spends words only.
                            tried += 1;
                            if tried < queries.len() {
                                let query = queries[(rotation + tried) % queries.len()];
                                pending = Some(SfxPending::Pick {
                                    at,
                                    gain_db,
                                    queries,
                                    tried,
                                    query,
                                });
                                return Some(call(
                                    "sounds_search",
                                    json!({
                                        "query": query,
                                        "limit": 3,
                                    }),
                                ));
                            }
                            placed += 1;
                            // The family is spent twice over: the peak stays
                            // unaccented, the way a mixer with a thin shelf
                            // leaves it.
                        }
                        None => {
                            // The next call is a fresh shop.
                        }
                    }
                    // The next peak the timeline actually plays: a peak
                    // cut away or never placed is stepped over, not the
                    // end of the stage. The runner reads `None` as "the
                    // accents are done", so answering it for one unplayed
                    // peak would drop every peak after it - the run would
                    // quietly lose the accents it was written to land.
                    let at = next_played_peak(&mut placed, peaks, log)?;
                    // The accent family the peak's own line asks for: the
                    // peak's own take and file moment are the line's, so
                    // the emotion read there names the sound.
                    let (take, moment) = peaks
                        .get(placed)
                        .map(|(take, moment, _)| (take.clone(), *moment))
                        .unwrap_or_default();
                    let read = emotion_lines(log, &sfx_emotion);
                    let line = emotion_at(&read, &take, moment);
                    let emotion = line
                        .map(|line| line.emotion.clone())
                        .unwrap_or_else(|| "neutral".to_owned());
                    // The hook's own accent is not the mood's business:
                    // the first three seconds are the only part of a
                    // short cut a viewer is guaranteed to see, so a peak
                    // that lands there is shopped for entrance energy -
                    // an impact, a riser, a whoosh - whatever the line's
                    // mood would otherwise have asked for.
                    let hook = at < 3.0;
                    let queries = if hook {
                        vec!["impact", "riser", "whoosh"]
                    } else {
                        sfx_queries_for_line(line, &emotion)
                    };
                    let strength = peaks
                        .get(placed)
                        .map(|(_, _, strength)| *strength)
                        .unwrap_or(0.5);
                    // The level this accent lands at: the strength's own
                    // bucket, attenuated by the take's own noise penalty -
                    // a noisy take's accents sit a fifth down, out of the
                    // mud its floor would make of them.
                    let penalties = accent_attenuation.get_or_insert_with(Vec::new);
                    let penalty = penalties
                        .iter()
                        .find(|(path, _)| *path == take)
                        .map(|(_, penalty)| *penalty)
                        .unwrap_or(0.0);
                    let gain_db = accent_gain_db(strength) - penalty;
                    // The generator first, when the machine has one: the
                    // hook is the one accent worth the heavier model, and
                    // every other peak is a short sound the small model
                    // writes in seconds. A model that is not installed
                    // answers `ok: false` in a second and the shelf takes
                    // over for the rest of the run.
                    if ai_accent != Some(false) {
                        let kind = if hook { "sfx-large" } else { "sfx-small" };
                        pending = Some(SfxPending::Generate {
                            at,
                            gain_db,
                            queries: queries.clone(),
                            tried: 0,
                        });
                        return Some(call(
                            "sense_sound_model",
                            json!({
                                "action": "generate",
                                "kind": kind,
                                "prompt": sfx_prompt(&queries, &emotion),
                                "seconds": if hook { 2.5 } else { 1.5 },
                                "seed": placed,
                                "project": sfx_project,
                            }),
                        ));
                    }
                    let query = queries[rotation % queries.len()];
                    pending = Some(SfxPending::Pick {
                        at,
                        gain_db,
                        queries: queries.clone(),
                        tried: 0,
                        query,
                    });
                    Some(call(
                        "sounds_search",
                        json!({
                            "query": query,
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
                        let sheet = log
                            .iter()
                            .rev()
                            .find(|reply| {
                                reply
                                    .get("pngPath")
                                    .and_then(Value::as_str)
                                    .is_some_and(|png| !png.trim().is_empty())
                            })
                            .cloned()?;
                        let png = sheet["pngPath"].as_str().unwrap_or_default();
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
                        // and the stage would call that a critique. The
                        // SHEET travels with the pixels: its craft and
                        // audio measurements are what the critique scores
                        // against - pngPath alone left every reading empty
                        // and the heuristic answered a constant 6.7.
                        return Some(call(
                            "craft_critique",
                            json!({
                                "pngPath": png,
                                "sheet": sheet,
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
                        let sheet = log
                            .iter()
                            .rev()
                            .find(|reply| {
                                reply
                                    .get("pngPath")
                                    .and_then(Value::as_str)
                                    .is_some_and(|png| !png.trim().is_empty())
                            })
                            .cloned()
                            .unwrap_or_else(|| json!({}));
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
                                "sheet": sheet,
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

    // 13. Save and read back the editable project. AUTO EDIT's default
    //     deliverable is the timeline the user can continue editing, not
    //     an MP4 the user did not ask to render.
    let ready_project = source.project.clone();
    steps.push(
        StageStep::run(
            Stage::TimelineReady,
            vec![
                call("project_save", json!({ "path": ready_project.clone() })),
                call("timeline_read", json!({ "path": ready_project.clone() })),
            ],
        )
        .into(),
    );

    // 14. Verify the saved timeline and its lanes. The independent
    //     acceptance receipt checks every media reference and source path;
    //     this stage's own gate refuses shared lanes before the user opens
    //     the result.
    let verify_project = source.project.clone();
    steps.push(
        StageStep::run(
            Stage::Verify,
            vec![
                call("collision_check", json!({ "path": verify_project.clone() })),
                call("timeline_read", json!({ "path": verify_project })),
            ],
        )
        .gate(ReplyGate::True {
            field: "clean",
            what: "clips share a lane on the saved timeline",
        })
        .into(),
    );

    // 15. Review the final timeline after revisions. watch_render renders
    //     a preview/contact sheet from the open project, not an export;
    //     the critic scores that sheet against the measured brief.
    let watch_project = source.project.clone();
    let watch_sheet = source.sheet.clone();
    steps.push(
        DerivedStep::react(
            Stage::Watch,
            Box::new({
                let watch_project = watch_project.clone();
                let watch_sheet = watch_sheet.clone();
                let mut phase = 0u8;
                move |log| {
                    if phase == 0 {
                        phase = 1;
                        return Some(call(
                            "watch_render",
                            json!({
                                "project": watch_project,
                                "out": watch_sheet,
                                "audio": true,
                            }),
                        ));
                    }
                    if phase == 1 {
                        phase = 2;
                        let sheet = log
                            .iter()
                            .rev()
                            .find(|reply| {
                                reply
                                    .get("pngPath")
                                    .and_then(Value::as_str)
                                    .is_some_and(|path| !path.trim().is_empty())
                            })
                            .cloned()
                            .unwrap_or_else(|| json!({}));
                        let png_path = sheet
                            .get("pngPath")
                            .and_then(Value::as_str)
                            .unwrap_or_default();
                        return Some(call(
                            "craft_critique",
                            json!({
                                "pngPath": png_path,
                                "sheet": sheet,
                                "project": watch_project,
                                "brief": signal_brief(log),
                                "autoSetup": true,
                            }),
                        ));
                    }
                    None
                }
            }),
        )
        .warn()
        .gate(ReplyGate::AtLeast {
            field: "overall",
            minimum: 7.0,
            what: "the final timeline preview scored under the craft bar",
        })
        .into(),
    );
    EditProgram::mixed(PathBuf::from("timeline"), steps).enforcing_acceptance()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fixed-spine source an agent's call is built from: narration
    /// audio only, the bed skipped, takes de-duplicated in first-play
    /// order, and every placement carrying its file span and rate.
    #[test]
    fn a_view_becomes_the_same_takes_and_placements_the_button_builds() {
        let view = json!({
            "media": [
                { "id": "m1", "path": "/edits/Reel/voice/a.wav" },
                { "id": "m2", "path": "/edits/Reel/bgm/bed.mp3" },
                { "id": "m3", "path": "/edits/Reel/voice/b.wav" },
            ],
            "timeline": { "clips": [
                { "kind": "audio", "media": "m2", "start": 0.0, "duration": 8.0, "sourceStart": 0.0, "speed": 1.0 },
                { "kind": "audio", "media": "m1", "start": 1.0, "duration": 2.0, "sourceStart": 0.5, "speed": 1.0 },
                { "kind": "audio", "media": "m1", "start": 4.0, "duration": 1.0, "sourceStart": 3.0, "speed": 1.5 },
                { "kind": "audio", "media": "m3", "start": 6.0, "duration": 1.0, "sourceStart": 0.0, "speed": 1.0 },
            ] }
        });
        let source = auto_edit_source_from_view(
            "/edits/Reel edit",
            &view,
            Some("  ".to_owned()),
            "pixabay".to_owned(),
        )
        .expect("the view is a narration timeline");
        assert_eq!(source.takes.len(), 2);
        assert_eq!(source.takes[0].take, "/edits/Reel/voice/a.wav");
        assert_eq!(
            source.takes[0].enhanced,
            "/edits/Reel edit/auto-edit/enhanced-000-a.wav"
        );
        assert_eq!(source.takes[1].take, "/edits/Reel/voice/b.wav");
        assert_eq!(source.placements.len(), 3);
        assert_eq!(source.placements[1].source_in, 3.0);
        assert_eq!(source.placements[1].source_out, 4.5);
        assert_eq!(source.placements[1].speed, 1.5);
        assert_eq!(source.stock_provider, "pixabay");
        assert_eq!(source.sheet, "auto-edit-sheet.png");
        assert_eq!(source.script, None, "a blank script is no script");

        let empty = json!({ "media": [], "timeline": { "clips": [] } });
        let refused =
            match auto_edit_source_from_view("/edits/Empty", &empty, None, "pexels".to_owned()) {
                Ok(_) => panic!("no narration is a refusal"),
                Err(refused) => refused,
            };
        assert!(refused.contains("no narration audio"), "{refused}");
    }

    /// The program the button runs: the whole unattended spine in the
    /// runner's register - the static handoff, the fanned repair and
    /// transcription (every take at the same time), the derived tighten,
    /// grade, licensed live-stock B-roll, captions, title entrances,
    /// music bed and accents, the preview, the revise, the saved
    /// timeline, the verify and the watch. The plan's creative brief, the
    /// eye-picked stock and the baked motion comps remain the Director
    /// agent's.
    #[test]
    fn the_auto_edit_program_is_the_watchers_spine_over_fixed_paths() {
        use concat_director::{ErrorPolicy, FanStep, ProgramStep};

        let source = AutoEditProgramSource {
            project: "/edits/Reel edit".to_owned(),
            takes: vec![
                TakePaths {
                    take: "/edits/Reel edit/voiceover/take-000-a.wav".to_owned(),
                    enhanced: "/edits/Reel edit/voiceover/enhanced-000-a.wav".to_owned(),
                    aroll: None,
                },
                TakePaths {
                    take: "/edits/Reel edit/voiceover/take-001-b.wav".to_owned(),
                    enhanced: "/edits/Reel edit/voiceover/enhanced-001-b.wav".to_owned(),
                    aroll: None,
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
            script: None,
        };
        let program = super::auto_edit_program(&source);

        // The register, in the run's own order: the fanned repair and
        // transcription, the derived tighten-then-grade, B-roll, captions,
        // title entrances, bed and accents, the preview, the revise,
        // the saved timeline, the verify gates and the final watch.
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
                "timeline-ready",
                "verify",
                "watch",
            ]
        );
        // Stage::ALL is the complete register, including the optional
        // explicit-export stage. The default run must be an ordered
        // subsequence and must not schedule an export.
        let register: Vec<&str> = Stage::ALL.iter().map(|stage| stage.label()).collect();
        assert!(order.iter().all(|stage| register.contains(stage)));
        assert!(!order.contains(&"export"));
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

        // The program's artifact is the editable timeline. Rendering
        // remains an explicit user action in the editor.
        assert_eq!(program.export, std::path::Path::new("timeline"));
        assert!(
            program.enforce_acceptance,
            "the auto edit claims the whole edit, so its receipt must decide the outcome"
        );

        // The repair is fanned, one lane per take, and its chain lands on
        // the take the timeline plays: the export ships the repaired
        // sound, the watcher's own promise. The lane measures before it
        // repairs: diagnose_take first (its reply sits in the log where
        // the critic reads what the repair was chosen on), the
        // enhancement second, and only then the repair the diagnosis
        // ordered - a hum gets its notch, a DC offset its blocker, a
        // muffled take a presence lift instead of the denoise that
        // would deepen it.
        let ProgramStep::Fanned(repair) = &program.steps[1] else {
            panic!("the repair is fanned");
        };
        assert_eq!(repair.groups.len(), 2, "one lane per take");
        assert_eq!(repair.groups[0][0].tool, "sense_audio_repair");
        assert_eq!(repair.groups[0][0].arguments["action"], "diagnose_take");
        assert_eq!(
            repair.groups[0][0].arguments["audio"],
            source.takes[0].take.as_str()
        );
        assert_eq!(repair.groups[0][1].arguments["action"], "enhance_speech");
        assert_eq!(
            repair.groups[0][1].arguments["out"],
            source.takes[0].enhanced.as_str()
        );
        assert_eq!(
            repair.groups[0][2].arguments["action"],
            "repair_by_diagnosis"
        );
        assert_eq!(
            repair.groups[0][2].arguments["audio"],
            source.takes[0].enhanced.as_str()
        );
        assert_eq!(
            repair.groups[0][2].arguments["out"],
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
        for at in [4usize, 6, 7, 8, 9, 10, 11, 12, 13, 16] {
            assert!(
                matches!(program.steps[at], ProgramStep::Derived(_)),
                "step {at} is derived"
            );
        }

        // The saved timeline is explicitly written and read back; the
        // final verify checks collisions, and the final watch reviews the
        // open project rather than an automatically encoded MP4.
        let ready = program.steps[14].as_static();
        assert_eq!(ready.calls[0].tool, "project_save");
        assert_eq!(ready.calls[0].arguments["path"], source.project.as_str());
        assert_eq!(ready.calls[1].tool, "timeline_read");
        assert_eq!(ready.calls[1].arguments["path"], source.project.as_str());
        let verify = program.steps[15].as_static();
        assert_eq!(verify.calls[0].tool, "collision_check");
        assert_eq!(verify.calls[1].tool, "timeline_read");
        assert_eq!(
            verify.gates,
            vec![ReplyGate::True {
                field: "clean",
                what: "clips share a lane on the saved timeline",
            }]
        );
        assert!(matches!(program.steps[16], ProgramStep::Derived(_)));
        // The transcription's own done gate: a wait that gives up with
        // "still running" used to complete the stage over a transcript
        // that never arrived, and every word-derived stage downstream
        // silently shipped defaults.
        assert_eq!(
            program.steps[2].gates(),
            vec![ReplyGate::Word {
                field: "state",
                equals: "done",
                what: "a take's transcription was still running when its waits ran out",
            }]
        );
        assert_eq!(
            program.steps[16].gates(),
            vec![ReplyGate::AtLeast {
                field: "overall",
                minimum: 7.0,
                what: "the final timeline preview scored under the craft bar",
            }]
        );
        // The plan's own gate: its structure is refused before a single
        // edit lands, not warned about after the fact.
        assert_eq!(
            program.steps[5].gates(),
            vec![ReplyGate::True {
                field: "valid",
                what: "the plan the run wrote did not validate",
            }]
        );
        // No other step gates its replies.
        for (at, step) in program.steps.iter().enumerate() {
            if at != 2 && at != 5 && at != 15 && at != 16 {
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
            aroll: None,
        }];
        let log = vec![json!({ "audio": { "path": "/takes/a.wav" },
            "lines": [{
                "prosody": { "emphasisWords": [
                    { "t": 1.0, "strength": 1.2, "wordText": "wreck" },
                ]},
            }],
        })];
        let words = stressed_words(&log, &takes);
        assert_eq!(
            words,
            vec![("/takes/a.wav".to_owned(), vec!["wreck".to_owned()])],
            "the set is the take's own, keyed by the take it was measured on"
        );
        let carries = |text: &str| {
            text.split_whitespace()
                .map(|w| {
                    w.trim_matches(|c: char| !c.is_alphanumeric())
                        .to_lowercase()
                })
                .any(|w| words[0].1.contains(&w))
        };
        assert!(carries("we found the wreck"));
        assert!(!carries("the hull was empty"));
    }

    /// A claim the narration asserts with no evidence behind it is the
    /// argument's weak point, and the proof card is the edit's answer.
    /// The reader exists because the profile's graphic policy named
    /// `claims` and nothing produced it: the signal was measured by the
    /// discourse graph and spent by nobody.
    #[test]
    fn an_unbacked_claim_earns_a_proof_card() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "this ship sank in 1912" },
                { "start": 3.0, "end": 5.0, "text": "because the hull was torn open" },
            ],
        });
        let graph = json!({
            "uncoveredClaims": [
                { "id": "c0", "kind": "claim", "line": 0,
                  "text": "this ship sank in 1912", "start": 0.0, "end": 2.0,
                  "strength": 0.55, "evidenceCues": [], "hedges": [] },
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
            json!({ "tool": "extract_claim_graph",
                    "uncoveredClaims": graph["uncoveredClaims"].clone() }),
        ];
        let claims = uncovered_claims(&log, &takes);
        assert_eq!(claims.len(), 1, "the uncovered claim is the one read");
        assert_eq!(claims[0].text, "this ship sank in 1912");
        let specs = motion_specs(&[], &[], &[], &[], &[], &[], &[], &claims, false);
        let card = specs
            .iter()
            .find(|spec| spec.earned_by == "claims")
            .expect("the claim earns a card");
        assert_eq!(card.preset, "funcut.quote.pull");
        assert_eq!(card.act, "proof");
    }

    /// A question the narration asks is a card the act earns. The
    /// profile's graphic policy names `speechActs` for exactly this, and
    /// before the reader existed the signal was measured by nothing and
    /// spent by nothing.
    #[test]
    fn a_question_line_earns_its_own_card() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "have you ever lost a wreck" },
                { "start": 3.0, "end": 5.0, "text": "it was not empty" },
            ],
        });
        let acts = json!({
            "perLine": [
                { "line": 0, "text": "have you ever lost a wreck",
                  "start": 0.0, "end": 2.0,
                  "acts": ["rhetorical_question"], "primary": "rhetorical_question" },
                { "line": 1, "text": "it was not empty",
                  "start": 3.0, "end": 5.0,
                  "acts": ["statement"], "primary": "statement" },
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
            json!({ "tool": "detect_speech_acts",
                    "perLine": acts["perLine"].clone() }),
        ];
        let questions = question_acts(&log, &takes);
        assert_eq!(questions.len(), 1, "only the question line is an ask");
        assert_eq!(questions[0].start, 0.0);
        let specs = motion_specs(&[], &[], &[], &[], &[], &[], &questions, &[], false);
        assert!(
            specs
                .iter()
                .any(|spec| spec.earned_by == "speechActs" && spec.at == 0.0),
            "the ask becomes a card: {specs:?}"
        );
    }

    /// The graphic's log entry names only the signals the choice consumed:
    /// naming `prosody` while the entrance is picked by the read's
    /// intensity would be the log asserting evidence no decision used.
    #[test]
    fn the_graphic_log_claims_only_the_signals_it_spent() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
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
                script: None,
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

    /// Every logged decision carries the confidence the profile rated its
    /// signals at, and an entry is only as certain as its weakest family.
    /// The profile answers `signalConfidence.perSignal`; until this it was
    /// parsed into `VoiceRead::weight` and read by nothing, so a decision
    /// taken on a whisper of a measurement looked exactly like a certain
    /// one.
    #[test]
    fn every_decision_carries_the_confidence_it_was_measured_at() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 3.0, "text": "we found the wreck" },
                { "start": 4.0, "end": 7.0, "text": "it was not empty" },
            ],
        });
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 8.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let log = vec![
            json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
            view,
            json!({ "lines": [
                emotion_row(0.0, 3.0, "surprise", 0.9, "hook", "hot", ""),
                emotion_row(4.0, 7.0, "fear", 0.9, "reveal", "cold", ""),
            ]}),
            // The profile's own rating of the families it measured. `words`
            // is certain, `emotion` less so, and the cut entry names both.
            json!({ "audio": { "path": "/t/a.wav" },
                "wholeTake": { "loudness": { "integratedLufs": -20.0 } },
                "signalConfidence": { "perSignal": [
                    { "signal": "wordConfidence", "confidence": 0.95 },
                    { "signal": "pauses", "confidence": 0.75 },
                    { "signal": "speechRate", "confidence": 0.9 },
                    { "signal": "loudness", "confidence": 0.9 },
                    { "signal": "speechRatio", "confidence": 0.9 },
                    { "signal": "forensic", "confidence": 0.8 },
                    { "signal": "prosody", "confidence": 0.85 },
                    { "signal": "sentenceType", "confidence": 0.8 },
                    { "signal": "motifs", "confidence": 0.7 },
                    { "signal": "language", "confidence": 0.85 },
                    { "signal": "speaker", "confidence": 0.7 },
                    { "signal": "entities", "confidence": 0.8 },
                    { "signal": "fillers", "confidence": 0.65 },
                ]} }),
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
                script: None,
            },
        )
        .expect("a plan from the read");
        let entries = plan["signalLog"].as_array().expect("a signal log");
        assert!(!entries.is_empty(), "the plan logs its decisions");
        // Every entry is rated, and every rating is one the profile gave.
        for entry in entries {
            let confidence = entry["confidence"]
                .as_f64()
                .unwrap_or_else(|| panic!("entry has no confidence: {entry}"));
            assert!(
                (0.0..=1.0).contains(&confidence),
                "confidence {confidence} is outside 0..1"
            );
            assert!(
                confidence > 0.0,
                "entry {entry} was rated 0 despite naming measured signals"
            );
        }
        // The graphic card names `emotion` and `words`; the weakest of the
        // two is the emotion read's own 0.85, so that is the entry's
        // rating, not the transcript's 0.95.
        let graphic = entries
            .iter()
            .find(|entry| entry["decision"] == json!("graphic"))
            .expect("a graphic entry");
        assert_eq!(
            graphic["confidence"],
            json!(0.85),
            "the card is only as certain as its least certain signal"
        );
        // `words` is rated under the profile's own name for it,
        // `wordConfidence`: without the alias the caption entry would log 0
        // for a transcript measured at 0.95.
        let caption = entries
            .iter()
            .find(|entry| entry["decision"] == json!("caption"))
            .expect("a caption entry");
        assert_eq!(
            caption["confidence"],
            json!(0.95),
            "the transcript's own rating reaches the caption"
        );
    }

    /// The plan AUTO EDIT builds must pass the schema `plan_validate`
    /// enforces, or the run dies at its own first gate. It emitted
    /// `pacing` entries while the canonical schema's decision enum stopped
    /// at `transition`, so every plan with a pacing entry was refused -
    /// the log this program writes was not a log the validator accepted.
    #[test]
    fn the_plan_auto_edit_builds_passes_the_real_plan_schema() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 3.0, "text": "we found the wreck" },
                { "start": 4.0, "end": 7.0, "text": "it was not empty" },
            ],
        });
        let view = json!({
            "project": { "name": "Reel", "width": 1080, "height": 1920 },
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 8.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let log = vec![
            json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
            view,
            json!({ "lines": [
                emotion_row(0.0, 3.0, "surprise", 0.9, "hook", "hot", ""),
                emotion_row(4.0, 7.0, "fear", 0.9, "reveal", "cold", ""),
            ]}),
            json!({ "audio": { "path": "/t/a.wav" },
                "directorHints": { "cutStyle": "mixed" },
                "wholeTake": {
                    "loudness": { "integratedLufs": -20.0 },
                    "forensic": { "noiseFloorDbfs": -60.0, "snrDb": 18.0 },
                },
                "pauses": [{ "start": 3.0, "end": 4.0, "class": "dead_air" }],
                "motion": {
                    "emphasisHits": [{ "line": 0, "at": 0.5, "word": "wreck",
                                       "strength": 0.8, "scale": 1.2 }],
                    "rateTempo": [{ "line": 0, "start": 0.0, "end": 3.0,
                                    "wpm": 210.0, "speed": 1.4 }],
                },
                "signalConfidence": { "perSignal": [
                    { "signal": "pauses", "confidence": 0.75 },
                    { "signal": "wordConfidence", "confidence": 0.95 },
                    { "signal": "loudness", "confidence": 0.9 },
                    { "signal": "forensic", "confidence": 0.8 },
                ]} }),
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
                script: None,
            },
        )
        .expect("a plan from the read");
        // The pacing entry is the one the enum used to refuse: prove the
        // fixture actually exercises it, or the test proves nothing.
        let decisions: Vec<&str> = plan["signalLog"]
            .as_array()
            .expect("a signal log")
            .iter()
            .filter_map(|entry| entry["decision"].as_str())
            .collect();
        assert!(
            decisions.contains(&"pacing"),
            "the fixture must exercise the pacing entry: {decisions:?}"
        );
        assert!(
            decisions.contains(&"motion"),
            "the fixture must exercise the motion entry: {decisions:?}"
        );
        let schema = crate::plan_schema::canonical();
        crate::schema::check(&schema, &plan, &schema, "plan")
            .expect("AUTO EDIT's own plan passes the schema plan_validate enforces");
    }

    /// The direction audition is a decision, not a label: a vertical tech
    /// explainer, a long evidence-led documentary and a wide emotional
    /// story must choose different films.
    #[test]
    fn the_direction_audition_picks_the_frame_and_topic() {
        let kinetic = vec!["the ai model runs on a chip".to_owned()];
        assert_eq!(
            choose_direction(&kinetic, &[], &[], 30.0, "9:16"),
            VisualDirection::Kinetic
        );
        let documentary = vec!["the archive evidence from the war".to_owned()];
        assert_eq!(
            choose_direction(&documentary, &[], &[], 120.0, "16:9"),
            VisualDirection::Documentary
        );
        let cinematic = vec!["her story of love and loss".to_owned()];
        assert_eq!(
            choose_direction(&cinematic, &[], &[], 45.0, "16:9"),
            VisualDirection::Cinematic
        );
        let alternatives = direction_alternatives(&cinematic, &[], &[], 45.0, "16:9");
        assert_eq!(alternatives.len(), 3);
        assert_eq!(
            alternatives
                .iter()
                .filter(|choice| choice["selected"] == json!(true))
                .count(),
            1
        );
    }

    /// The story skeleton covers the runtime without a gap, starts with
    /// the hook, and adds the longform intro/re-hook/outro the validator
    /// asks for.
    #[test]
    fn the_story_skeleton_covers_the_runtime_in_order() {
        for (runtime, mode) in [
            (3.0, "reel"),
            (10.0, "reel"),
            (30.0, "reel"),
            (90.0, "longform"),
        ] {
            let sections = story_sections(runtime, mode, runtime * 0.55, runtime * 0.9);
            let sections = sections.as_array().expect("sections");
            assert_eq!(sections[0]["role"], json!("hook"));
            assert_eq!(sections[0]["start"], json!(0.0));
            assert!(
                (sections.last().unwrap()["end"].as_f64().unwrap() - runtime).abs() < 0.06,
                "the sections end where the plan does"
            );
            for pair in sections.windows(2) {
                let left = pair[0]["end"].as_f64().unwrap();
                let right = pair[1]["start"].as_f64().unwrap();
                assert!((left - right).abs() < 0.06, "no gap or overlap");
            }
        }
        let longform = story_sections(90.0, "longform", 49.5, 81.0);
        let longform_roles: Vec<&str> = longform
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|section| section["role"].as_str())
            .collect();
        assert!(longform_roles.contains(&"rehook"));
        for role in [
            "hook",
            "intro",
            "curiosity",
            "progression",
            "rehook",
            "payoff",
            "cta",
            "ending",
        ] {
            assert!(
                longform_roles.contains(&role),
                "longform carries a {role} section"
            );
        }
        let short_sections = story_sections(30.0, "reel", 16.5, 27.0);
        let short: Vec<&str> = short_sections
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|section| section["role"].as_str())
            .collect();
        for role in ["hook", "curiosity", "progression", "payoff", "ending"] {
            assert!(short.contains(&role), "a reel carries a {role} section");
        }
    }

    /// The retention pass names the three failure modes the button is
    /// meant to catch: long low-energy stretches, repeated explanations
    /// and cuts that keep the same act and mood.
    #[test]
    fn the_retention_pass_names_repeats_and_boring_stretches() {
        let spoken = vec!["the ocean holds salt and the ocean holds salt".to_owned()];
        let segments = vec![
            json!({ "start": 0.0, "end": 10.0, "act": "body", "emotion": "neutral" }),
            json!({ "start": 10.0, "end": 20.0, "act": "body", "emotion": "neutral" }),
        ];
        let analysis = retention_analysis(&spoken, &segments, &[], 20.0);
        assert!(
            !analysis["repeatedExplanations"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        assert!(!analysis["boringStretches"].as_array().unwrap().is_empty());
        assert!(!analysis["weakTransitions"].as_array().unwrap().is_empty());
    }

    /// The Short finder keeps whole lines, scores the measured hook and
    /// payoff, and never returns two windows that start in the same place.
    #[test]
    fn the_short_finder_scores_questions_and_keeps_windows_apart() {
        let moments = vec![
            (0.0, 3.0, "Why did the ship sink?".to_owned()),
            (
                3.0,
                8.0,
                "The harbour master had logged a storm.".to_owned(),
            ),
            (
                8.0,
                14.0,
                "That's why the crates were still aboard.".to_owned(),
            ),
            (14.0, 20.0, "The next ship left at dawn.".to_owned()),
            (20.0, 26.0, "Why did nobody answer the radio?".to_owned()),
            (
                26.0,
                32.0,
                "The recording was erased that night.".to_owned(),
            ),
        ];
        let candidates = short_candidates(&moments, &[], 32.0, "9:16");
        assert!(!candidates.is_empty(), "a 32 second take has a Short");
        assert!(candidates.len() <= 3, "the plan keeps the top three");
        assert!(
            candidates[0]["why"]
                .as_str()
                .is_some_and(|why| why.contains("opens on a question")),
            "the opening question is named as the reason: {:?}",
            candidates[0]
        );
        for pair in candidates.windows(2) {
            let left = pair[0]["start"].as_f64().unwrap();
            let right = pair[1]["start"].as_f64().unwrap();
            assert!(
                (left - right).abs() >= 5.0,
                "candidate windows are distinct: {left} and {right}"
            );
        }
    }

    /// A measured stress inside a span outranks the direction's rotation:
    /// the entrance lands on the word, and two stressed neighbours
    /// alternate accents instead of repeating one.
    #[test]
    fn a_stressed_word_keeps_the_entrance_on_the_beat() {
        let mut segments = vec![
            json!({ "start": 0.0, "end": 2.0, "act": "body",
                    "beat": { "at": 1.0 }, "in": { "anim": "pop" } }),
            json!({ "start": 2.0, "end": 4.0, "act": "body",
                    "beat": { "at": 3.0 }, "in": { "anim": "pop" } }),
        ];
        vary_segments(&mut segments, VisualDirection::Cinematic);
        assert_eq!(segments[0]["in"]["anim"], json!("punch"));
        assert_eq!(segments[1]["in"]["anim"], json!("whip"));
    }

    /// The variety pass never lets two neighbours share a shot scale or
    /// entrance, and the plan records what the pass actually used.
    #[test]
    fn the_variety_pass_never_repeats_a_neighbour() {
        let mut segments = vec![
            json!({ "start": 0.0, "end": 2.0, "act": "body",
                    "visual": { "query": "a" }, "in": { "anim": "pop" } }),
            json!({ "start": 2.0, "end": 4.0, "act": "body",
                    "visual": { "query": "b" }, "in": { "anim": "pop" } }),
            json!({ "start": 4.0, "end": 6.0, "act": "body",
                    "visual": { "query": "c" }, "in": { "anim": "pop" } }),
        ];
        vary_segments(&mut segments, VisualDirection::Kinetic);
        let variety = visual_variety(&segments);
        assert_eq!(variety["consecutiveShotScaleRepeats"], json!(0));
        assert_eq!(variety["consecutiveEntranceRepeats"], json!(0));
        assert!(variety["shotScales"].as_array().unwrap().len() >= 2);
    }

    /// The alias reaches production, not just the helper: a `cut` entry
    /// names `pause.class=dead_air`, which the profile rates under
    /// `pauses`. Rated under the log's own name it would read 0 - the
    /// entry would say a measured silence was no evidence at all.
    #[test]
    fn the_alias_rates_a_pause_entry_in_production() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "we found the wreck" },
                { "start": 3.5, "end": 6.0, "text": "it was not empty" },
            ],
        });
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 8.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let log = vec![
            json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
            view,
            // The mood the gap opens on, so the transition entry is rated
            // by more than one family and the weakest link is a real one.
            json!({ "lines": [
                emotion_row(0.0, 2.0, "neutral", 0.4, "setup", "balanced", ""),
            ]}),
            // A measured dead air between the two lines: this is what
            // makes the transition entry name `pause`.
            json!({ "audio": { "path": "/t/a.wav" },
                "pauses": [{ "start": 2.0, "end": 3.5, "class": "dead_air" }],
                "signalConfidence": { "perSignal": [
                    { "signal": "pauses", "confidence": 0.75 },
                    { "signal": "wordConfidence", "confidence": 0.95 },
                ]} }),
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
                script: None,
            },
        )
        .expect("a plan from the read");
        let entries = plan["signalLog"].as_array().expect("a signal log");
        let transition = entries
            .iter()
            .find(|entry| entry["decision"] == json!("transition"))
            .expect("a transition entry");
        let signals = transition["signals"].as_array().expect("its signals");
        assert!(
            signals.contains(&json!("pause")),
            "the fixture measured a dead air at the seam: {signals:?}"
        );
        // The weakest link of the bare `pause` (rated by the profile's
        // `pauses` at 0.75) and `emotion` (0.85 from the fixture) is the
        // pause reading. Under the log's own name it would be 0.
        assert_eq!(transition["confidence"], json!(0.75));
    }

    /// The log's vocabulary is not always the profile's: `words` is rated
    /// `wordConfidence`, a `pause` is rated `pauses`, a `speakerChanges` is
    /// rated `speaker`. A name that maps to nothing would log a 0, which
    /// reads as "no evidence" for a signal that was measured.
    #[test]
    fn the_log_s_vocabulary_maps_onto_the_profile_s() {
        assert_eq!(confidence_family("words"), "words");
        assert_eq!(confidence_family("pause"), "pauses");
        assert_eq!(confidence_family("sentence"), "sentenceType");
        assert_eq!(confidence_family("speakerChanges"), "speaker");
        assert_eq!(confidence_family("noiseFloor"), "forensic");
        assert_eq!(confidence_family("musicSegments"), "forensic");
        assert_eq!(confidence_family("emphasisWords"), "prosody");
        // A name already in the profile's vocabulary passes through, so
        // the mapping cannot silently rename a family the profile rates.
        for family in ["loudness", "prosody", "entities", "motifs", "speechRate"] {
            assert_eq!(confidence_family(family), family);
        }
    }

    /// The vocabulary is the contract: every family the profile can
    /// answer must have a reader in this program, or be named here with
    /// the reason it does not. The check exists because three families
    /// (`speechActs`, `claims`, `rhythmLayers`) were measured, listed in
    /// the profile's own decision policy, and spent by nothing - the
    /// profile promised a signal the edit never used, and no test
    /// noticed.
    #[test]
    fn every_signal_family_has_a_reader() {
        // Families the program reads today, each through a named reader
        // or a stage that spends it.
        const READ: &[&str] = &[
            "pause",
            "emphasisWords",
            "speechRate",
            "rhythmLayers",
            "motifs",
            "words",
            "language",
            "sentence",
            "entities",
            "speechActs",
            "claims",
            "musicSegments",
            "speechRatio",
            "speakerChanges",
            "topics",
            "discourseMarkers",
            "loudness",
            "prosody",
            "fillers",
            "arc",
            "emotion",
            "noiseFloor",
            "wordConfidence",
            // Spent since the audit: the turn a seam falls on, and the
            // transient it lands on.
            "speaker",
            "forensic",
            "motion",
            // The picture's own families, spent by the gather and the
            // captions: the in-point, the drift veto, the continuity
            // direction and scale, the stock veto and the caption band.
            "visualMotion",
            "shotScale",
            "camera",
            "visualQuality",
            "saliency",
            "scene",
        ];
        // Families the vocabulary names but this program deliberately does
        // not spend, and why. Each is a real reading the profile makes; the
        // entry is the reason the edit is better without it.
        const NOT_SPENT: &[(&str, &str)] = &[(
            "formants",
            "a vocal-tract reading describes the speaker, not the cut: no \
                 timeline decision follows from it",
        )];
        let mut unaccounted: Vec<&str> = Vec::new();
        for family in crate::tools::signal_vocabulary() {
            if READ.contains(family) || NOT_SPENT.iter().any(|(name, _)| name == family) {
                continue;
            }
            unaccounted.push(family);
        }
        assert!(
            unaccounted.is_empty(),
            "these signals are in the vocabulary but no reader or reason accounts for them: {unaccounted:?}"
        );
        // The list above is a claim, and a claim can lie: `loudness` sat in
        // READ while nothing in the program ever set a level, so the
        // profile's LUFS reading reached no decision. Each READ family must
        // therefore also appear by name in the program's own source, outside
        // this test module - which is where a reader would have to mention
        // it. A family that is only listed here fails here.
        let program = include_str!("director_program.rs");
        let body = program
            .split("#[cfg(test)]")
            .next()
            .expect("the source has a program half");
        let unread: Vec<&str> = READ
            .iter()
            .copied()
            .filter(|family| !body.contains(&format!("\"{family}\"")))
            .collect();
        assert!(
            unread.is_empty(),
            "these families are listed as read but the program never names them: {unread:?}"
        );
        // Every entry is rated, so every name the log can carry must be one
        // the profile - or the engine that owns it - rates. A name that
        // mapped to nothing would log a confidence of 0 for a signal that
        // was measured, which reads as "no evidence" and is the opposite of
        // the truth. The families below are rated by the engines that
        // measured them, through `confidence_family` and the per-entry
        // match; the rest must be in the profile's own vocabulary.
        const RATED_ELSEWHERE: &[&str] = &[
            "emotion",          // the emotion engine, per line
            "arc",              // the emotion engine, per line
            "speechActs",       // the discourse engine, per line
            "claims",           // the discourse engine, per line
            "topics",           // the tiling's own cohesion
            "words",            // the profile's `wordConfidence`
            "pause",            // the profile's `pauses`
            "sentence",         // the profile's `sentenceType`
            "speakerChanges",   // the profile's `speaker`
            "noiseFloor",       // the profile's `forensic`
            "musicSegments",    // the profile's `forensic`
            "emphasisWords",    // the profile's `prosody`
            "discourseMarkers", // a lexicon match, rated as the fillers are
        ];
        // The families the log can actually carry, taken from the program's
        // own source rather than a list that can drift: every name in a
        // `"signals": [...]` literal and every `signals.push("...")`. Each
        // must be rateable, or the entry it appears in logs a 0 for a
        // signal that was measured.
        let mut logged: Vec<&str> = Vec::new();
        for chunk in body.split("\"signals\":").skip(1) {
            // A literal list is scanned to its close; `"signals": signals`
            // hands a variable over and says nothing here.
            let Some(list) = chunk.trim_start().strip_prefix('[') else {
                continue;
            };
            let Some(end) = list.find(']') else { continue };
            for name in list[..end].split('"').skip(1).step_by(2) {
                logged.push(name);
            }
        }
        for chunk in body.split("signals.push(").skip(1) {
            if let Some(name) = chunk
                .trim_start()
                .strip_prefix('"')
                .and_then(|rest| rest.split('"').next())
            {
                logged.push(name);
            }
        }
        logged.sort_unstable();
        logged.dedup();
        let vocabulary = crate::tools::signal_vocabulary();
        let unrated: Vec<&str> = logged
            .iter()
            .copied()
            .filter(|family| {
                // The rating pass strips a suffix at `.` or `[` before
                // mapping, so `sentence.type` is rated as `sentence`; the
                // guard does the same rather than reading it as a family
                // of its own.
                let base = family.split(['.', '[']).next().unwrap_or(family);
                let known = confidence_family(base);
                !vocabulary.contains(&base)
                    && !vocabulary.contains(&known)
                    && !RATED_ELSEWHERE.contains(&base)
            })
            .collect();
        assert!(
            !logged.is_empty(),
            "the scan found no logged signals, so it is proving nothing"
        );
        assert!(
            unrated.is_empty(),
            "these logged families would be rated 0 - no engine rates them: {unrated:?}"
        );
    }

    /// A caption's hold is sized on the rate the profile measured, and the
    /// two rates are not interchangeable: a line of long words is slow to
    /// read at the same words-per-minute, so the syllables-per-second
    /// reading wins where the two disagree. A line with neither rate gets
    /// `None`, which is the honest answer and keeps the hold on the timing.
    #[test]
    fn the_caption_hold_reads_the_rate_the_profile_measured() {
        let read = VoiceRead {
            take: "/t/a.wav".to_owned(),
            lines: vec![
                // A 2 s line at 5 syllables/s: 10 syllables, 30 chars,
                // which needs 2.0 s at 15 chars/s. The 300 wpm reading
                // implies 50 chars and 3.3 s, so words win here.
                crate::voice_read::LineRead {
                    start: 0.0,
                    end: 2.0,
                    syllables_per_sec: Some(5.0),
                    words_per_minute: Some(300.0),
                    ..Default::default()
                },
                // A 2 s line at 180 wpm: 6 words, 30 chars, 2.0 s.
                crate::voice_read::LineRead {
                    start: 2.0,
                    end: 4.0,
                    words_per_minute: Some(180.0),
                    ..Default::default()
                },
                crate::voice_read::LineRead {
                    start: 4.0,
                    end: 6.0,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let hold = read.caption_hold(&read.lines[0]).expect("a hold");
        assert!(
            (hold - (50.0 / 15.0)).abs() < 1e-9,
            "the words reading is the longer estimate and should win: {hold}"
        );
        let only_words = read.caption_hold(&read.lines[1]).expect("a hold");
        assert!(
            (only_words - 2.0).abs() < 1e-9,
            "180 wpm over 2 s is 30 chars, two reading seconds: {only_words}"
        );
        assert_eq!(
            read.caption_hold(&read.lines[2]),
            None,
            "a line with no rate must not invent a hold"
        );
    }

    /// The delivery's direction is only called when the slope is steep
    /// enough to mean one: a near-flat slope is a line that reports, and
    /// reading it as a rise would put a build where the voice did not.
    #[test]
    fn the_delivery_direction_needs_a_slope_behind_it() {
        let read = VoiceRead::default();
        let line = |slope: Option<f64>| crate::voice_read::LineRead {
            pitch_slope: slope,
            ..Default::default()
        };
        assert_eq!(read.delivery_rises(&line(Some(2.0))), Some(true));
        assert_eq!(read.delivery_rises(&line(Some(-2.0))), Some(false));
        assert_eq!(
            read.delivery_rises(&line(Some(0.2))),
            None,
            "a flat slope is not a direction"
        );
        assert_eq!(read.delivery_rises(&line(None)), None);
    }

    /// The seam lands on the take's own transient when the forensic ear
    /// measured one nearby, and the entry says so. A run with no onsets
    /// leaves the note as it was rather than claiming a transient it
    /// never found.
    #[test]
    fn the_seam_lands_on_a_measured_onset() {
        let take = TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        };
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 4.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let profile = |onsets: Value| {
            json!({
                "audio": { "path": "/t/a.wav" },
                "wholeTake": { "forensic": { "fluxOnsetsSec": onsets } },
                "lines": [{ "start": 0.0, "end": 4.0, "text": "we found the wreck" }],
                "signalConfidence": { "perSignal": [
                    { "signal": "forensic", "confidence": 0.8 },
                ]},
            })
        };
        let with = vec![view.clone(), profile(json!([2.0, 6.5]))];
        let read = voice_reads(&with, std::slice::from_ref(&take));
        assert_eq!(read[0].onset_near(6.6, 0.25), Some(6.5));
        assert_eq!(
            read[0].onset_near(4.0, 0.25),
            None,
            "no onset within the window is not an onset at the seam"
        );
        let without = vec![view, profile(json!([]))];
        let read = voice_reads(&without, &[take]);
        assert_eq!(read[0].onset_near(2.0, 0.25), None);
    }

    /// The noise character names what a denoise would target, and the
    /// flatness decides whether a denoise fits at all: a broadband floor
    /// is hiss a spectral denoise can lift, a tonal one is a hum a notch
    /// serves better. A profile that named no noise type answers nothing.
    #[test]
    fn the_noise_character_says_what_a_denoise_targets() {
        let read = |kind: &str, flatness: Option<f64>| VoiceRead {
            take_wide: crate::voice_read::TakeRead {
                noise_type: kind.to_owned(),
                spectral_flatness: flatness,
                ..Default::default()
            },
            ..Default::default()
        };
        let hiss = read("hiss", Some(0.6)).noise_character().expect("a read");
        assert!(hiss.contains("broadband"), "{hiss}");
        let hum = read("hum", Some(0.1)).noise_character().expect("a read");
        assert!(hum.contains("tonal"), "{hum}");
        let bare = read("rumble", None).noise_character().expect("a read");
        assert_eq!(bare, "rumble");
        assert_eq!(read("", Some(0.6)).noise_character(), None);
    }

    /// The brief the critic reads carries the profile's numbers, not only
    /// its family names: a critic told `noiseFloor 0.80` cannot say the bed
    /// was too loud, and a critic told `-38 dBFS` can. A family the profile
    /// never measured stays absent, so "measured quiet" and "never
    /// measured" are different briefs.
    #[test]
    fn the_brief_carries_the_measured_values() {
        let log = vec![json!({
            "wholeTake": {
                "forensic": { "noiseFloorDbfs": -38.0, "snrDb": 9.5,
                              "noiseType": "hiss", "spectralFlatness": 0.42 },
                "loudness": { "integratedLufs": -16.5 },
                "speechRatio": 0.71,
            },
            "lines": [
                { "start": 0.0, "end": 2.0, "speechRate": { "wordsPerMinute": 120.0 } },
                { "start": 2.0, "end": 4.0, "speechRate": { "wordsPerMinute": 190.0 } },
            ],
        })];
        let brief = signal_brief(&log);
        let measured = &brief["measured"];
        assert_eq!(measured["take"]["noiseFloorDbfs"], json!(-38.0));
        assert_eq!(measured["take"]["noiseType"], json!("hiss"));
        assert_eq!(measured["take"]["snrDb"], json!(9.5));
        assert_eq!(measured["take"]["integratedLufs"], json!(-16.5));
        // The per-line shape is a span, not forty rows.
        assert_eq!(measured["lines"]["wordsPerMinute"]["low"], json!(120.0));
        assert_eq!(measured["lines"]["wordsPerMinute"]["high"], json!(190.0));
        assert_eq!(measured["lines"]["wordsPerMinute"]["lines"], json!(2));
        // A reading the profile never returned stays out of the brief.
        assert!(measured["take"].get("spectralCentroidHz").is_none());
        assert!(measured["lines"].get("pitchSlopeSemitonesPerSec").is_none());
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
                { "id": "c1", "kind": "video", "media": "m1", "track": "t1",
                  "start": 0.0, "duration": 4.0 },
                { "id": "c2", "kind": "video", "media": "m2", "track": "t1",
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
            aroll: None,
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

    /// The critic is handed the run's whole reading, and every name it is
    /// handed is one `plan_validate` will accept in a decision. The
    /// profile names a family for what it measured (`pauses`,
    /// `sentenceType`); the vocabulary names it for the decision it
    /// serves. A brief that passed the producer's name through would give
    /// the critic a signal it cannot cite, and the plan it writes would
    /// fail the gate that checks the log's vocabulary.
    #[test]
    fn the_critics_brief_cites_only_names_the_validator_accepts() {
        let log = vec![json!({
            "signalConfidence": { "perSignal": [
                { "signal": "loudness", "confidence": 0.9, "basis": "ebur128" },
                { "signal": "pauses", "confidence": 0.75, "basis": "silencedetect" },
                { "signal": "sentenceType", "confidence": 0.8, "basis": "punctuation" },
                { "signal": "wordConfidence", "confidence": 0.6, "basis": "gate" },
                { "signal": "formants", "confidence": 0.55, "basis": "narrow band" },
                { "signal": "forensic", "confidence": 0.8, "basis": "spectral flux" },
                { "signal": "speaker", "confidence": 0.0, "basis": "not built" },
            ]},
        })];
        let brief = signal_brief(&log);
        let evidence = &brief["signalEvidence"];
        let measured: Vec<&str> = evidence["measured"]
            .as_array()
            .expect("the digest lists what it measured")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(
            !measured.contains(&"pauses"),
            "the producer's own name reached the brief: {measured:?}"
        );
        assert!(
            measured.contains(&"pause"),
            "it is spelled as the vocabulary spells it"
        );
        assert!(measured.contains(&"sentence"));
        assert!(measured.contains(&"words"));
        assert!(
            !measured.contains(&"speaker"),
            "a family at confidence 0 was measured by nothing and is not offered"
        );
        for name in &measured {
            assert!(
                crate::tools::signal_vocabulary_has(name),
                "{name:?} is not a name the decision log may cite"
            );
        }
        // The weak families carry the reason they are weak, and each
        // measured family names the decision it can inform.
        assert!(
            evidence["whyWeak"]["words"]
                .as_str()
                .is_some_and(|why| why.contains("gate")),
            "a weak family carries its basis: {evidence:?}"
        );
        assert_eq!(evidence["decides"]["pause"], json!(["cut", "transition"]));
        assert_eq!(
            evidence["decides"]["words"],
            json!(["caption", "graphic"]),
            "a family the plan now spends names every decision it informs"
        );
        assert_eq!(
            evidence["decides"]["forensic"],
            json!(["bgm", "repair", "mix", "grade", "sfx"]),
            "a family the plan now spends names the decision it informs"
        );
    }

    /// The profile says in its own words what it could not measure, and
    /// nothing carried that to the brief: a family missing from
    /// `measured` could be missing because the take had none or because
    /// the tool was absent, and a critic could not tell the two apart. The
    /// digest now repeats the profile's own account, deduplicated across
    /// takes, so the gap is stated rather than silently absent.
    #[test]
    fn the_brief_says_what_the_profile_could_not_measure() {
        let log = vec![
            json!({
                "audio": { "path": "/t/a.wav" },
                "degradations": [
                    "ffmpeg absent: loudness unmeasured",
                    "silence detect absent: pauses unclassified",
                ],
                "signalConfidence": { "perSignal": [
                    { "signal": "pauses", "confidence": 0.75, "basis": "silencedetect" },
                ]},
            }),
            // A second take repeating one line: the digest keeps it once.
            json!({
                "audio": { "path": "/t/b.wav" },
                "degradations": ["ffmpeg absent: loudness unmeasured"],
                "signalConfidence": { "perSignal": [
                    { "signal": "prosody", "confidence": 0.8, "basis": "pitch" },
                ]},
            }),
        ];
        let brief = signal_brief(&log);
        let gaps: Vec<&str> = brief["signalEvidence"]["notMeasured"]
            .as_array()
            .expect("the digest reports what it could not measure")
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert_eq!(
            gaps,
            vec![
                "ffmpeg absent: loudness unmeasured",
                "silence detect absent: pauses unclassified",
            ],
            "each gap once, in the order the profile gave it"
        );
    }

    /// The vocabulary is the contract and the decision map is what the
    /// critic is told each family is for. A family the program spends but
    /// the map omits is invisible in `decides`: the critic reads the
    /// signal's name and never learns which choice it drove. `fillers`
    /// was exactly that - stripped from every caption, mapped to no
    /// decision. Every family in the vocabulary must name its decision
    /// here, except the ones the program deliberately does not spend,
    /// which are held to a reason instead.
    #[test]
    fn every_vocabulary_family_names_the_decision_it_informs() {
        const NOT_SPENT: &[&str] = &["formants"];
        let unmapped: Vec<&str> = crate::tools::signal_vocabulary()
            .iter()
            .copied()
            .filter(|family| !NOT_SPENT.contains(family))
            .filter(|family| decision_for(family).is_empty())
            .collect();
        assert!(
            unmapped.is_empty(),
            "these families are spent by the edit but name no decision, so \
             the critic is never told what they drove: {unmapped:?}"
        );
        // And the mapping is not free-form: a family may only claim a
        // decision the log's own gate accepts, or the critic would be
        // pointed at a choice the validator rejects.
        for family in crate::tools::signal_vocabulary() {
            for decision in decision_for(family) {
                assert!(
                    matches!(
                        *decision,
                        "cut"
                            | "caption"
                            | "graphic"
                            | "bgm"
                            | "transition"
                            | "pacing"
                            | "repair"
                            | "mix"
                            | "grade"
                            | "sfx"
                            | "motion"
                    ),
                    "{family} claims {decision:?}, which is not a logged decision"
                );
            }
        }
        assert_eq!(decision_for("fillers"), &["caption"][..]);
        assert_eq!(decision_for("pause"), &["cut", "transition"][..]);
        assert_eq!(decision_for("topics"), &["transition", "graphic"][..]);
        assert!(decision_for("forensic").contains(&"grade"));
        assert!(decision_for("forensic").contains(&"mix"));
        assert!(decision_for("forensic").contains(&"repair"));
        assert!(decision_for("forensic").contains(&"sfx"));
    }

    /// The family-level guard above is not enough, and this is the test
    /// that proves it: `prosody` was in READ while the reading inside it
    /// that decides whether a line is *performed* or *reported* -
    /// `pitchRangeSemitones` - was parsed by `voice_read` and spent by
    /// nothing. A family can be named in the program and still have its
    /// most useful field dropped, so each field the reader parses is held
    /// here to a consumer or a stated reason.
    #[test]
    fn every_reading_inside_a_family_has_a_consumer() {
        // The fields `voice_read` parses, and the reason any that no
        // decision spends is left alone. A field with neither a consumer
        // nor an entry here fails: the profile measured it and the edit
        // threw it away.
        const NOT_SPENT: &[(&str, &str)] = &[
            (
                "cut_rule",
                "the profile's own cut-style rule is prose: `cut_style` is \
                 the reading it explains, and that one sizes every shot",
            ),
            (
                "label",
                "a phrase label is descriptive metadata; the measured start/end \
                 boundary is the timing decision, and no card is inferred from the label",
            ),
        ];
        // Both readers: the voice profile's fields and the picture's, so a
        // visual reading the gather parses and no decision spends fails the
        // same guard the audio readings fail.
        let source = concat!(
            include_str!("voice_read.rs"),
            "\n",
            include_str!("visual_read.rs"),
        );
        let program = include_str!("director_program.rs");
        let body = program
            .split("#[cfg(test)]")
            .next()
            .expect("the source has a program half");
        // Every `pub field:` the reader declares, from its struct bodies.
        let mut fields: Vec<&str> = Vec::new();
        for line in source.lines() {
            let line = line.trim();
            let Some(rest) = line.strip_prefix("pub ") else {
                continue;
            };
            let Some((name, _)) = rest.split_once(':') else {
                continue;
            };
            if name.chars().all(|c| c.is_alphanumeric() || c == '_') && !name.is_empty() {
                fields.push(name);
            }
        }
        assert!(
            fields.len() > 30,
            "the field scan found {} fields, which is too few to be the reader: \
             the parse has drifted and this test would pass vacuously",
            fields.len()
        );
        // A field is spent when the program names it directly, or when it
        // is read by a `voice_read` method the program calls: the reader
        // exists to put a helper between a raw field and the decision, and
        // `noise_floor_dbfs` reaches the bed through `wants_bed`, which is
        // a consumer even though the program never spells the field.
        let helper_reads: Vec<(&str, Vec<&str>)> = source
            .split("\n    pub fn ")
            .skip(1)
            .filter_map(|block| {
                let name = block.split('(').next()?.trim();
                // The block ends where the function does - the first line
                // that is only the method's own closing brace. Without
                // this cut the last pub fn's block would run to the end of
                // the file, and every field the private parse functions
                // (which sit after it) write would count as spent through
                // a helper whose body never names them: the guard would
                // pass for exactly the fields it exists to catch.
                let block = block.split("\n    }\n").next().unwrap_or(block);
                let uses: Vec<&str> = fields
                    .iter()
                    .copied()
                    .filter(|field| {
                        block.contains(&format!(".{field}")) || block.contains(&format!("{field}:"))
                    })
                    .collect();
                Some((name, uses))
            })
            .collect();
        let spent_through_helper = |field: &str| {
            helper_reads.iter().any(|(helper, uses)| {
                uses.contains(&field) && body.contains(&format!(".{helper}("))
            })
        };
        // Match a complete field access: .speaker_at( is a helper call,
        // not a read of a field named speaker.
        let field_access = |field: &str| {
            let needle = format!(".{field}");
            body.match_indices(&needle).any(|(at, _)| {
                let after = &body[at + needle.len()..];
                !after
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || c == '_')
            })
        };
        let unspent: Vec<&str> = fields
            .iter()
            .copied()
            .filter(|field| !NOT_SPENT.iter().any(|(name, _)| name == field))
            .filter(|field| {
                !body.contains(&format!("{field}("))
                    && !field_access(field)
                    && !spent_through_helper(field)
            })
            .collect();
        assert!(
            unspent.is_empty(),
            "these readings are parsed from the profile and spent by no \
             decision: {unspent:?} - wire each to a choice, or name it above \
             with the reason the edit is better without it"
        );
    }

    /// The brief carries the profile's own numbers, and it reads them from
    /// the paths the profile writes. A shape that drifts - a key renamed
    /// on the producer side, a count read off the wrong node - must show
    /// up here rather than as a critic that quietly had nothing to say.
    #[test]
    fn the_brief_carries_the_numbers_at_the_paths_the_profile_writes() {
        let log = vec![json!({
            "audio": { "path": "/t/a.wav" },
            "wholeTake": {
                "loudness": { "integratedLufs": -16.5 },
                "forensic": {
                    "noiseFloorDbfs": -38.0, "snrDb": 9.5, "noiseType": "hiss",
                    "spectralFlatness": 0.42, "spectralCentroidHz": 2100.0,
                    "crest": 12.0, "dynamicRangeDb": 18.0, "voicingRate": 0.61,
                    "fluxOnsetsSec": [1.0, 3.25, 7.5],
                },
                "speechRatio": 0.71,
                "dominantPeriodSec": 0.6,
                "prosody": [ { "periodSec": 0.6, "strength": 0.7 } ],
            },
            "directorHints": {
                "bgm": { "recommendation": "skip", "reason": "already musical" },
                "questions": [2, 5],
            },
            "speaker": {
                "turns": [
                    { "start": 0.0, "end": 4.5, "speaker": 0 },
                    { "start": 4.5, "end": 9.0, "speaker": 1 },
                    { "start": 9.0, "end": 12.0, "speaker": 0 },
                ],
            },
            "lines": [
                { "speechRate": { "wordsPerMinute": 140.0, "syllablesPerSec": 3.9 },
                  "loudness": { "relativeLu": 1.5 },
                  "prosody": { "pitchSlopeSemitonesPerSec": 2.0,
                               "pitchRangeSemitones": 5.0 } },
            ],
        })];
        let take = &signal_brief(&log)["measured"]["take"];
        assert_eq!(take["noiseFloorDbfs"], json!(-38.0));
        assert_eq!(take["snrDb"], json!(9.5));
        assert_eq!(take["noiseType"], json!("hiss"));
        assert_eq!(take["spectralFlatness"], json!(0.42));
        assert_eq!(
            take["crest"],
            json!(12.0),
            "crest is carried through the typed take-wide reading"
        );
        assert_eq!(
            take["fluxOnsets"],
            json!(3),
            "the count is the array's length, not a guessed rate"
        );
        assert_eq!(
            take["speakerTurns"],
            json!(3),
            "turns are counted off `/speaker/turns`"
        );
        assert_eq!(
            take["questions"],
            json!(2),
            "questions are counted off `/directorHints/questions`"
        );
        assert_eq!(
            take["bgmVerdict"],
            json!("skip"),
            "the verdict is `recommendation`, not `decision`"
        );
        let lines = &signal_brief(&log)["measured"]["lines"];
        assert_eq!(lines["wordsPerMinute"]["low"], json!(140.0));
        assert_eq!(lines["pitchSlopeSemitonesPerSec"]["high"], json!(2.0));
    }

    /// A plan built on a take whose delivery was measured names that
    /// delivery in its log, and one built on a take with no rate measured
    /// names none: the log never claims a rhythm the reading did not hear.
    #[test]
    fn the_plan_logs_the_delivery_only_where_it_was_measured() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
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
            script: None,
        };
        let pacing = |log: &[Value]| {
            let plan = auto_plan(log, &takes, &source()).expect("a plan from the read");
            plan["signalLog"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["decision"] == json!("pacing"))
                .filter(|row| {
                    row["signals"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|signal| signal == "speechRate")
                })
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

    /// The profile measures a line's pitch range and rates it against its
    /// own 6-semitone bar: wide is a line *performed*, narrow is a line
    /// *reported*. Nothing spent that reading, so a whispered confession
    /// and a read-out of a phone number were set identically. The caption
    /// now takes the difference - and a line whose range was never
    /// measured keeps the house setting rather than being guessed at.
    #[test]
    fn a_performed_line_is_set_heavier_than_a_reported_one() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        // The weight and the size, for a line whose pitch range was read
        // as `range`. `None` is a take whose prosody was never measured.
        let set_for = |range: Option<f64>| {
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
                "media": [{ "id": "m1",
                            "path": "/edits/Reel/voiceover/take-000-a.wav" }],
            });
            let transcript = json!({
                "segments": [{
                    "start": 0.5, "end": 1.5, "text": "hello there",
                    "words": [
                        { "text": "hello", "start": 0.5, "end": 0.9 },
                        { "text": "there", "start": 0.95, "end": 1.5 },
                    ],
                }],
            });
            let mut prosody = json!({});
            if let Some(range) = range {
                prosody = json!({ "pitchRangeSemitones": range });
            }
            let log = vec![
                view,
                json!({
                    "path": "/edits/Reel/voiceover/take-000-a.wav",
                    "output": transcript.to_string(),
                }),
                json!({
                    "audio": { "path": "/edits/Reel/voiceover/take-000-a.wav" },
                    "lines": [{ "start": 0.5, "end": 1.5, "prosody": prosody }],
                }),
            ];
            let call = (captions.next)(log.as_slice()).expect("the captions are placed");
            let style = &call.arguments["command"]["commands"][0]["style"];
            (
                style["fontWeight"].as_f64().expect("a weight"),
                style["fontSize"].as_f64().expect("a size"),
            )
        };
        let (wide_weight, wide_size) = set_for(Some(9.0));
        let (narrow_weight, narrow_size) = set_for(Some(2.0));
        assert!(
            wide_weight > narrow_weight,
            "a performed line should be set heavier: {wide_weight} vs {narrow_weight}"
        );
        assert!(
            wide_size > narrow_size,
            "a performed line should be set larger: {wide_size} vs {narrow_size}"
        );
        // Never measured: the house setting, and no claim either way.
        let (none_weight, none_size) = set_for(None);
        assert_eq!(
            none_weight, narrow_weight,
            "an unmeasured range leaves the weight where it was"
        );
        assert_eq!(
            none_size, narrow_size,
            "an unmeasured range leaves the size where it was"
        );
    }

    /// The mix is a logged decision, not a silent gain: the entry names
    /// `loudness`, which is the family the profile measured it in, and a
    /// take already on target logs nothing.
    #[test]
    fn the_mix_is_logged_only_where_the_loudness_was_measured() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let view = json!({
            "project": { "name": "Cut", "width": 1920.0, "height": 1080.0 },
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 3.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let source = || AutoEditProgramSource {
            project: "/t".to_owned(),
            takes: takes.clone(),
            placements: Vec::new(),
            export: "/t/out.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        let mix = |lufs: Option<f64>| {
            let read = match lufs {
                Some(lufs) => json!({
                    "audio": { "path": "/t/a.wav" },
                    "wholeTake": { "loudness": { "integratedLufs": lufs } },
                }),
                None => json!({ "audio": { "path": "/t/a.wav" } }),
            };
            let log = vec![view.clone(), read];
            let plan = auto_plan(&log, &takes, &source()).expect("a plan from the read");
            plan["signalLog"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|row| row["decision"] == json!("mix"))
                .filter(|row| {
                    row["signals"]
                        .as_array()
                        .is_some_and(|signals| signals.contains(&json!("loudness.integratedLufs")))
                })
                .cloned()
                .collect::<Vec<_>>()
        };
        let quiet = mix(Some(-22.5));
        assert_eq!(quiet.len(), 1, "a quiet take logs its gain");
        let note = quiet[0]["note"].as_str().expect("the entry says why");
        assert!(note.contains("-22.5") && note.contains("8.5"), "{note}");
        assert!(
            quiet[0]["signals"]
                .as_array()
                .expect("signals")
                .iter()
                .filter_map(Value::as_str)
                .all(crate::tools::signal_vocabulary_has)
        );
        assert!(
            mix(Some(-14.2)).is_empty(),
            "a take on target takes no gain and logs none"
        );
        assert!(mix(None).is_empty(), "an unmeasured take logs no level");
    }

    /// The accents stage steps over a peak the timeline does not play
    /// instead of ending on it: the runner reads a derive's `None` as
    /// "the stage is done", so answering `None` for one unplayed peak
    /// would drop every peak after it.
    #[test]
    fn the_accents_step_over_a_peak_the_timeline_does_not_play() {
        // Two takes whose clips play overlapping file ranges: take A
        // spans file seconds 0..10, take B 0..8 at timeline 10..18 - so
        // a bare file-second is ambiguous between the takes, and only
        // the peak's own take can place it. The 50 s peak of take A is
        // past its clip; the cursor at it must reach the 2 s peak of
        // take B (timeline 12.0) rather than end the stage - and never
        // take A's clock for take B's peak.
        let log = vec![json!({
            "media": [
                { "id": "m1", "path": "/takes/a.wav" },
                { "id": "m2", "path": "/takes/b.wav" },
            ],
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 10.0,
                  "sourceStart": 0.0, "speed": 1.0 },
                { "id": "c2", "media": "m2", "start": 10.0, "duration": 8.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
        })];
        let peaks = [
            ("/takes/a.wav".to_owned(), 1.0, 0.9),
            ("/takes/a.wav".to_owned(), 50.0, 0.8),
            ("/takes/b.wav".to_owned(), 2.0, 0.7),
        ];
        let mut placed = 1;
        assert_eq!(
            next_played_peak(&mut placed, &peaks, &log),
            Some(12.0),
            "the unplayed peak is stepped over, and take B's own clock places its peak"
        );
        assert_eq!(placed, 2, "the cursor landed on the played peak");
        // Once every peak is spent, the stage is done.
        placed = 3;
        assert_eq!(next_played_peak(&mut placed, &peaks, &log), None);
    }

    /// The critic cites the sheet's own one-based labels ("frame 4" is
    /// the tile the sheet drew as 04), and the tile array is zero-based:
    /// the fourth LABEL is the array's third tile. A fix that named
    /// frame 4 used to land one shot late - on the clip the sheet's
    /// tile 05 samples. The boundary pins it: tile 04 samples 3.5 s
    /// inside c1, tile 05 samples 4.5 s inside c2.
    #[test]
    fn the_critic_s_frame_numbers_read_the_sheet_s_own_labels() {
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
                { "where": "frame 4", "problem": "off centre",
                  "change": "shift x by 96px", "op": "shift",
                  "patch": { "dx": 96, "dy": 0 }, "confidence": 0.9 },
            ],
        })];
        let run = [vec![view.clone()], log].concat();
        let commands = revise_commands(&run);
        let shift = commands
            .iter()
            .find(|command| command["op"] == json!("setClipTransform"))
            .expect("the shift is spent");
        assert_eq!(
            shift["clipId"],
            json!("c1"),
            "the cited label 4 is the sheet's tile 04, which samples c1"
        );
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
            script: None,
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
            script: None,
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
        let timeline_ready = at("timeline-ready");
        let verify = at("verify");
        let watch = at("watch");

        // The revise reads the preview's sheet, so it follows it...
        assert!(preview < revise, "the revise answers the preview");
        // ...and corrects the timeline the program then saves and reads
        // back. The timeline is the deliverable the user keeps editing -
        // an export the run was asked to land reads the same saved
        // timeline, so a revise after the save leaves the shipped file
        // unrevised exactly the way it left the exported one.
        assert!(
            revise < timeline_ready,
            "a revise after the save lands a timeline the revise never corrected"
        );
        // The verify and the watch both read the saved timeline, so both
        // follow it.
        assert!(
            timeline_ready < verify,
            "the verify certifies the saved timeline"
        );
        assert!(
            timeline_ready < watch,
            "the watch scores the saved timeline"
        );
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
            script: None,
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

    /// The take's measured integrated loudness is spent, not just read:
    /// the profile reports LUFS, the verify stage gates on the platform
    /// target, and until this the program set no level at all, so the
    /// gate passed or failed on whatever the recording happened to be.
    #[test]
    fn the_timeline_sets_the_voice_on_the_platform_loudness_target() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(timeline) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "timeline")
            .expect("the program has a timeline stage")
        else {
            panic!("the timeline stage is derived");
        };
        let view = json!({
            "project": { "name": "Reel" },
            "project": { "name": "Cut", "width": 1920.0, "height": 1080.0 },
            "timeline": { "id": "t1", "clips": [{
                "id": "c1", "media": "m1", "start": 0.0, "duration": 6.0,
                "sourceStart": 0.0, "speed": 1.0,
            }]},
            "media": [{
                "id": "m1",
                "path": "/edits/Reel/voiceover/take-000-a.wav",
            }],
        });
        let mut log = vec![
            view.clone(),
            json!({
                "audio": { "path": "/edits/Reel/voiceover/take-000-a.wav" },
                "wholeTake": { "loudness": { "integratedLufs": -22.5 } },
                "pauses": [],
            }),
        ];
        // The tighten and the read draw first, then the grade.
        assert_eq!(
            (timeline.next)(log.as_slice()).map(|call| call.tool),
            Some("timeline_read".to_owned())
        );
        log.push(view.clone());
        let grade = (timeline.next)(log.as_slice()).expect("the look is laid over the cut");
        assert_eq!(grade.tool, "edit_apply");
        // The grade's reply is another view: the mix follows it, and the
        // gain is the measured gap to streaming's -14 LUFS.
        log.push(view.clone());
        let mix = (timeline.next)(log.as_slice()).expect("the voice is set on target");
        assert_eq!(mix.tool, "clip_audio_levels_db");
        assert_eq!(mix.arguments["clipId"], "c1");
        assert_eq!(mix.arguments["baseDb"], 8.5);
        // The mix's own reply ends the stage: one level, never a stack of
        // gains on the same clip.
        log.push(view);
        assert_eq!((timeline.next)(log.as_slice()), None);
    }

    /// The mix is only made from a measured loudness: a take the profile
    /// never read takes no gain, because an unmeasured take is not a quiet
    /// one.
    #[test]
    fn an_unmeasured_take_takes_no_loudness_gain() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(timeline) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "timeline")
            .expect("the program has a timeline stage")
        else {
            panic!("the timeline stage is derived");
        };
        let view = json!({
            "project": { "name": "Reel" },
            "project": { "name": "Cut", "width": 1920.0, "height": 1080.0 },
            "timeline": { "id": "t1", "clips": [{
                "id": "c1", "media": "m1", "start": 0.0, "duration": 6.0,
                "sourceStart": 0.0, "speed": 1.0,
            }]},
            "media": [{
                "id": "m1",
                "path": "/edits/Reel/voiceover/take-000-a.wav",
            }],
        });
        let mut log = vec![
            view.clone(),
            json!({
                "audio": { "path": "/edits/Reel/voiceover/take-000-a.wav" },
                "wholeTake": {},
                "pauses": [],
            }),
        ];
        (timeline.next)(log.as_slice());
        log.push(view.clone());
        (timeline.next)(log.as_slice());
        log.push(view);
        assert_eq!(
            (timeline.next)(log.as_slice()),
            None,
            "no measurement, no gain"
        );
    }

    /// The bed is gated on the measured floor, not only the music check:
    /// a take whose noise floor says it is already wet gets no bed
    /// stacked under it. The signal log has claimed this gate since it
    /// was written; until now the stage never read the floor, so the log
    /// asserted evidence no decision used.
    #[test]
    fn a_wet_take_is_shopped_no_bed() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: "/edits/Reel/voiceover/enhanced-000-a.wav".to_owned(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        let bed_for = |floor_dbfs: f64| {
            let mut program = auto_edit_program(&source);
            let ProgramStep::Derived(music) = program
                .steps
                .iter_mut()
                .find(|step| step.stage().label() == "music")
                .expect("the program has a music stage")
            else {
                panic!("the music stage is derived");
            };
            let log = vec![json!({
                "audio": { "path": "/edits/Reel/voiceover/take-000-a.wav" },
                "wholeTake": {
                    "forensic": { "noiseFloorDbfs": floor_dbfs },
                    "speechRatio": 0.8,
                },
            })];
            (music.next)(log.as_slice()).map(|call| call.tool)
        };
        assert_eq!(
            bed_for(-60.0),
            Some("bgm_status".to_owned()),
            "a dry take is shopped a bed"
        );
        assert_eq!(bed_for(-30.0), None, "a wet take is not");
    }

    /// The profile's own BGM verdict outranks any single field this
    /// program can read: it weighed the floor, the SNR and the spectral
    /// flatness together. A `skip` or a `duck_only` means no bed, even
    /// when the floor alone would have said the take was dry.
    #[test]
    fn the_profile_s_own_bed_verdict_is_honoured() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        let bed_for = |decision: &str| {
            let mut program = auto_edit_program(&source);
            let ProgramStep::Derived(music) = program
                .steps
                .iter_mut()
                .find(|step| step.stage().label() == "music")
                .expect("the program has a music stage")
            else {
                panic!("the music stage is derived");
            };
            // A floor dry enough to want a bed on its own, so only the
            // verdict can stop one. The verdict is `recommendation` in
            // the profile's own JSON: a fixture written against
            // `decision` passed while the reader looked for a key the
            // producer never writes.
            let log = vec![json!({
                "audio": { "path": "/edits/Reel/voiceover/take-000-a.wav" },
                "wholeTake": { "forensic": { "noiseFloorDbfs": -60.0 } },
                "directorHints": {
                    "bgm": { "recommendation": decision, "reason": "measured" },
                },
            })];
            (music.next)(log.as_slice()).map(|call| call.tool)
        };
        assert_eq!(
            bed_for("add"),
            Some("bgm_status".to_owned()),
            "the profile that wants a bed gets one"
        );
        assert_eq!(
            bed_for("skip"),
            None,
            "the profile that says skip is obeyed"
        );
        assert_eq!(
            bed_for("duck_only"),
            None,
            "a take needing a clean gets no bed stacked on it"
        );
    }

    /// A hissy take still gets its bed, but a quieter one: the SNR the
    /// forensic ear measured deepens the duck by the shortfall against the
    /// profile's own 12 dB floor. Until this the reading was parsed into
    /// `TakeRead` and spent by nothing.
    #[test]
    fn a_hissy_take_ducks_its_bed_deeper() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 3.0, "text": "we found the wreck" },
                { "start": 4.0, "end": 7.0, "text": "it was not empty" },
            ],
        });
        let view = json!({
            "timeline": { "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 8.0,
                  "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": "/t/a.wav" }],
        });
        let duck_for = |snr: Option<f64>| {
            let mut whole = json!({ "forensic": { "noiseFloorDbfs": -60.0 } });
            if let Some(snr) = snr {
                whole["forensic"]["snrDb"] = json!(snr);
            }
            let log = vec![
                json!({ "path": "/t/a.wav", "output": transcript.to_string() }),
                view.clone(),
                json!({ "lines": [
                    emotion_row(0.0, 3.0, "surprise", 0.9, "hook", "hot", ""),
                    emotion_row(4.0, 7.0, "fear", 0.9, "reveal", "cold", ""),
                ]}),
                json!({ "audio": { "path": "/t/a.wav" }, "wholeTake": whole }),
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
                    script: None,
                },
            )
            .expect("a plan from the read");
            plan.pointer("/audio/bgm/duckDb").and_then(Value::as_f64)
        };
        // The read's own duck, from the fixture's dominant line.
        let read_duck = -14.0;
        // A clean take leaves the read's own duck alone.
        assert_eq!(duck_for(Some(20.0)), Some(read_duck));
        // 6 dB of SNR is 6 dB short of the profile's 12 dB floor.
        assert_eq!(duck_for(Some(6.0)), Some(read_duck - 6.0));
        // The shortfall is capped at 6 dB so a near-silent take cannot
        // bury the bed entirely: 0 dB is 12 dB short and lands on the cap.
        assert_eq!(duck_for(Some(0.0)), Some(read_duck - 6.0));
        // An unmeasured SNR guesses nothing.
        assert_eq!(duck_for(None), Some(read_duck));
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
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
        // First call: the bed is written for a cut this short - and when
        // the model is not installed, the shelf's state. Either way, not
        // a duck of nothing.
        let first = (music.next)(log.as_slice()).expect("the bed is asked for");
        assert_eq!(first.tool, "sense_sound_model");
        assert_eq!(first.arguments["kind"], json!("bgm"));
        log.push(
            json!({ "tool": "generate", "ok": true, "model": "absent", "generated": false,
                      "reason": "the model is not downloaded" }),
        );
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
        // folder - and the bed (8 s) is longer than the cut (2 s): it is
        // trimmed to the cut before anything else, or the export would
        // ship a black, scored tail behind the last word.
        log.push(json!({
            "timeline": { "id": "t1", "clips": [
                { "id": "bed1", "media": "m9", "start": 0.0, "duration": 8.0 }
            ]},
            "media": [{ "id": "m9", "path": "/edits/Reel/bgm/track-abc123.mp3" }],
        }));
        let trim = (music.next)(log.as_slice()).expect("the bed is trimmed to the cut");
        assert_eq!(trim.tool, "edit_apply");
        assert_eq!(trim.arguments["command"]["op"], "trimClip");
        assert_eq!(trim.arguments["command"]["clipId"], "bed1");
        assert_eq!(trim.arguments["command"]["delta"], -6.0);
        // The trim's own reply is a view: the bed is read back at the
        // length the cut keeps, and ducked under the voice.
        log.push(json!({
            "timeline": { "id": "t1", "clips": [
                { "id": "bed1", "media": "m9", "start": 0.0, "duration": 2.0 }
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
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
                "words": [
                        { "text": "hello", "start": 0.5, "end": 0.9 },
                        { "text": "there", "start": 0.95, "end": 1.5 },
                    ],
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

    /// The caption groups are cut on the delivery's own seams: the
    /// sentence's punctuation closes a group, a measured breath gap
    /// closes it, four words is the cap - and the safe-remove fillers
    /// never reach the screen at all. The character offsets the accent
    /// pass patches are the group's own text's, counted in characters.
    #[test]
    fn the_caption_groups_are_cut_on_the_delivery_s_own_seams() {
        let word =
            |text: &str, start: f64, end: f64| json!({ "text": text, "start": start, "end": end });
        let segment = json!({
            "start": 0.0, "end": 10.0, "text": "one two. three um four five six seven",
            "words": [
                word("one", 0.0, 0.3), word("two.", 0.35, 0.6),
                // The breath gap: 1.4 s of silence the recogniser measured.
                word("three", 2.0, 2.3), word("um", 2.35, 2.5),
                word("four", 2.55, 2.8), word("five", 2.85, 3.1),
                word("six", 3.15, 3.4), word("seven", 3.45, 3.7),
            ],
        });
        let groups = caption_groups(&segment, &["um".to_owned()]);
        let texts: Vec<&str> = groups.iter().map(|group| group.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["one two.", "three four five six", "seven"],
            "punctuation closes, four words is the cap, the filler is dropped"
        );
        // The breath gap closed the first group before `three`.
        assert_eq!(groups[0].start, 0.0);
        assert_eq!(groups[0].end, 0.6);
        assert_eq!(groups[1].start, 2.0);
        // The character offsets are the group's own: `three` holds 0..5,
        // `four` 6..10, in the group's text.
        assert_eq!(groups[1].words[0].text, "three");
        assert_eq!(groups[1].words[0].at, 0);
        assert_eq!(groups[1].words[1].text, "four");
        assert_eq!(groups[1].words[1].at, 6);
    }

    /// The accent pass styles the words the delivery actually stressed -
    /// one `styleTextRange` per word, on the title the batch just placed -
    /// and a hot line with no stressed word accents its own longest
    /// word instead. A third ask is the stage's end.
    #[test]
    fn the_accent_pass_styles_the_stressed_words() {
        let take = "/edits/Reel/voiceover/take-000-a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
            "media": [{ "id": "m1", "path": take }],
        });
        let transcript = json!({
            "segments": [{
                "start": 0.5, "end": 1.5, "text": "hello there",
                "words": [
                    { "text": "hello", "start": 0.5, "end": 0.9 },
                    { "text": "there", "start": 0.95, "end": 1.5 },
                ],
            }],
        });
        let signals = json!({
            "audio": { "path": take },
            "lines": [{
                "start": 0.5, "end": 1.5,
                "prosody": { "emphasisWords": [
                    { "t": 0.6, "strength": 0.8, "wordIndex": 0, "wordText": "hello" },
                ]},
            }],
        });
        let mut log = vec![
            view,
            json!({ "path": take, "output": transcript.to_string() }),
            signals,
        ];
        let first = (captions.next)(log.as_slice()).expect("the groups are placed");
        assert_eq!(first.tool, "edit_apply");
        assert_eq!(
            first.arguments["command"]["commands"][0]["style"]["content"], "hello there",
            "the group's own words are the caption"
        );
        // The batch's reply: a view carrying the title the run placed.
        log.push(json!({
            "timeline": { "id": "t1", "clips": [
                { "id": "c1", "media": "m1", "start": 0.0, "duration": 4.0,
                  "sourceStart": 0.0, "speed": 1.0 },
                { "id": "title-1", "kind": "text", "start": 0.5, "duration": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": take }],
        }));
        let accents = (captions.next)(log.as_slice()).expect("the accent pass runs");
        assert_eq!(accents.tool, "edit_apply");
        let commands = accents.arguments["command"]["commands"]
            .as_array()
            .expect("one undoable accent batch");
        assert_eq!(commands.len(), 1, "one patch per stressed word");
        assert_eq!(commands[0]["op"], "styleTextRange");
        assert_eq!(commands[0]["clipId"], "title-1");
        assert_eq!(commands[0]["start"], 0, "`hello` begins the group's text");
        assert_eq!(commands[0]["end"], 5, "`hello` is five characters");
        assert_eq!(commands[0]["patch"]["color"], "#ffd400");
        // A third ask is the stage's end, whatever the log grew by.
        log.push(json!({
            "timeline": { "id": "t1", "clips": [] },
            "media": [],
        }));
        assert_eq!((captions.next)(log.as_slice()), None);
    }

    /// A line the speaker lifted above the take's own level is set
    /// heavier than a flat one: the profile measures `relativeLufs` per
    /// line, and until this the reading was named in the pacing entry and
    /// spent by nothing.
    #[test]
    fn a_lifted_line_takes_the_caption_weight() {
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: "/edits/Reel/voiceover/take-000-a.wav".to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        let weight_for = |relative_lu: f64| {
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
                "media": [{ "id": "m1",
                            "path": "/edits/Reel/voiceover/take-000-a.wav" }],
            });
            let transcript = json!({
                "segments": [{
                    "start": 0.5, "end": 1.5, "text": "hello there",
                    "words": [
                        { "text": "hello", "start": 0.5, "end": 0.9 },
                        { "text": "there", "start": 0.95, "end": 1.5 },
                    ],
                }],
            });
            let log = vec![
                view,
                json!({
                    "path": "/edits/Reel/voiceover/take-000-a.wav",
                    "output": transcript.to_string(),
                }),
                json!({
                    "audio": { "path": "/edits/Reel/voiceover/take-000-a.wav" },
                    "lines": [{ "start": 0.5, "end": 1.5,
                                "loudness": { "relativeLu": relative_lu } }],
                }),
            ];
            let call = (captions.next)(log.as_slice()).expect("the captions are placed");
            call.arguments["command"]["commands"][0]["style"]["fontWeight"]
                .as_f64()
                .expect("the caption carries a weight")
        };
        assert_eq!(
            weight_for(5.0),
            800.0,
            "a line lifted 5 LU above the take is set heavier"
        );
        assert_eq!(
            weight_for(0.5),
            600.0,
            "a line level with the take keeps the house weight"
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
            aroll: None,
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

    /// Whether the slots tile every surviving piece of the take with at
    /// most a seam uncovered per piece: the removed dead air between two
    /// pieces is the tighten's business, not a hole.
    fn slot_spans_tile(slots: &[super::BrollSlot], pieces: &[(f64, f64, f64, f64)]) -> bool {
        let sorted: Vec<(f64, f64)> = slots.iter().map(|slot| (slot.from, slot.to)).collect();
        pieces.iter().all(|(piece_from, piece_to, _, _)| {
            let mut cursor = *piece_from;
            for (from, to) in sorted
                .iter()
                .filter(|(from, to)| *to > *piece_from && *from < *piece_to)
            {
                if *from > cursor + 0.35 {
                    return false;
                }
                cursor = cursor.max(*to);
            }
            cursor >= piece_to - 0.35
        })
    }

    /// The audit's first smoking gun, answered: a voiceover-only sibling
    /// gets picture over its SPEECH, not only over its gaps. Two spoken
    /// spans and a tightened gap between them - the exact shape the old
    /// gap-only scan left black - tile end to end with slots over the
    /// words themselves.
    #[test]
    fn the_picture_floor_tiles_the_speech_itself() {
        let take = "/takes/a.wav";
        let takes = vec![TakePaths {
            take: take.to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        // The tighten already ran: the dead-air gap 2.0-4.0 is gone, and
        // the two speech pieces sit 0.0-2.0 and 2.0-4.0 on the timeline.
        let view = json!({
            "timeline": { "clips": [
                { "id": "p1", "media": "m1", "start": 0.0, "duration": 2.0,
                  "sourceStart": 0.0, "speed": 1.0 },
                { "id": "p2", "media": "m1", "start": 2.0, "duration": 2.0,
                  "sourceStart": 4.0, "speed": 1.0 }
            ]},
            "media": [{ "id": "m1", "path": take }],
        });
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "we found the city" },
                { "start": 4.0, "end": 6.0, "text": "the crowds were crossing" },
            ],
        });
        let log = vec![
            view,
            json!({ "path": take, "output": transcript.to_string() }),
        ];
        let slots = broll_slots(&log, &takes);
        assert!(
            !slots.is_empty(),
            "speech spans earn pictures even with no gap to fill"
        );
        // The tiles cover the whole SURVIVING take: no second the
        // timeline still plays is left black. (The removed dead air is
        // not a span to cover - the tighten took it.)
        let surviving: f64 = take_pieces(&log[0], &media_paths(&log[0]), take)
            .iter()
            .map(|(from, to, _, _)| to - from)
            .sum();
        let covered: f64 = slots.iter().map(|slot| slot.to - slot.from).sum();
        assert!(
            slot_spans_tile(&slots, &take_pieces(&log[0], &media_paths(&log[0]), take)),
            "the tiles cover the surviving take, not pockets of it: {covered} of {surviving}"
        );
        assert!(
            covered >= surviving - 0.35,
            "at most a seam is lost: {covered} of {surviving}"
        );
        // The words the first shot plays under are the words it shops
        // with - not the tail of a line that has not happened yet.
        assert_eq!(slots[0].query, "we found the city");
    }

    /// The audit's second smoking gun, answered: a slot bound that lands
    /// inside a span the tighten removed maps to the join the cut made,
    /// so the shot survives instead of being silently dropped.
    #[test]
    fn a_slot_bound_inside_a_removed_span_maps_to_the_join() {
        let take = "/takes/a.wav";
        // One piece survived: file 0.0-2.0 at timeline 0.0-2.0. The file
        // seconds 2.0-4.0 were cut away.
        let view = json!({
            "timeline": { "clips": [
                { "id": "p1", "media": "m1", "start": 0.0, "duration": 2.0,
                  "sourceStart": 0.0, "speed": 1.0 }
            ]},
            "media": [{ "id": "m1", "path": take }],
        });
        let media = media_paths(&view);
        // A bound inside the removed span: the exact mapping is gone, and
        // the surviving edge answers.
        assert_eq!(timeline_at(&view, &media, take, 3.0), None);
        assert_eq!(
            timeline_at_or_before(&view, &media, take, 3.0),
            Some(2.0),
            "a closing bound in the cut maps to the join"
        );
        assert_eq!(
            timeline_at_or_after(&view, &media, take, 3.0),
            None,
            "no later piece exists to open at"
        );
        // A bound before the cut maps exactly, as it always did.
        assert_eq!(timeline_at_or_before(&view, &media, take, 1.0), Some(1.0));
        assert_eq!(timeline_at_or_after(&view, &media, take, 1.0), Some(1.0));
    }

    /// The floor does not shop for what the edit already shows: a span
    /// the sibling's own A-roll covers is not tiled, and the carried
    /// footage keeps its place.
    #[test]
    fn the_floor_skips_spans_the_sibling_already_pictures() {
        let take = "/takes/a.wav";
        let takes = vec![TakePaths {
            take: take.to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        // A talking-head video beside the narration: the whole span but
        // the last second already has a picture.
        let view = json!({
            "timeline": { "clips": [
                { "id": "voice", "kind": "audio", "media": "m1", "start": 0.0,
                  "duration": 10.0, "sourceStart": 0.0, "speed": 1.0 },
                { "id": "roll", "kind": "video", "media": "m2", "start": 0.0,
                  "duration": 9.0 }
            ]},
            "media": [
                { "id": "m1", "path": take },
                { "id": "m2", "path": "/takes/talking-head.mp4" }
            ],
        });
        let transcript = json!({
            "segments": [{ "start": 0.0, "end": 9.5, "text": "we talk for a while here" }],
        });
        let log = vec![
            view,
            json!({ "path": take, "output": transcript.to_string() }),
        ];
        let slots = broll_slots(&log, &takes);
        assert!(
            slots.iter().all(|slot| slot.from >= 8.9),
            "only the unpictured second earns a shot, not the carried A-roll"
        );
    }

    /// The budget scales with the runtime: a two-minute reel's planner
    /// asks are not truncated at six, and a short piece's are not
    /// inflated to forty.
    #[test]
    fn the_planner_budget_scales_with_the_runtime() {
        let take = "/takes/a.wav";
        let takes = vec![TakePaths {
            take: take.to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let mut suggestions: Vec<Value> = Vec::new();
        for index in 0..12 {
            let from = index as f64 * 10.0;
            suggestions.push(json!({
                "kind": "dialogue_gap",
                "needBRoll": true,
                "startS": from,
                "endS": from + 6.0,
                "priority": 1.0 - index as f64 * 0.01,
                "topicTags": ["city skyline"],
            }));
        }
        let long_view = json!({
            "timeline": { "clips": [
                { "id": "voice", "media": "m1", "start": 0.0, "duration": 130.0,
                  "sourceStart": 0.0, "speed": 1.0 }
            ]},
            "media": [{ "id": "m1", "path": take }],
        });
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 8.0, "text": "we found the city at dusk" },
                { "start": 10.0, "end": 18.0, "text": "the crowds were crossing" },
            ],
        });
        let log = vec![
            long_view,
            json!({ "path": take, "output": transcript.to_string() }),
            json!({
                "take": take,
                "beatGridMeasured": false,
                "gapCount": 12,
                "suggestions": suggestions,
            }),
        ];
        let planned = slots_from_plan(&log, &takes);
        assert_eq!(
            planned.len(),
            12,
            "a 130 s runtime earns a 43-shot budget: all twelve asks fit"
        );
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
            aroll: None,
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
                aroll: None,
            },
            TakePaths {
                take: take_b.to_owned(),
                enhanced: String::new(),
                aroll: None,
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
            aroll: None,
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
            aroll: None,
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

    /// The stock page is read for the clips that fit the slot, not the
    /// clips the shelf happened to rank first: a vertical project gets
    /// vertical clips, a clip too short to cover the hole is not a
    /// candidate, and a clip already shopped is never shopped again.
    #[test]
    fn the_fitting_hits_fit_the_slot_rather_than_the_first_results() {
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
        let picked = fitting_hits(&page, 4.0, "9:16", &[], 3);
        assert_eq!(
            picked
                .iter()
                .map(|hit| hit["id"].clone())
                .collect::<Vec<_>>(),
            vec![json!("b")],
            "only the vertical clip that covers the hole is looked at"
        );

        // In a horizontal project the landscape clip is the right one.
        let picked = fitting_hits(&page, 4.0, "16:9", &[], 3);
        assert_eq!(
            picked
                .iter()
                .map(|hit| hit["id"].clone())
                .collect::<Vec<_>>(),
            vec![json!("a")],
            "the landscape clip is the one a horizontal frame looks at"
        );

        // Missing duration is not evidence that a hit can cover the hole.
        let unmeasured = hit("unknown-duration", 1080.0, 1920.0, 0.0);
        assert!(
            fitting_hits(&[unmeasured], 4.0, "9:16", &[], 3).is_empty(),
            "a hit with no measured duration cannot be promised as full coverage"
        );

        // A clip already shopped is not a candidate: the next gap takes
        // the next fit rather than repeating the shot. Unlike the old
        // single-pick, an orientation-mismatched clip is not a last
        // resort here - the sheet only looks at clips the frame will not
        // have to crop against their grain.
        let picked = fitting_hits(&page, 4.0, "9:16", &["b".to_owned()], 3);
        assert!(
            picked.is_empty(),
            "a landscape clip is not looked at for a vertical frame"
        );

        // Only a page with no licensable clip at all fills nothing.
        assert!(fitting_hits(&[], 4.0, "9:16", &[], 3).is_empty());
    }

    /// A hit without the licence metadata the import gate needs is never
    /// a candidate, whatever its shape.
    #[test]
    fn a_hit_without_a_licence_is_never_looked_at() {
        let page = vec![json!({
            "id": "x", "provider": "pexels", "url": "https://x/x",
            "width": 1080.0, "height": 1920.0, "seconds": 10.0,
        })];
        assert!(fitting_hits(&page, 3.0, "9:16", &[], 3).is_empty());
    }

    /// The sheet's own measurements reorder the page: chroma, contrast
    /// and detail for, black frames, blown ones, letterboxing and visual
    /// duplicates against - and a candidate the sheet never measured
    /// keeps its shelf order behind every measured one.
    #[test]
    fn the_visual_score_ranks_what_the_sheet_measured() {
        let candidate = |id: &str, tile: Value| StockCandidate {
            provider: "pexels".to_owned(),
            id: id.to_owned(),
            url: format!("https://x/{id}"),
            caption: format!("caption for {id}"),
            local_path: format!("/cache/{id}.mp4"),
            width: 1080.0,
            height: 1920.0,
            tile,
        };
        let vivid = json!({
            "luma": 130.0, "contrast": 55.0, "detail": 6.0,
            "blackFraction": 0.0, "blownFraction": 0.0, "barRatio": 0.0,
            "saturation": 0.5, "warmth": 0.3,
            "hash": "aaaa1111aaaa1111",
        });
        let grey = json!({
            "luma": 128.0, "contrast": 20.0, "detail": 1.0,
            "blackFraction": 0.0, "blownFraction": 0.0, "barRatio": 0.0,
            "saturation": 0.05, "warmth": 0.0,
            "hash": "bbbb2222bbbb2222",
        });
        let black = json!({
            "luma": 5.0, "contrast": 3.0, "detail": 0.0,
            "blackFraction": 0.8, "blownFraction": 0.0, "barRatio": 0.0,
            "saturation": 0.1, "warmth": -0.2,
            "hash": "cccc3333cccc3333",
        });
        let unmeasured = candidate("unmeasured", Value::Null);
        let mut ranked = [
            candidate("grey", grey.clone()),
            candidate("black", black),
            candidate("vivid", vivid.clone()),
            unmeasured,
        ];
        ranked.sort_by(|left, right| {
            let left = left.visual_score(&[]);
            let right = right.visual_score(&[]);
            match (left, right) {
                (Some(left), Some(right)) => right.total_cmp(&left),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            }
        });
        let ids: Vec<&str> = ranked.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["vivid", "grey", "unmeasured", "black"],
            "the vivid frame wins, the black one is refused, and the unmeasured keeps shelf order behind the measured"
        );

        // The warmth the sheet measured is the continuity gate's own
        // scale, and an unmeasured warmth is not a neutral zero.
        assert_eq!(ranked[0].warmth(), Some(0.3));
        assert_eq!(ranked[2].warmth(), None);

        // A frame the run already placed - same hash, different id - is
        // scored against, not refused: it stays a last resort.
        let duplicate = candidate("duplicate", {
            let mut tile = vivid.clone();
            tile["hash"] = json!("aaaa1111aaaa1111");
            tile
        });
        let fresh = candidate("fresh", {
            let mut tile = vivid.clone();
            tile["hash"] = json!("dddd4444dddd4444");
            tile
        });
        let used = vec!["aaaa1111aaaa1111".to_owned()];
        let duplicate_score = duplicate
            .visual_score(&used)
            .expect("a duplicate is still scored");
        let fresh_score = fresh.visual_score(&used).expect("a fresh frame is scored");
        assert!(
            fresh_score > duplicate_score,
            "the visually duplicate frame ranks below the fresh one"
        );
    }

    /// The crop hint's offsets are spent only on the axis the fill
    /// actually crops, and only when the source is at least as wide (or
    /// tall) as the square the hint's arithmetic assumes.
    #[test]
    fn the_reframe_follows_the_subject_only_where_the_fill_crops() {
        let tile = |x: f64, y: f64| {
            json!({
                "cropHint": { "x": x, "y": y },
                "reframeHint": { "clipTransform": { "offsetX": x, "offsetY": y } },
            })
        };
        // A landscape source into a portrait frame: the horizontal hint
        // is spent.
        assert_eq!(
            reframe_offsets(&tile(0.2, -0.1), "9:16", 1920.0, 1080.0),
            Some((0.2, 0.0))
        );
        // A portrait source into a portrait frame: nothing crops
        // horizontally, and the vertical hint is not the fill's to spend.
        assert_eq!(
            reframe_offsets(&tile(0.2, -0.1), "9:16", 1080.0, 1920.0),
            None
        );
        // A portrait source into a landscape frame: the vertical hint is
        // spent.
        assert_eq!(
            reframe_offsets(&tile(0.2, -0.15), "16:9", 1080.0, 1920.0),
            Some((0.0, -0.15))
        );
        // Dust-small offsets are not moves.
        assert_eq!(
            reframe_offsets(&tile(0.01, 0.0), "9:16", 1920.0, 1080.0),
            None
        );
        // No hint at all: nothing to spend.
        assert_eq!(reframe_offsets(&Value::Null, "9:16", 1920.0, 1080.0), None);
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
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
            "project": { "name": "Reel", "width": 1080, "height": 1920 },
            "timeline": { "clips": [
                {
                    "id": "voice",
                    "media": "voice-media",
                    "start": 0.0,
                    "duration": 4.0,
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

        // The coverage floor tiles the whole four-second take as one
        // shot - the words it plays under are the words it shops with.
        let search = (gather.next)(&log).expect("the span is searched");
        assert_eq!(search.tool, "stock_search");
        assert_eq!(search.arguments["provider"], "pexels");
        assert_eq!(
            search.arguments["query"],
            "the city skyline at dusk tonight"
        );
        assert_eq!(
            search.arguments["orientation"], "vertical",
            "the shelf is shopped for the frame the project actually is"
        );
        assert!(search.arguments.get("monetized").is_none());

        log.push(json!({
            "provider": "pexels",
            "license": "Pexels License",
            "hits": [{
                "id": "2499611",
                "provider": "pexels",
                "url": "https://videos.pexels.com/files/hd.mp4",
                "license": "Pexels License",
                "caption": "aerial city skyline at dusk",
                "width": 1080.0,
                "height": 1920.0,
                "seconds": 10.0
            }]
        }));
        // The licensed hit is looked at before it is chosen: fetched,
        // then measured by a contact sheet.
        let fetch = (gather.next)(&log).expect("the licensed hit is fetched");
        assert_eq!(fetch.tool, "stock_fetch");
        assert_eq!(fetch.arguments["provider"], "pexels");
        assert_eq!(fetch.arguments["id"], "2499611");
        log.push(json!({
            "id": "2499611", "provider": "pexels",
            "path": "/cache/stock/pexels/2499611.mp4", "bytes": 8_000_000,
        }));
        let sheet = (gather.next)(&log).expect("the fetched clip is looked at");
        assert_eq!(sheet.tool, "stock_contact_sheet");
        assert_eq!(
            sheet.arguments["paths"],
            json!(["/cache/stock/pexels/2499611.mp4"]),
            "the sheet sees the cached file"
        );
        assert_eq!(sheet.arguments["autoSetup"], false);
        log.push(json!({
            "pngPath": "/cache/sense/sheet-1.png",
            "tiles": [{
                "index": 0,
                "localPath": "/cache/stock/pexels/2499611.mp4",
                "luma": 128.0, "contrast": 40.0, "detail": 4.0,
                "blackFraction": 0.0, "blownFraction": 0.0, "barRatio": 0.0,
                "saturation": 0.4, "warmth": 0.2,
                "hash": "ab12cd34ef56ab12",
                "cropHint": { "x": 0.0, "y": 0.0 },
                "reframeHint": { "clipTransform": { "offsetX": 0.0, "offsetY": 0.0 } },
            }],
        }));
        // The picture's own read rides between the sheet and the gate:
        // the sheet looked at one tile, the read measures the frames the
        // clip actually plays.
        let visual = (gather.next)(&log).expect("the clip's picture is measured");
        assert_eq!(visual.tool, "sense_visual");
        assert_eq!(visual.arguments["action"], "measure_clips");
        assert_eq!(
            visual.arguments["paths"],
            json!(["/cache/stock/pexels/2499611.mp4"])
        );
        log.push(json!({
            "clips": [{
                "path": "/cache/stock/pexels/2499611.mp4",
                "ok": true,
                "motion": { "mean": 0.03, "peak": 0.09, "peakAt": 1.2, "still": false },
                "camera": { "kind": "pan", "cameraMoving": true,
                            "subjectMoving": false, "travel": "left" },
                "shotScale": { "scale": "wide" },
                "quality": { "verdict": "pass" },
                "saliency": { "focus": "center" },
                "scene": { "cuts": 0 },
                "visualScore": 74.0,
            }],
        }));
        // The emotion gate rides between the sheet and the choice: the
        // candidate it weighs carries the shelf's own caption (not the
        // slot's shared query), and the segment defaults to neutral - the
        // exact shape that used to crash the Python gate.
        let gate = (gather.next)(&log).expect("the sheet's choice is gated");
        assert_eq!(gate.tool, "emotion");
        assert_eq!(gate.arguments["action"], "gate");
        assert_eq!(
            gate.arguments["segment"],
            json!({ "emotion": "neutral", "act": "setup" }),
            "no emotion line covers the gap, so the segment is the neutral default"
        );
        assert_eq!(
            gate.arguments["candidates"],
            json!([{ "index": 0, "id": "2499611", "caption": "aerial city skyline at dusk" }]),
            "the gate judges the shelf's own words for the clip"
        );
        log.push(json!({ "ok": true, "order": [0] }));
        let import = (gather.next)(&log).expect("the gate-passed hit is imported");
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
        // The lane every B-roll clip shares, minted once before the
        // first placement: a seam's transition only renders between
        // same-lane neighbours, so the shots ride one dedicated lane
        // rather than the first-free walk.
        let lane = (gather.next)(&log).expect("the shared B-roll lane is minted");
        assert_eq!(lane.tool, "edit_apply");
        assert_eq!(lane.arguments["command"]["op"], "addTrack");
        log.push(json!({ "createdId": "t-broll" }));
        let place = (gather.next)(&log).expect("the clip is placed on the shared lane");
        assert_eq!(place.tool, "edit_apply");
        assert_eq!(place.arguments["command"]["op"], "addClip");
        assert_eq!(place.arguments["command"]["mediaId"], "stock-media");
        assert_eq!(place.arguments["command"]["trackId"], "t-broll");
        assert_eq!(
            place.arguments["command"]["start"], 0.0,
            "the coverage tile opens at the take's start"
        );

        log.push(json!({
            "timeline": { "clips": [
                {
                    "id": "voice",
                    "media": "voice-media",
                    "start": 0.0,
                    "duration": 4.0,
                    "sourceStart": 0.0,
                    "speed": 1.0
                },
                {
                    "id": "stock-clip",
                    "media": "stock-media",
                    "start": 0.0,
                    "duration": 10.0
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
            "one span is placed only once"
        );
    }

    /// The gate's two failure shapes behave differently: an ANSWERED
    /// gate that rejected every candidate drops the beat (never pads),
    /// while a gate that never answered (a refusal shape in the log)
    /// degrades to the sheet's own ranking rather than dropping work on
    /// a technicality.
    #[test]
    fn the_gate_s_rejection_drops_and_its_degradation_falls_back() {
        use concat_director::ProgramStep;

        let take = "/takes/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: "/takes/a-enhanced.wav".to_owned(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
            "project": { "name": "Reel", "width": 1080, "height": 1920 },
            "timeline": { "clips": [{
                "id": "voice", "media": "voice-media",
                "start": 0.0, "duration": 4.0,
                "sourceStart": 0.0, "speed": 1.0,
            }]},
            "media": [{ "id": "voice-media", "path": take }],
        });
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 2.0, "text": "the city skyline at dusk tonight" },
                { "start": 4.0, "end": 6.0, "text": "the crowds crossing" },
            ],
        });
        let base = |log: Vec<Value>| -> Vec<Value> {
            let mut log = log;
            log.insert(0, view.clone());
            log.insert(1, json!({ "path": take, "output": transcript.to_string() }));
            log
        };
        let search_reply = json!({
            "provider": "pexels",
            "license": "Pexels License",
            "hits": [{
                "id": "2499611", "provider": "pexels",
                "url": "https://videos.pexels.com/files/hd.mp4",
                "license": "Pexels License",
                "caption": "aerial city skyline at dusk",
                "width": 1080.0, "height": 1920.0, "seconds": 10.0,
            }],
        });
        let fetch_reply = json!({
            "id": "2499611", "provider": "pexels",
            "path": "/cache/stock/pexels/2499611.mp4", "bytes": 8_000_000,
        });
        let sheet_reply = json!({
            "pngPath": "/cache/sense/sheet-1.png",
            "tiles": [{
                "index": 0,
                "localPath": "/cache/stock/pexels/2499611.mp4",
                "luma": 128.0, "contrast": 40.0, "detail": 4.0,
                "blackFraction": 0.0, "blownFraction": 0.0, "barRatio": 0.0,
                "saturation": 0.4, "warmth": 0.2,
                "hash": "ab12cd34ef56ab12",
                "cropHint": { "x": 0.0, "y": 0.0 },
                "reframeHint": { "clipTransform": { "offsetX": 0.0, "offsetY": 0.0 } },
            }],
        });
        let visual_reply = json!({
            "clips": [{
                "path": "/cache/stock/pexels/2499611.mp4",
                "ok": true,
                "motion": { "mean": 0.03, "peak": 0.09, "peakAt": 1.2, "still": false },
                "camera": { "kind": "pan", "cameraMoving": true,
                            "subjectMoving": false, "travel": "left" },
                "shotScale": { "scale": "wide" },
                "quality": { "verdict": "pass" },
                "saliency": { "focus": "center" },
                "scene": { "cuts": 0 },
                "visualScore": 74.0,
            }],
        });
        let drive_to_gate =
            |log: &mut Vec<Value>,
             gather: &mut (dyn FnMut(&[Value]) -> Option<ToolCall> + Send)| {
                let _ = (gather)(log); // stock_search
                log.push(search_reply.clone());
                let _ = (gather)(log); // stock_fetch
                log.push(fetch_reply.clone());
                let _ = (gather)(log); // stock_contact_sheet
                log.push(sheet_reply.clone());
                let visual = (gather)(log).expect("the clip's picture is measured");
                assert_eq!(visual.tool, "sense_visual");
                assert_eq!(visual.arguments["action"], "measure_clips");
                log.push(visual_reply.clone());
                let gate = (gather)(log).expect("the gate is asked");
                assert_eq!(gate.tool, "emotion");
            };

        // An answered gate that rejected every candidate: the beat is
        // dropped, nothing is imported for it, and - this take tiles
        // into exactly one coverage shot - the stage ends.
        let mut log = base(vec![]);
        let step = gather;
        {
            let gather = &mut *step.next;
            drive_to_gate(&mut log, gather);
        }
        log.push(json!({ "ok": true, "order": [] }));
        assert!(
            (step.next)(&log).is_none(),
            "a rejected beat is dropped, not imported, and the stage ends"
        );

        // A gate that never answered: the sheet's ranking decides among
        // the visually qualified candidates. A fresh program - the
        // closure above spent its one slot.
        let mut program = auto_edit_program(&source);
        let ProgramStep::Derived(gather) = program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "gather")
            .expect("the program has a gather stage")
        else {
            panic!("the gather stage is derived");
        };
        let mut log = base(vec![]);
        {
            let gather = &mut *gather.next;
            drive_to_gate(&mut log, gather);
        }
        log.push(json!({ "ok": false, "error": "the hosted model is unreachable" }));
        let import = (gather.next)(&log).expect("the degraded gate falls back to the sheet");
        assert_eq!(import.tool, "stock_import");
        assert_eq!(import.arguments["id"], "2499611");
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
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
                "duration": 4.0,
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
        let search = (gather.next)(&log).expect("the span is searched");
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
        let targets = entrance_targets(&log, &[]);
        assert_eq!(
            targets,
            vec![("t1".to_owned(), 2.0, 0.0, default_ease())],
            "only the picture-free title long enough to hold an entrance, on the settle a line was never read for"
        );

        // A caption over a measured take reads the line that plays under
        // it - through the timeline, not a media field a text clip has
        // never carried. The take sits at 0.0 playing file seconds
        // 4.0..8.0; a caption starting at timeline 1.5 lands on the tense
        // line at file second 5.5 and takes ITS curve, never the default.
        let view = json!({
            "timeline": { "clips": [
                { "id": "v1", "media": "m1", "start": 0.0, "duration": 6.0,
                  "sourceStart": 4.0, "speed": 1.0 },
                { "id": "c1", "kind": "text", "start": 1.5, "duration": 1.0 }
            ]},
            "media": [ { "id": "m1", "path": "/takes/a.wav" } ],
        });
        let mut row = emotion_row(4.0, 7.0, "tense", 0.8, "middle", "warm", "riser");
        row["easing"] = json!({
            "curve": { "kind": "bezier", "x1": 0.7, "y1": 0.0, "x2": 0.84, "y2": 0.0 }
        });
        let read = json!({ "audio": { "path": "/takes/a.wav" }, "lines": [row] });
        // The transcript the emotion read answers: one timed line inside
        // the take, so the take counts as asked and the reply zips to it.
        let transcript = json!({
            "path": "/takes/a.wav",
            "output": serde_json::to_string(&json!({
                "segments": [
                    { "text": "you will not believe what happened next",
                      "start": 4.0, "end": 7.0 }
                ]
            })).unwrap()
        });
        let log = vec![transcript, read, view];
        let takes = vec![TakePaths {
            take: "/takes/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
        }];
        let targets = entrance_targets(&log, &takes);
        let ease = targets
            .iter()
            .find(|(id, _, _, _)| id == "c1")
            .map(|(_, _, _, ease)| ease.clone())
            .expect("the caption over the take is keyed");
        assert_ne!(
            ease,
            default_ease(),
            "a caption over a measured line takes the line's own curve"
        );

        // Two seconds of title: the entrance settles within the first
        // 0.35 s, a fraction of the title's own span.
        let keys = entrance_keys("t1", 2.0, 0.0, &default_ease());
        assert_eq!(keys.len(), 4);
        assert_eq!(keys[0]["property"], "scale");
        assert_eq!(keys[0]["at"], 0.0);
        assert_eq!(keys[0]["value"], 0.94);
        assert_eq!(keys[1]["at"], 0.175);
        assert_eq!(keys[1]["value"], 1.0);
        assert_eq!(keys[2]["property"], "offsetY");
        assert_eq!(keys[3]["value"], 0.0);

        // A placed caption rises around its own placement, not around
        // zero: a lower-third stays a lower-third while it settles.
        let placed = entrance_keys("t1", 2.0, 0.22, &default_ease());
        assert_eq!(placed[2]["value"], 0.27);
        assert_eq!(placed[3]["value"], 0.22);

        // A very short title still settles inside its span, not past it.
        let keys = entrance_keys("t", 0.5, 0.0, &default_ease());
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
            // The engine's own rating: the emotion read answers one per
            // line, and the signal log rates an `emotion` entry by it.
            "confidence": 0.85,
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
                    aroll: None,
                },
                TakePaths {
                    take: "/edits/Reel/voiceover/b.wav".to_owned(),
                    enhanced: String::new(),
                    aroll: None,
                },
            ],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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

    /// A foreign reply may carry words-per-minute without the producer's
    /// pace class; the same 180/115 bars still turn it into a beat.
    #[test]
    fn a_rate_without_a_pace_class_still_sets_the_beat() {
        let log = vec![json!({
            "audio": { "path": "/t/a.wav" },
            "lines": [
                { "start": 0.0, "end": 2.0, "speechRate": { "wordsPerMinute": 200.0 } },
                { "start": 2.0, "end": 4.0, "speechRate": { "wordsPerMinute": 100.0 } },
            ],
        })];
        assert_eq!(delivery_pace(&log, "/t/a.wav", 0.0, 2.0), "punchy");
        assert_eq!(delivery_pace(&log, "/t/a.wav", 2.0, 4.0), "slow");
        assert_eq!(pace_from_wpm(140.0), "conversational");
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
            pitch_range: None,
            pitch_lift: None,
            earned_by: "emotion",
            intensity: 0.5,
            pulse: None,
            motion_speed: None,
            chapter: false,
            slot: None,
            slots: Vec::new(),
        };
        assert!(card("punchy").hold() < card("").hold());
        assert!(card("slow").hold() > card("").hold());
    }

    /// The emotional read's intensity and the take's pulse both shape a
    /// card's hold: a hard beat stays longer, and a pulsed take rounds
    /// the hold onto its own beat rather than the frame grid.
    #[test]
    fn the_arc_and_pulse_shape_a_cards_hold() {
        let card = |intensity: f64, pulse: Option<f64>| MotionSpec {
            take: "/t/a.wav".to_owned(),
            at: 0.0,
            end: 4.0,
            preset: "kinetic.title",
            words: "the wreck".to_owned(),
            act: "reveal".to_owned(),
            pace: String::new(),
            pitch_range: None,
            pitch_lift: None,
            earned_by: "emotion",
            intensity,
            pulse,
            motion_speed: None,
            chapter: false,
            slot: None,
            slots: Vec::new(),
        };
        assert!(
            card(0.9, None).hold() > card(0.1, None).hold(),
            "an intense beat holds longer than a calm one"
        );
        let pulsed = card(0.5, Some(0.5)).hold();
        assert!(
            (pulsed / 0.5).fract().abs() < 1.0e-6,
            "a pulsed hold lands on the beat, got {pulsed}"
        );
    }

    /// An entity is a stock subject only when it names something a camera
    /// can show, and only when the NER was sure: a number is a stat
    /// card's job, and a low-confidence reading is not a subject to shop.
    #[test]
    fn the_entity_stock_query_shops_names_not_numbers() {
        let entity = |kind: &str, text: &str, confidence: Option<f64>| Entity {
            take: "/t/a.wav".to_owned(),
            kind: kind.to_owned(),
            text: text.to_owned(),
            card: String::new(),
            at: Some(1.0),
            approximate: false,
            confidence,
            value: None,
            unit: None,
        };
        assert_eq!(
            entity_stock_query(&entity("ORG", "Acme", Some(0.9))),
            Some("Acme company".to_owned())
        );
        assert_eq!(
            entity_stock_query(&entity("PER", "Ada Lovelace", Some(0.9))),
            Some("Ada Lovelace portrait".to_owned())
        );
        assert_eq!(
            entity_stock_query(&entity("DATE", "1972", Some(0.9))),
            Some("1972 archive".to_owned())
        );
        assert_eq!(
            entity_stock_query(&entity("MONEY", "$10,450", Some(0.9))),
            None,
            "a number is a stat card, not footage"
        );
        assert_eq!(
            entity_stock_query(&entity("ORG", "Acme", Some(0.4))),
            None,
            "an unsure reading is not a subject"
        );
    }

    /// A motion card is vetoed by the same readings that veto a caption:
    /// a hoarse voice, a line with almost no voice under it, or a line
    /// the confidence gate could not hear. An unmeasured line is not
    /// vetoed - only a reading that says the line is not what the
    /// transcript thinks refuses the card.
    #[test]
    fn a_motion_card_is_vetoed_by_an_unreliable_line() {
        let read = |line: LineRead| VoiceRead {
            take: "/t/a.wav".to_owned(),
            lines: vec![line],
            ..Default::default()
        };
        let clean = LineRead {
            start: 0.0,
            end: 2.0,
            hnr_db: Some(24.0),
            jitter: Some(0.01),
            shimmer: Some(0.01),
            voiced_ratio: Some(0.9),
            ..Default::default()
        };
        assert!(motion_line_reliable(&[read(clean)], "/t/a.wav", 1.0));
        let hoarse = LineRead {
            start: 0.0,
            end: 2.0,
            jitter: Some(0.12),
            ..Default::default()
        };
        assert!(!motion_line_reliable(&[read(hoarse)], "/t/a.wav", 1.0));
        let unvoiced = LineRead {
            start: 0.0,
            end: 2.0,
            voiced_ratio: Some(0.05),
            ..Default::default()
        };
        assert!(!motion_line_reliable(&[read(unvoiced)], "/t/a.wav", 1.0));
        let unmeasured = LineRead {
            start: 0.0,
            end: 2.0,
            ..Default::default()
        };
        assert!(motion_line_reliable(&[read(unmeasured)], "/t/a.wav", 1.0));
    }

    /// A coverage bound snaps to a stressed word when no stronger
    /// structural moment is near: the delivery's own beat is a better cut
    /// than the cadence's arbitrary frame.
    #[test]
    fn a_coverage_bound_snaps_to_a_stressed_word() {
        let bounds = SnapBounds {
            emphasis: vec![3.0],
            ..Default::default()
        };
        assert_eq!(bounds.snap(3.1, 0.0, 0.8, 0.3), Some(3.0));
    }

    /// A card opens on the delivery's stressed word, not a fixed offset
    /// from the line start, and a performed or lifted line holds it a
    /// beat longer.
    #[test]
    fn a_motion_card_opens_on_the_stressed_word() {
        let line = EmotionLine {
            take: "/t/a.wav".to_owned(),
            start: 0.0,
            end: 4.0,
            emotion: "neutral".to_owned(),
            intensity: 0.8,
            act: "reveal".to_owned(),
            temperature: "balanced".to_owned(),
            duck_db: -18.0,
            sfx_type: String::new(),
            bgm_words: Vec::new(),
            queries: Vec::new(),
            easing: default_ease(),
            text: "we found the wreck".to_owned(),
            pace: "conversational".to_owned(),
            confidence: None,
        };
        let mut read = VoiceRead {
            take: "/t/a.wav".to_owned(),
            ..Default::default()
        };
        read.take_wide.mean_f0_hz = Some(120.0);
        read.lines.push(LineRead {
            start: 0.0,
            end: 4.0,
            text: "we found the wreck".to_owned(),
            pitch_range: Some(8.0),
            pitch_mean_hz: Some(180.0),
            emphasis: vec![crate::voice_read::Emphasis {
                word: "wreck".to_owned(),
                at: 2.5,
                strength: 0.9,
            }],
            ..LineRead::default()
        });
        let specs = motion_specs(&[], &[line], &[read], &[], &[], &[], &[], &[], false);
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].at, 2.5, "the card opens on the stressed word");
        assert_eq!(specs[0].pitch_range, Some(8.0));
        assert!(
            specs[0].hold() > 1.35,
            "a performed, lifted line holds longer: {}",
            specs[0].hold()
        );
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
                queries: Vec::new(),
                easing: default_ease(),
                text: String::new(),
                pace: String::new(),
                confidence: None,
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
                queries: Vec::new(),
                easing: default_ease(),
                text: String::new(),
                pace: String::new(),
                confidence: None,
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
        assert_eq!(transition_for_emotion("joy"), "concat.zoom-blur-transition");
        assert_eq!(alternate_transition("joy"), "concat.cross-pop");
        // Every transition the program can name is a package the effects
        // catalogue actually ships: a look-up id is a transition that
        // silently never applies. And it must be a TRANSITION-kind
        // package - the export resolver's catalogue arm requires a
        // `[transition]` section, and an effect-kind id (the plain
        // zoom-blur family) falls through to a plain cut, the move
        // silently lost. This is the pin that keeps the joy and sadness
        // seams from ever drifting back to the effect-kind neighbours.
        let catalogue =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../concat-effects/packages");
        for id in [
            transition_for_emotion("joy"),
            transition_for_emotion("surprise"),
            transition_for_emotion("anger"),
            transition_for_emotion("fear"),
            transition_for_emotion("sadness"),
            transition_for_emotion("disgust"),
            transition_for_emotion("neutral"),
            alternate_transition("joy"),
            alternate_transition("surprise"),
            alternate_transition("anger"),
            alternate_transition("fear"),
            alternate_transition("sadness"),
            alternate_transition("disgust"),
            alternate_transition("neutral"),
            "concat.whip-pan",
            "concat.slice-social",
            "concat.glitch-social",
            "concat.snap-zoom",
            "concat.flash-social",
            "concat.cross-pop",
            "concat.dissolve",
        ] {
            assert!(
                catalogue.join(id).is_dir(),
                "the transition `{id}` is not a package the catalogue ships"
            );
            let manifest = std::fs::read_to_string(catalogue.join(id).join("effect.toml"))
                .unwrap_or_else(|error| {
                    panic!("the transition `{id}` has no effect.toml: {error}")
                });
            assert!(
                manifest.contains("kind = \"transition\""),
                "the transition `{id}` is not transition-kind: the export resolver falls it through to a plain cut"
            );
            assert!(
                manifest.contains("[transition]"),
                "the transition `{id}` carries no [transition] section: the resolver's catalogue arm requires one"
            );
        }
    }

    /// The no-repeat rule holds against the *seam* moves too, not only
    /// the mood's: a chapter opening's whip-pan is the mood's own
    /// alternate on a neutral read, so the old fallback wore whip-pan
    /// twice running. The fix falls back to the mood's primary move,
    /// which the alternate never equals (the test above pins that).
    #[test]
    fn a_seam_never_repeats_the_last_shot_s_move() {
        let slot = BrollSlot {
            take: "take.wav".into(),
            from: 5.0,
            to: 6.0,
            query: "city".into(),
        };
        let chapter = TopicBoundary {
            take: "take.wav".into(),
            at: 5.5,
            headline: "the turn".into(),
            confidence: Some(0.8),
        };
        let first = seam_transition(&[], std::slice::from_ref(&chapter), &[], &[], &slot, None);
        assert_eq!(first, "concat.whip-pan");
        let second = seam_transition(&[], &[chapter], &[], &[], &slot, Some(first));
        assert_ne!(second, first, "a chapter seam repeated its own move");
        assert_eq!(second, "concat.blur-dissolve");
    }

    /// The seam spends the pause class and the sentence shape, not only
    /// the presence of a gap: a scene turn earns a visible move, an
    /// exclamation a hard one, a fragment a light one.
    #[test]
    fn the_seam_spends_the_pause_class_and_sentence_type() {
        let slot = BrollSlot {
            take: "take.wav".into(),
            from: 5.0,
            to: 6.0,
            query: "city".into(),
        };
        let mut read = VoiceRead {
            take: "take.wav".into(),
            ..Default::default()
        };
        read.lines.push(LineRead {
            start: 4.0,
            end: 5.5,
            text: "we found it".into(),
            sentence_type: "exclamation".into(),
            ..LineRead::default()
        });
        read.pauses.push(crate::voice_read::Pause {
            start: 5.0,
            end: 6.0,
            class: "scene_turn".into(),
            keep: true,
        });
        assert_eq!(
            seam_transition(&[read.clone()], &[], &[], &[], &slot, None),
            "concat.whip-pan",
            "a scene turn is the strongest pause class"
        );
        read.pauses[0].class = "breath".into();
        assert_eq!(
            seam_transition(&[read.clone()], &[], &[], &[], &slot, None),
            "concat.crash-zoom",
            "an exclamation lands hard even over a breath"
        );
        read.lines[0].sentence_type = "fragment".into();
        assert_eq!(
            seam_transition(&[read], &[], &[], &[], &slot, None),
            "concat.cross-pop",
            "a fragment trails off"
        );
    }

    /// A parsed entity value is what the card shows: the viewer reads
    /// `$10,450`, `45%` or `2026`, not the raw words the ear happened to
    /// hear. An unparsed entity keeps its text.
    #[test]
    fn an_entity_card_formats_the_parsed_value_and_unit() {
        let entity = |text: &str, value: Option<f64>, unit: Option<&str>| Entity {
            take: "take.wav".into(),
            kind: "MONEY".into(),
            text: text.into(),
            card: "stat".into(),
            at: Some(2.0),
            approximate: false,
            confidence: Some(0.9),
            value,
            unit: unit.map(str::to_owned),
        };
        let money = comp_for_entity(&entity("$10,450", Some(10450.0), Some("USD")));
        assert_eq!(money.expect("a stat card").1, "$10,450");
        let percent = comp_for_entity(&entity("45%", Some(45.0), Some("percent")));
        assert_eq!(percent.expect("a stat card").1, "45%");
        let year = comp_for_entity(&entity("2026", Some(2026.0), Some("year")));
        assert_eq!(year.expect("a stat card").1, "2026");
        let count = comp_for_entity(&entity("1,500", Some(1500.0), Some("count")));
        assert_eq!(count.expect("a stat card").1, "1,500");
        let raw = comp_for_entity(&entity("a great many", None, None));
        assert_eq!(raw.expect("a stat card").1, "a great many");
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
            queries: Vec::new(),
            easing: default_ease(),
            text: String::new(),
            pace: String::new(),
            confidence: None,
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

    /// The SFX stage reads the exact same typed emphasis values as the
    /// caption and pacing consumers; a producer key rename cannot leave
    /// the accents silently reading an obsolete raw JSON path.
    #[test]
    fn sfx_emphasis_peaks_come_from_the_typed_voice_read() {
        let reads = vec![VoiceRead {
            take: "/t/a.wav".to_owned(),
            lines: vec![crate::voice_read::LineRead {
                emphasis: vec![crate::voice_read::Emphasis {
                    word: "launch".to_owned(),
                    at: 2.5,
                    strength: 0.85,
                }],
                ..Default::default()
            }],
            ..Default::default()
        }];
        assert_eq!(
            emphasis_peaks(&reads),
            vec![("/t/a.wav".to_owned(), 2.5, 0.85)]
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
            queries: Vec::new(),
            easing: default_ease(),
            text: String::new(),
            pace: String::new(),
            confidence: None,
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
            aroll: None,
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

    /// A talking-head's carried A-roll is cut in lockstep with the take
    /// its audio was extracted from, and the carried cutaways move with
    /// the lanes: the tighten ripples only the lane it cuts, so without
    /// this the picture would drift right of the words by every removed
    /// pause.
    #[test]
    fn the_tighten_cuts_the_a_roll_in_lockstep_and_moves_the_cutaways() {
        let take = "/edits/Reel edit/voiceover/take-000-head.wav";
        let aroll = "/edits/Reel edit/footage/footage-000-head.mp4";
        let takes = vec![TakePaths {
            take: take.to_owned(),
            enhanced: String::new(),
            aroll: Some(aroll.to_owned()),
        }];
        let view = json!({
            "timeline": { "clips": [
                { "id": "words", "kind": "audio", "media": "m-take", "start": 0.0,
                  "duration": 10.0, "sourceStart": 0.0, "speed": 1.0 },
                { "id": "face", "kind": "video", "media": "m-roll", "start": 0.0,
                  "duration": 10.0, "sourceStart": 0.0, "speed": 1.0 },
                { "id": "cutaway", "kind": "image", "media": "m-still", "start": 8.0,
                  "duration": 2.0 },
            ]},
            "media": [
                { "id": "m-take", "path": take },
                { "id": "m-roll", "path": aroll },
                { "id": "m-still", "path": "/edits/Reel edit/footage/footage-001-still.png" },
            ],
        });
        let log = vec![
            view,
            json!({
                "audio": { "path": take },
                "pauses": [{ "start": 4.0, "end": 5.5, "class": "dead_air" }],
                "lines": [],
            }),
            json!({
                "lines": [emotion_row(0.0, 3.5, "neutral", 0.2, "setup", "balanced", "")],
            }),
        ];
        let cuts = dead_air_cuts(&log, &takes, "/edits/Reel edit", &[]);
        // Three edits: the words cut, the face cut in lockstep, the
        // cutaway moved.
        assert_eq!(cuts.len(), 3, "{cuts:#?}");
        assert_eq!(cuts[0].arguments["command"]["op"], json!("removeSpans"));
        assert_eq!(cuts[0].arguments["command"]["clipId"], json!("words"));
        assert_eq!(
            cuts[0].arguments["command"]["spans"],
            json!([[4.08, 5.42]]),
            "the pause, padded the breath rule's 80 ms a side"
        );
        // The A-roll takes the SAME spans: the same file clock, mapped
        // through its own span.
        assert_eq!(cuts[1].arguments["command"]["op"], json!("removeSpans"));
        assert_eq!(cuts[1].arguments["command"]["clipId"], json!("face"));
        assert_eq!(cuts[1].arguments["command"]["spans"], json!([[4.08, 5.42]]));
        // The cutaway slides left by the removed 1.34 s.
        assert_eq!(cuts[2].arguments["command"]["op"], json!("moveClips"));
        let moves = cuts[2].arguments["command"]["moves"]
            .as_array()
            .expect("the move list");
        assert_eq!(moves.len(), 1);
        assert_eq!(moves[0]["clipId"], json!("cutaway"));
        assert!((moves[0]["start"].as_f64().unwrap() - (8.0 - 1.34)).abs() < 0.011);
    }

    /// The bed is shopped by the read's own words and ducked to the level
    /// its act profile sets - not to a fixed -20 dB.
    #[test]
    fn the_bed_follows_the_read_s_words_and_level() {
        let takes = vec![TakePaths {
            take: "/t/a.wav".to_owned(),
            enhanced: String::new(),
            aroll: None,
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
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
            view.clone(),
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
                      "prosody": { "emphasisWords": [{ "word": "found", "t": 1.0, "strength": 0.9 }] } },
                    { "start": 5.0, "end": 9.0,
                      "prosody": { "emphasisWords": [{ "word": "desk", "t": 6.0, "strength": 0.8 }] } },
                ],
            }),
            json!({
                "lines": [
                    emotion_row(0.0, 4.0, "anger", 0.9, "tension", "hot", "impact-heavy (hard hits, distorted risers)"),
                    emotion_row(5.0, 9.0, "neutral", 0.2, "setup", "balanced", ""),
                ],
            }),
        ];
        // The loud peak is inside the first three seconds: the hook asks
        // the heavier generator for its accent.
        let first = (sfx.next)(&log).expect("the loud peak is generated");
        assert_eq!(first.tool, "sense_sound_model");
        assert_eq!(first.arguments["kind"], json!("sfx-large"));
        // The model is not installed: the shelf takes the accent instead,
        // and every peak after this one is the shelf's too.
        let mut after = log.clone();
        after.push(json!({ "tool": "generate", "ok": true, "model": "absent",
                          "generated": false }));
        let shop = (sfx.next)(&after).expect("the shelf answers when the model is absent");
        assert_eq!(shop.tool, "sounds_search");
        // The calm line's peak is skipped: the shelf's answer for the loud
        // one is the last call, so the next ask is the add, and once that
        // is spent the stage ends rather than shopping the calm peak.
        after.push(json!({ "sounds": [{ "id": "s1" }] }));
        let add = (sfx.next)(&after).expect("the sound is added");
        assert_eq!(add.tool, "sounds_add");
        // The placement is held against the file before it is gained: the
        // check measures the copy the timeline will actually play, so a
        // manifest `syncAt` that drifted is caught here, not heard.
        after.push(json!({ "mediaIds": ["m2"], "clipId": "clip-9",
                          "file": "/edits/Reel/audio/s1.ogg" }));
        let check = (sfx.next)(&after).expect("the placement is checked");
        assert_eq!(check.tool, "sense_sound");
        assert_eq!(check.arguments["action"], json!("check_placement"));
        assert_eq!(check.arguments["role"], json!("action_impact"));
        assert_eq!(check.arguments["intendedAt"], json!(1.0));
        // As directed: the accent is gained the moment it stands - an add
        // left at the source's unity would sum past full scale under the
        // voice's own loudest moment.
        after.push(json!({
            "ok": true, "issues": [],
            "measured": { "moveBy": 0.0 },
        }));
        let gain = (sfx.next)(&after).expect("the accent is gained");
        assert_eq!(gain.tool, "clip_audio_levels_db");
        assert_eq!(gain.arguments["clipId"], json!("clip-9"));
        assert_eq!(gain.arguments["baseDb"], json!(accent_gain_db(0.9)));
        after.push(view.clone());
        assert_eq!(
            (sfx.next)(&after),
            None,
            "the calm line's peak never earns an accent"
        );
    }

    /// The placement the check answers is the one the run keeps: a peak
    /// that measures off the moment is moved onto it, a file that measures
    /// as another family's sound comes off the lane and the family's next
    /// word shops the peak again, and a check that cannot answer leaves
    /// the placement as laid rather than pretending it verified one.
    #[test]
    fn the_placement_is_measured_corrected_or_dropped() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
        };
        // One fresh program per scenario: the derived step keeps its own
        // rotation and spent ids, and a second scenario must not inherit
        // the first one's shopping.
        let view = json!({
            "timeline": { "clips": [{
                "id": "c1", "media": "m1", "start": 0.0, "duration": 20.0,
                "sourceStart": 0.0, "speed": 1.0,
            }]},
            "media": [{ "id": "m1", "path": take }],
        });
        let log = vec![
            view.clone(),
            json!({
                "path": take,
                "output": json!({
                    "segments": [
                        { "start": 0.0, "end": 4.0, "text": "we found it" },
                    ],
                }).to_string(),
            }),
            json!({
                "audio": { "path": take },
                "lines": [
                    { "start": 0.0, "end": 4.0,
                      "prosody": { "emphasisWords": [{ "word": "found", "t": 1.0, "strength": 0.9 }] } },
                ],
            }),
            json!({
                "lines": [
                    emotion_row(0.0, 4.0, "anger", 0.9, "tension", "hot",
                                "impact-heavy (hard hits, distorted risers)"),
                ],
            }),
        ];
        // Every walk starts the same way: the generator is asked first and
        // answers absent, so the shelf is the only accent there is. The
        // peak sits at 1.0 s - inside the hook, whose family is impact,
        // riser, whoosh - and the first search shops the family's first
        // word, the rotation walking each later ask from a different one.
        let shelf = {
            let mut start = log.clone();
            start.push(json!({ "tool": "generate", "ok": true, "model": "absent",
                              "generated": false }));
            start
        };

        // The hit carries a `syncAt` the manifest believes: the placement
        // lays the file by it, and the check then catches the drift. Every
        // walk asks the generator first, hears it answer absent, and only
        // then shops the shelf.
        let mut drift = log.clone();
        let mut drift_program = auto_edit_program(&source);
        let ProgramStep::Derived(drift_sfx) = drift_program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "sfx")
            .expect("the program has an sfx stage")
        else {
            panic!("the sfx stage is derived");
        };
        let asked = (drift_sfx.next)(&drift).expect("the generator is asked first");
        assert_eq!(asked.tool, "sense_sound_model");
        drift = shelf.clone();
        let search = (drift_sfx.next)(&drift).expect("the shelf is asked");
        assert_eq!(search.tool, "sounds_search");
        drift.push(json!({ "sounds": [
            { "id": "hit-1", "duration": 0.8, "syncAt": 0.1 },
        ] }));
        let add = (drift_sfx.next)(&drift).expect("the hit is placed");
        assert_eq!(add.tool, "sounds_add");
        assert_eq!(add.arguments["start"], json!(0.9));
        drift.push(json!({ "clipId": "clip-1", "file": "/edits/Reel/audio/hit-1.ogg" }));
        let check = (drift_sfx.next)(&drift).expect("the placement is checked");
        assert_eq!(check.tool, "sense_sound");
        assert_eq!(check.arguments["intendedAt"], json!(1.0));
        // The file's own peak sits 0.25 s later than the manifest said: the
        // check prescribes the move, and the spine makes it.
        drift.push(json!({
            "ok": true,
            "issues": [{ "code": "sync_off", "severity": "fix" }],
            "measured": { "moveBy": 0.25, "peakAt": 0.35 },
        }));
        let moved = (drift_sfx.next)(&drift).expect("the clip is moved onto the moment");
        assert_eq!(moved.tool, "clips_move");
        assert_eq!(
            moved.arguments["moves"][0]["start"],
            json!(1.15),
            "the corrected start is the laid start plus the measured drift"
        );
        drift.push(json!({ "moved": 1 }));
        let gained = (drift_sfx.next)(&drift).expect("the moved clip is gained");
        assert_eq!(gained.tool, "clip_audio_levels_db");

        // A file that measures as another family's sound is not left on the
        // lane to be heard as the accent it was asked for: it comes off,
        // and the family's next word shops the peak again.
        let mut wrong = log.clone();
        let mut wrong_program = auto_edit_program(&source);
        let ProgramStep::Derived(wrong_sfx) = wrong_program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "sfx")
            .expect("the program has an sfx stage")
        else {
            panic!("the sfx stage is derived");
        };
        (wrong_sfx.next)(&wrong).expect("the generator is asked first");
        wrong = shelf.clone();
        (wrong_sfx.next)(&wrong).expect("the shelf is asked");
        wrong.push(json!({ "sounds": [
            { "id": "hit-2", "duration": 0.6, "syncAt": 0.05 },
        ] }));
        let add2 = (wrong_sfx.next)(&wrong).expect("the second hit is placed");
        assert_eq!(add2.tool, "sounds_add");
        wrong.push(json!({ "clipId": "clip-2", "file": "/edits/Reel/audio/hit-2.ogg" }));
        let check2 = (wrong_sfx.next)(&wrong).expect("the second placement is checked");
        assert_eq!(check2.tool, "sense_sound");
        wrong.push(json!({
            "ok": true,
            "issues": [{ "code": "wrong_sound", "severity": "fix" }],
            "measured": { "moveBy": 0.0 },
        }));
        let removed = (wrong_sfx.next)(&wrong).expect("the wrong sound comes off");
        assert_eq!(removed.tool, "clips_remove");
        assert_eq!(removed.arguments["clipIds"], json!(["clip-2"]));
        wrong.push(json!({ "removed": 1 }));
        let research = (wrong_sfx.next)(&wrong).expect("the family's next word is shopped");
        assert_eq!(research.tool, "sounds_search");
        assert_eq!(
            research.arguments["query"],
            json!("whoosh"),
            "the rotation had walked once by the add, so the re-shop is the family's third word"
        );

        // A check that cannot answer leaves the placement as laid: the run
        // says so by gaining it, never by claiming it was verified.
        let mut blind = log.clone();
        let mut blind_program = auto_edit_program(&source);
        let ProgramStep::Derived(blind_sfx) = blind_program
            .steps
            .iter_mut()
            .find(|step| step.stage().label() == "sfx")
            .expect("the program has an sfx stage")
        else {
            panic!("the sfx stage is derived");
        };
        (blind_sfx.next)(&blind).expect("the generator is asked first");
        blind = shelf.clone();
        (blind_sfx.next)(&blind).expect("the shelf is asked");
        blind.push(json!({ "sounds": [
            { "id": "hit-3", "duration": 0.9, "syncAt": 0.2 },
        ] }));
        let add3 = (blind_sfx.next)(&blind).expect("the third hit is placed");
        assert_eq!(add3.tool, "sounds_add");
        blind.push(json!({ "clipId": "clip-3", "file": "/edits/Reel/audio/hit-3.ogg" }));
        let check3 = (blind_sfx.next)(&blind).expect("the third placement is checked");
        assert_eq!(check3.tool, "sense_sound");
        blind.push(json!({ "ok": false, "error": "the sidecar could not decode it" }));
        let gained3 = (blind_sfx.next)(&blind).expect("the unverified accent is still gained");
        assert_eq!(gained3.tool, "clip_audio_levels_db");
        assert_eq!(gained3.arguments["clipId"], json!("clip-3"));
    }

    /// A wrong-family accent is removed and re-shopped for the same peak;
    /// the replacement must not advance the cursor twice and skip the next
    /// emphasis in the voiceover.
    #[test]
    fn a_wrong_sfx_replacement_does_not_skip_the_next_peak() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
        let mut log = vec![
            json!({
                "timeline": { "clips": [{
                    "id": "c1", "media": "m1", "start": 0.0, "duration": 20.0,
                    "sourceStart": 0.0, "speed": 1.0,
                }]},
                "media": [{ "id": "m1", "path": take }],
            }),
            json!({
                "path": take,
                "output": json!({
                    "segments": [
                        { "start": 0.0, "end": 4.0, "text": "we found it" },
                        { "start": 5.0, "end": 9.0, "text": "the danger grew" },
                    ],
                }).to_string(),
            }),
            json!({
                "audio": { "path": take },
                "lines": [
                    { "start": 0.0, "end": 4.0,
                      "prosody": { "emphasisWords": [{ "word": "found", "t": 1.0, "strength": 0.9 }] } },
                    { "start": 5.0, "end": 9.0,
                      "prosody": { "emphasisWords": [{ "word": "danger", "t": 6.0, "strength": 0.9 }] } },
                ],
            }),
            json!({
                "lines": [
                    emotion_row(0.0, 4.0, "anger", 0.9, "tension", "hot", "impact-heavy"),
                    emotion_row(5.0, 9.0, "fear", 0.9, "tension", "cold", "drone"),
                ],
            }),
        ];

        assert_eq!(
            (sfx.next)(&log).expect("the first generator is asked").tool,
            "sense_sound_model"
        );
        log.push(json!({ "tool": "generate", "ok": true, "model": "absent",
                         "generated": false }));
        assert_eq!(
            (sfx.next)(&log).expect("the shelf is asked").tool,
            "sounds_search"
        );
        log.push(json!({ "sounds": [
            { "id": "hit-1", "duration": 0.8, "syncAt": 0.1 },
        ] }));
        assert_eq!(
            (sfx.next)(&log).expect("the first hit is added").tool,
            "sounds_add"
        );
        log.push(json!({ "clipId": "clip-1", "file": "/edits/Reel/audio/hit-1.ogg" }));
        let check = (sfx.next)(&log).expect("the first placement is checked");
        assert_eq!(check.tool, "sense_sound");
        assert_eq!(check.arguments["role"], json!("action_impact"));
        log.push(json!({
            "ok": true,
            "issues": [{ "code": "wrong_sound", "severity": "fix" }],
            "measured": { "moveBy": 0.0 },
        }));
        assert_eq!(
            (sfx.next)(&log).expect("the wrong hit is removed").tool,
            "clips_remove"
        );
        log.push(json!({ "removed": 1 }));
        assert_eq!(
            (sfx.next)(&log).expect("the same peak is re-shopped").tool,
            "sounds_search"
        );
        log.push(json!({ "sounds": [
            { "id": "hit-2", "duration": 0.7, "syncAt": 0.1 },
        ] }));
        assert_eq!(
            (sfx.next)(&log).expect("the replacement is added").tool,
            "sounds_add"
        );
        log.push(json!({ "clipId": "clip-2", "file": "/edits/Reel/audio/hit-2.ogg" }));
        let replacement_check = (sfx.next)(&log).expect("the replacement is checked");
        assert_eq!(replacement_check.tool, "sense_sound");
        assert_eq!(
            replacement_check.arguments["role"],
            json!("quick_transition")
        );
        log.push(json!({ "ok": true, "issues": [], "measured": { "moveBy": 0.0 } }));
        assert_eq!(
            (sfx.next)(&log).expect("the replacement is gained").tool,
            "clip_audio_levels_db"
        );
        log.push(json!({ "ok": true }));

        let next_peak = (sfx.next)(&log)
            .expect("the replacement must leave the next voiceover peak to be processed");
        assert_eq!(next_peak.tool, "sounds_search");
        assert_eq!(next_peak.arguments["query"], json!("ambience"));
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
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
                "project": { "name": "Reel", "width": 1080, "height": 1920 },
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
        // A dry voiceover asks the bed question first - the bed is the
        // beat source - and a cold shelf (missing) is answered by moving
        // straight on: the planner runs without a grid.
        let bed = (plan.next)(&log).expect("the bed question is asked");
        assert_eq!(bed.tool, "bgm_status");
        let mut dry = log.clone();
        dry.push(json!({ "freepd": "missing" }));

        // The footage shopping list is planned first: the planner reads the
        // transcript and says where coverage is wanted, before the plan
        // that names the signals is validated.
        let first = (plan.next)(&dry).expect("the footage plan is asked for");
        assert_eq!(first.tool, "footage_plan_suggest");
        assert!(
            !first.arguments["segments"]
                .as_array()
                .expect("the planner is given the transcript")
                .is_empty(),
            "the planner reads the narration's own spans: {first:?}"
        );
        let ask = (plan.next)(&dry).expect("the plan is validated");
        assert_eq!(ask.tool, "plan_validate");
        let plan = &ask.arguments["plan"];
        // The frame the project is: a 1080x1920 project is a vertical reel.
        assert_eq!(plan["aspect"], "9:16");
        assert_eq!(plan["mode"], "reel");
        // The narration answered the thesis and the payoff. The goal is
        // the button's own - a one-click edit is a retention edit - and
        // the audience stays absent here because this transcript named no
        // entity: the gate names it, which is the honest answer.
        assert_eq!(plan["creativeBrief"]["thesis"], "we found the wreck");
        assert_eq!(plan["creativeBrief"]["payoff"], "and then it moved");
        assert_eq!(plan["creativeBrief"]["goal"], "watch to the end");
        assert!(
            plan["creativeBrief"].get("audience").is_none(),
            "an audience is not measured from a transcript that names nothing"
        );
        // The narration's own spans are planned as the B-roll the gather
        // places - never as existing footage over audio media - and the
        // plan carries the live shelf it will be shopped from.
        for segment in plan["segments"].as_array().expect("segments") {
            assert_eq!(
                segment["visual"]["type"], "broll",
                "an audio clip's span is never claimed as existing footage"
            );
            assert_eq!(segment["visual"]["provider"], "pexels");
            assert!(
                segment["visual"]["description"]
                    .as_str()
                    .is_some_and(|description| description.chars().count() >= 40),
                "the semantic description clears the shelf's ranking floor"
            );
        }
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
    /// The audit's Q9, answered: a dry voiceover's cuts land on the BED's
    /// beats, not only on a take that already carried music. The plan
    /// stage shops the bed early (search, import, trim to the
    /// narration's own extent), analyzes it, and hands the planner the
    /// grid translated into the take's own file clock.
    #[test]
    fn the_bed_s_beats_drive_the_planner_on_a_dry_voiceover() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let bed = "/edits/Reel/bgm/warm-piano.mp3";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: vec![TakePlacement {
                take: take.to_owned(),
                start: 0.0,
                source_in: 0.0,
                source_out: 10.0,
                speed: 1.0,
            }],
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
        let view = json!({
            "settings": { "width": 1080, "height": 1920 },
            "timeline": { "clips": [
                { "id": "voice", "kind": "audio", "media": "m1", "start": 0.0,
                  "duration": 10.0, "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [{ "id": "m1", "path": take }],
        });
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 4.0, "text": "we found the wreck below the harbour wall" },
                { "start": 6.0, "end": 9.0, "text": "and it was not empty" },
            ],
        });
        let mut log = vec![
            view,
            json!({ "path": take, "output": transcript.to_string() }),
        ];

        // The bed question, answered ready: the shelf is shopped.
        let status = (plan.next)(&log).expect("the bed question is asked");
        assert_eq!(status.tool, "bgm_status");
        log.push(json!({ "freepd": "ready" }));
        let search = (plan.next)(&log).expect("the shelf is shopped");
        assert_eq!(search.tool, "bgm_search");
        assert_eq!(search.arguments["source"], "freepd");
        log.push(json!({
            "tracks": [
                { "id": "warm-piano", "title": "Warm Piano Nocturne", "seconds": 200.0 }
            ],
        }));
        let import = (plan.next)(&log).expect("the bed is imported");
        assert_eq!(import.tool, "bgm_import");
        assert_eq!(import.arguments["ids"], json!(["warm-piano"]));
        assert_eq!(import.arguments["monetized"], json!(true));
        log.push(json!({ "mediaIds": ["m-bed"] }));

        // The timeline is read back for the placed bed.
        let read = (plan.next)(&log).expect("the timeline is read for the bed");
        assert_eq!(read.tool, "timeline_read");
        let placed = json!({
            "timeline": { "clips": [
                { "id": "voice", "kind": "audio", "media": "m1", "start": 0.0,
                  "duration": 10.0, "sourceStart": 0.0, "speed": 1.0 },
                { "id": "bed", "kind": "audio", "media": "m-bed", "start": 0.0,
                  "duration": 200.0, "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [
                { "id": "m1", "path": take },
                { "id": "m-bed", "path": bed },
            ],
        });
        log.push(placed);

        // A three-minute bed under a ten-second cut is trimmed, so no
        // later runtime read is inflated by the bed.
        let trim = (plan.next)(&log).expect("the bed is trimmed to the cut");
        assert_eq!(trim.tool, "edit_apply");
        assert_eq!(trim.arguments["command"]["op"], "trimClip");
        assert_eq!(trim.arguments["command"]["clipId"], "bed");
        assert_eq!(trim.arguments["command"]["delta"], json!(-190.0));
        log.push(json!({ "applied": true }));

        // The trimmed bed is analyzed, and the grid is waited on.
        let analyze = (plan.next)(&log).expect("the bed is analyzed");
        assert_eq!(analyze.tool, "analyze_beats");
        assert_eq!(analyze.arguments["path"], bed);
        log.push(json!({ "job": "analysis-9", "path": bed, "output": "" }));
        let wait = (plan.next)(&log).expect("the grid is waited on");
        assert_eq!(wait.tool, "analysis_wait");
        assert_eq!(wait.arguments["path"], bed);
        log.push(json!({
            "path": bed,
            "output": json!({ "beatsMs": [2000.0, 5000.0], "bpm": 90.0 }).to_string(),
        }));

        // The planner is handed the grid, in the take's own file clock.
        let spec = (plan.next)(&log).expect("the planner is asked");
        assert_eq!(spec.tool, "footage_plan_suggest");
        assert_eq!(
            spec.arguments["beats"],
            json!([
                { "seconds": 2.0, "strength": 0.5 },
                { "seconds": 5.0, "strength": 0.5 },
            ]),
            "the bed's timeline-clock beats are translated into the take's file clock"
        );
    }

    /// The music stage does not shop twice: a bed the plan stage landed so
    /// its beats could drive the planner is trimmed and ducked where it
    /// lies, and the stage's first call reads the timeline instead of the
    /// shelf's state.
    #[test]
    fn the_music_stage_reuses_the_bed_the_plan_landed() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
        let log = vec![json!({
            "timeline": { "clips": [
                { "id": "voice", "kind": "audio", "media": "m1", "start": 0.0,
                  "duration": 12.0, "sourceStart": 0.0, "speed": 1.0 },
                { "id": "bed", "kind": "audio", "media": "m-bed", "start": 0.0,
                  "duration": 12.0, "sourceStart": 0.0, "speed": 1.0 },
            ]},
            "media": [
                { "id": "m1", "path": take },
                { "id": "m-bed", "path": "/edits/Reel/bgm/warm-piano.mp3" },
            ],
        })];
        let first = (music.next)(&log).expect("the stage starts");
        assert_eq!(
            first.tool, "timeline_read",
            "an existing bed is read back, not re-shopped"
        );
    }

    /// The plan the run writes VALIDATES: the stage's hard gate on
    /// `valid` is only honest if the fixed program's own plan clears it.
    /// The audience blocker may stand (a transcript that names nothing),
    /// but the structure - sections, segments, the signalLog, the
    /// B-roll claims with their licensed provider - must pass.
    #[test]
    fn the_plan_the_run_writes_validates() {
        use concat_director::ProgramStep;

        let take = "/t/a.wav";
        let source = AutoEditProgramSource {
            project: "/edits/Reel".to_owned(),
            takes: vec![TakePaths {
                take: take.to_owned(),
                enhanced: String::new(),
                aroll: None,
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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
                "settings": { "width": 1920, "height": 1080 },
                "timeline": { "clips": [
                    { "id": "c1", "media": "m1", "start": 0.0, "duration": 18.0,
                      "sourceStart": 0.0, "speed": 1.0 },
                ]},
                "media": [{ "id": "m1", "path": take }],
            }),
            json!({
                "path": take,
                "output": json!({
                    "segments": [
                        { "start": 0.0, "end": 4.0, "text": "we found the wreck below the old harbour wall" },
                        { "start": 5.0, "end": 9.0, "text": "it was not empty, the hold still carried crates" },
                        { "start": 12.0, "end": 16.0, "text": "and then the crane lifted it all away" },
                    ],
                }).to_string(),
            }),
            json!({
                "lines": [
                    emotion_row(0.0, 4.0, "surprise", 0.6, "hook", "cold", ""),
                    emotion_row(5.0, 9.0, "fear", 0.8, "tension", "cold", "impact-heavy"),
                    emotion_row(12.0, 16.0, "reveal", 0.9, "cold", "", "stinger"),
                ],
            }),
        ];
        // Walk to the plan_validate call and run it through the real
        // validator: the same function the tool serves.
        let mut ask = (plan.next)(&log).expect("the planner is asked");
        while ask.tool != "plan_validate" {
            ask = (plan.next)(&log).expect("the stage keeps going");
        }
        assert_eq!(ask.tool, "plan_validate");
        let plan = &ask.arguments["plan"];
        let roles: Vec<&str> = plan["sections"]
            .as_array()
            .expect("the plan has story sections")
            .iter()
            .filter_map(|section| section["role"].as_str())
            .collect();
        for role in ["hook", "curiosity", "progression", "payoff", "ending"] {
            assert!(
                roles.contains(&role),
                "the story skeleton is missing {role}: {roles:?}"
            );
        }
        assert!(
            plan["retention"]["analysis"]["boringStretches"].is_array(),
            "the retention pass writes its analysis"
        );
        assert!(
            plan["visualVariety"]["consecutiveShotScaleRepeats"].is_number(),
            "the variety pass writes its repeat counts"
        );
        assert!(
            plan["signalLog"]
                .as_array()
                .is_some_and(|log| !log.is_empty()),
            "the plan names the signals behind its decisions"
        );
        let youtube = &plan["youtube"];
        assert_eq!(youtube["mode"], json!("reel"));
        assert_eq!(youtube["profile"], json!("storytelling"));
        assert_eq!(
            youtube["profilePlan"]["profile"], youtube["profile"],
            "the profile plan names the same profile the package does"
        );
        assert!(
            youtube["profilePlan"]["pacing"]["patternBreakEvery"].is_number()
                && youtube["profilePlan"]["motion"]["intensity"].is_number()
                && youtube["profilePlan"]["broll"]["qualifier"].is_string(),
            "the profile plan names its pacing, motion and B-roll qualifier"
        );
        assert_eq!(
            plan["style"]["motion"]["intensity"], youtube["profilePlan"]["motion"]["intensity"],
            "the style's motion hand is the profile plan's own intensity"
        );
        assert_eq!(
            plan["segments"][0]["visual"]["profile"], youtube["profile"],
            "the B-roll query carries the profile it was shopped for"
        );
        assert!(
            plan["segments"][0]["visual"]["queries"]
                .as_array()
                .is_some_and(|queries| queries.len() >= 3),
            "the profile adds its own query door beside the direction's"
        );
        assert_eq!(
            youtube["titles"].as_array().map(Vec::len),
            Some(5),
            "five title directions are offered"
        );
        assert_eq!(
            youtube["thumbnails"].as_array().map(Vec::len),
            Some(3),
            "three thumbnail frames are offered"
        );
        let shorts = youtube["shorts"]
            .as_array()
            .filter(|shorts| !shorts.is_empty())
            .expect("the transcript supports at least one Short window");
        let short = &shorts[0];
        assert!(
            short["scoreBreakdown"].is_object()
                && short["standalone"].is_boolean()
                && short["title"].is_string()
                && short["reframe"].is_string()
                && short["captions"].is_string(),
            "each Short names its score, standalone check and vertical treatment: {short}"
        );
        assert!(
            short["thumbnailAt"].as_f64().is_some_and(
                |at| at >= 0.0 && at <= plan["segments"][0]["end"].as_f64().unwrap_or(0.0)
            ),
            "the Short's thumbnail frame sits inside the cut"
        );
        assert_eq!(
            youtube["retentionMap"].as_array().map(Vec::len),
            plan["sections"].as_array().map(Vec::len),
            "every story section has a retention entry"
        );
        assert!(
            youtube["thumbnails"]
                .as_array()
                .is_some_and(|frames| frames.iter().all(|frame| {
                    frame["frameAt"].as_f64().is_some_and(|at| at >= 0.0)
                        && frame["text"].as_str().is_some_and(|text| !text.is_empty())
                })),
            "each thumbnail names a frame and its text"
        );
        assert_eq!(
            youtube["quality"]["verdict"],
            json!("pass"),
            "the plan-level quality report passes a clean plan: {}",
            youtube["quality"]
        );
        assert!(
            youtube["quality"]["checks"]
                .as_array()
                .is_some_and(|checks| checks.len() >= 7
                    && checks.iter().all(|check| {
                        check["check"].is_string()
                            && check["verdict"].is_string()
                            && check["detail"].is_string()
                    })),
            "every quality check names what it measured"
        );
        let checks = youtube["quality"]["checks"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        for named in [
            "every segment has a picture",
            "the timeline is covered",
            "every Short inside the runtime",
        ] {
            assert!(
                checks.iter().any(|check| check["check"] == json!(named)),
                "the quality report names {named}"
            );
        }
        let checked =
            crate::tools::validate_plan_tool(ask.arguments.clone()).expect("the plan validates");
        assert_eq!(
            checked["valid"],
            json!(true),
            "the fixed program's own plan passes its own hard gate: {checked}"
        );
        // The audience blocker is the honest remainder: this transcript
        // names no entity, so no audience is invented to pass the
        // creative gate.
        assert_eq!(checked["creativeReady"], json!(false));
        assert!(
            checked["creativeBlockers"]
                .as_array()
                .is_some_and(|blockers| {
                    blockers.iter().any(|blocker| {
                        blocker
                            .as_str()
                            .is_some_and(|text| text.contains("audience"))
                    })
                }),
            "the audience is named as the blocker it honestly is"
        );
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
                    aroll: None,
                },
                TakePaths {
                    take: take_b.to_owned(),
                    enhanced: String::new(),
                    aroll: None,
                },
            ],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
            script: None,
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

        // The analysis is a job: the next call waits for it, or the
        // planner is built before a single beat was measured.
        let mut started = log.clone();
        started.push(json!({ "job": "analysis-7", "path": take_b, "output": "" }));
        let wait = (plan.next)(&started).expect("the beat job is waited on");
        assert_eq!(wait.tool, "analysis_wait");
        assert_eq!(wait.arguments["path"], take_b);

        // The wait's reply carries the grid the way the analysis writes
        // it: milliseconds, under beatsMs.
        let mut measured = started;
        measured.push(json!({
            "path": take_b,
            "output": json!({ "beatsMs": [1000.0, 2500.0, 4000.0], "bpm": 120.0 }).to_string(),
        }));
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
                aroll: None,
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
            script: None,
        };
        let program = auto_edit_program(&source);
        // The program's own Verify stage is the timeline's: the lanes it
        // leaves clean and the timeline it reads back. The encoded-file
        // probes - the transcript re-read, the loudness, the words
        // comparison and the motion probe of the actual file - are the
        // independent acceptance's checks() when a render exists, so the
        // shipped file is probed exactly where the surface says it is.
        let verify = program
            .steps
            .iter()
            .find(|step| step.stage().label() == "verify")
            .expect("the program has a Verify stage");
        let ProgramStep::Static(step) = verify else {
            panic!("Verify is a static stage");
        };
        let calls: Vec<&str> = step.calls.iter().map(|call| call.tool.as_str()).collect();
        assert_eq!(calls, ["collision_check", "timeline_read"]);
        assert!(
            step.gates.iter().any(|gate| gate.field() == "clean"),
            "the verify gate refuses shared lanes on the saved timeline"
        );
        // The probes the rendered export owes, on the acceptance's own
        // list: the file itself is opened by the motion probe, its
        // loudness measured, and the words it carries compared - after
        // the content and mix gates the program already landed.
        let asked = crate::acceptance::checks(&source.project, Some(&source.export));
        let probes: Vec<&str> = asked.iter().map(|(tool, _)| *tool).collect();
        assert!(
            probes.contains(&"motion_probe"),
            "the actual export is probed by the acceptance"
        );
        let probe = asked
            .iter()
            .find(|(tool, _)| *tool == "motion_probe")
            .expect("the probe is asked");
        assert_eq!(probe.1["path"], source.export);
    }
}
