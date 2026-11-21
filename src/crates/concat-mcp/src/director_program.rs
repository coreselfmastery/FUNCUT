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

use crate::voice_read::{Entity, Motif, VoiceRead};

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
    /// About to run the continuity gate over the chosen run.
    Continuity,
    /// The continuity reply is in: repair what the measurements flagged.
    ReadContinuity,
    /// About to import the next chosen candidate.
    Import,
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
}

impl SlotChoice {
    /// Advance to the next alternate the sheet measured as a different
    /// picture: a new clip id, a new frame hash, and not a clip another
    /// slot already chose. No such alternate leaves the flag standing,
    /// recorded by the check's own second pass.
    fn swap_past(&mut self, used_ids: &[String], used_hashes: &[String]) {
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
            return;
        }
    }
}

/// The continuity gate's manifest, from the measurements the sheet made
/// of the choices: the warmth each chosen frame read, and the slot's own
/// query words as its category - the plan's vocabulary, the same thing
/// the watcher's manifest carries. Nothing the pixels did not say is
/// written: an unmeasured warmth is omitted, never a neutral zero.
fn continuity_manifest(shopped: &[SlotChoice]) -> Vec<Value> {
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
            entry
        })
        .collect()
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
        let crosses_speaker = read.is_some_and(|read| {
            read.take_wide
                .speaker_changes
                .iter()
                .any(|change| *change > slot.from - 0.35 && *change < slot.to + 0.35)
        });
        let asks = read.is_some_and(|read| {
            read.line_at(slot.from).is_some_and(|line| {
                line.sentence_type == "question" || line.rise_confirms == Some(true)
            })
        });
        let holds = read.is_some_and(|read| read.keeps_pause_at(slot.from));
        // A chapter opens in this gap: the strongest seam the script
        // has, and the one the chapter card is already naming.
        let chapter = chapters.iter().any(|boundary| {
            boundary.take == slot.take
                && boundary.at > slot.from - 0.5
                && boundary.at < slot.to + 0.5
        });
        // A connective pivot inside the gap: the argument turned here,
        // so the picture turns with it.
        let pivot = pivots
            .iter()
            .any(|(take, at)| take == &slot.take && *at > slot.from - 0.5 && *at < slot.to + 0.5);
        (crosses_speaker, asks, holds, chapter, pivot)
    };
    let first = match seam {
        // A chapter opening is the clearest seam there is: the story
        // moved on.
        (_, _, _, true, _) => "concat.flash-social",
        // A connective pivot is the argument turning: the visible move
        // is what makes the turn read.
        (_, _, _, _, true) => "concat.cross-pop",
        // A speaker change is the clearest voice seam: the frame
        // changes because the voice did.
        (true, _, _, _, _) => "concat.flash-social",
        // A question opens a gap the answer fills: the move is the
        // ask, not the mood.
        (_, true, _, _, _) => "concat.cross-pop",
        // A held beat keeps the softest move the catalogue has, so the
        // cut never fights the silence the read said to hold.
        (_, _, true, _, _) => "concat.dissolve",
        _ => transition_for_emotion(&mood),
    };
    if last == Some(first) {
        alternate_transition(&mood)
    } else {
        first
    }
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
    /// The emotion engine's own confidence in this line's read, 0..1. The
    /// lexicon and the transformer answer at different certainties, and a
    /// signal log that rated an `emotion` entry by `voice_signals`'
    /// vocabulary alone could not rate it at all.
    confidence: Option<f64>,
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
            confidence: row["confidence"].as_f64(),
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

/// The words the take's own delivery stressed, lowercased and
/// deduplicated: `voice_signals`' `prosody.emphasisWords`, the signal the
/// caption stage spends. Empty when no reply measured any, which is the
/// honest answer for a take read without prosody - the stage then falls
/// back to its own evidence rather than inventing a stress.
fn stressed_words(log: &[Value], takes: &[TakePaths]) -> Vec<String> {
    voice_reads(log, takes)
        .into_iter()
        .flat_map(|read| {
            read.lines
                .into_iter()
                .flat_map(|line| line.emphasis.into_iter().map(|word| word.word))
        })
        .fold(Vec::new(), |mut words: Vec<String>, word| {
            if !words.contains(&word) {
                words.push(word);
            }
            words
        })
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
                confidence: line["confidence"].as_f64(),
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
                confidence: claim["confidence"].as_f64(),
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
    Some((preset, card_words(&entity.text)))
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

/// The motion the run will bake, one row per line that earned a card, in
/// play order: the take and file second the line opens on (the stage maps
/// it through the timeline), the preset, and the words the card sets.
///
/// The density floor is the plan schema's own: at most one card per
/// section and at most one per fifteen seconds, strongest line first. Two
/// cards in the same three seconds is a slideshow, not an edit.
fn motion_specs(
    lines: &[EmotionLine],
    entities: &[Entity],
    chapters: &[TopicBoundary],
    motifs: &[Motif],
    questions: &[ScriptLine],
    claims: &[ScriptLine],
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
                    earned_by: "emotion",
                    chapter: false,
                    slot: None,
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
        if words.is_empty() {
            continue;
        }
        let line = lines.iter().find(|line| {
            line.take == entity.take && line.start <= at + 0.05 && line.end > at - 0.05
        });
        ranked.push((
            line.map(|line| line.intensity).unwrap_or(0.5),
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
                earned_by: "entities",
                chapter: false,
                slot: None,
            },
        ));
    }
    // The chapter cards: a title over each topic change, named by the
    // tiling's own headline. They rank above a line's beat - a chapter is
    // the strongest thing the script said - but still obey the spacing
    // floor, because a chapter card landing on a stat card is a pile.
    for chapter in chapters {
        let words = card_words(&chapter.headline);
        if words.is_empty() {
            continue;
        }
        ranked.push((
            1.0,
            MotionSpec {
                take: chapter.take.clone(),
                at: chapter.at,
                end: chapter.at + 2.0,
                preset: "funcut.chapter.numbered",
                words,
                act: "turn".to_owned(),
                pace: String::new(),
                earned_by: "topics",
                chapter: true,
                slot: Some("Title"),
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
            ranked.push((
                line.intensity * 0.9,
                MotionSpec {
                    take: line.take.clone(),
                    at: line.start,
                    end: line.end,
                    preset,
                    words: words.clone(),
                    act: line.act.clone(),
                    pace: line.pace.clone(),
                    earned_by: "motifs",
                    chapter: false,
                    slot: None,
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
        if words.is_empty() {
            continue;
        }
        ranked.push((
            0.85,
            MotionSpec {
                take: question.take.clone(),
                at: question.start,
                end: question.end,
                preset: "funcut.caption.kinetic",
                words,
                act: String::new(),
                pace: String::new(),
                earned_by: "speechActs",
                chapter: false,
                slot: None,
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
        if words.is_empty() {
            continue;
        }
        ranked.push((
            0.8,
            MotionSpec {
                take: claim.take.clone(),
                at: claim.start,
                end: claim.end,
                preset: "funcut.quote.pull",
                words,
                act: "proof".to_owned(),
                pace: String::new(),
                earned_by: "claims",
                chapter: false,
                slot: None,
            },
        ));
    }
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
    /// The signal that earned the card: the read's act, or the entity the
    /// narration named. The log cites this, so a stat card is never
    /// accounted for by an emotion that had nothing to do with it.
    earned_by: &'static str,
    /// How the card holds. A chapter card is a title over a topic change:
    /// it holds its own beat rather than the line's, and never longer.
    chapter: bool,
    /// The preset slot the words belong in, when the card's words are not
    /// for the preset's first text slot. A chapter card's headline is its
    /// title, not its kicker.
    slot: Option<&'static str>,
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
        round3(paced.clamp(1.2, 5.0))
    }
}

/// A caption's words with one filler removed, matched whole and
/// case-insensitively, and never left with a double space or a dangling
/// comma where the word stood.
fn strip_word(text: &str, word: &str) -> String {
    let word = word.trim().to_lowercase();
    if word.is_empty() {
        return text.to_owned();
    }
    let mut kept: Vec<&str> = Vec::new();
    for piece in text.split_whitespace() {
        let bare = piece
            .trim_matches(|c: char| !c.is_alphanumeric())
            .to_lowercase();
        if bare == word {
            continue;
        }
        kept.push(piece);
    }
    kept.join(" ")
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

/// The emotional read the run made, in the order the takes were asked
/// about. The emotion stage asks once per take that has timed words, in
/// program order, so the `emotion` replies sit in the log in exactly
/// that order - the invariant this reader stands on, and the reason the
/// reader filters its takes with the same [`spoken_lines`] predicate the
/// stage skips them with.
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
        _ => signal,
    }
}

/// The loudness target the cut ships to, in LUFS: the plan's own platform
/// reading, from the timeline's aspect. A vertical cut is a reel and takes
/// the short-form target; everything else takes YouTube's. `None` when the
/// run has no timeline view to read the aspect from, so a mix is never
/// applied to a guess.
fn platform_target(log: &[Value]) -> Option<f64> {
    let view = latest_view(log)?;
    Some(match aspect_of(view) {
        "9:16" => -14.0,
        _ => -14.0,
    })
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
    let reads = voice_reads(log, takes);
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
        // The music check earns its place when the gate measured it, and
        // when the profile's own verdict turned on it: a `skip` is the
        // flatness-plus-onsets rule, which is the music-segment reading,
        // and a `duck_only` is the SNR rule the forensic read carries.
        let verdict = reads
            .iter()
            .filter_map(|read| read.take_wide.bgm_decision.as_deref())
            .find(|decision| *decision != "add");
        if reads
            .iter()
            .any(|read| read.take_wide.music_present.is_some())
            || verdict == Some("skip")
        {
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
        signal_log.push(json!({
            "at": 0.0,
            "decision": "pacing",
            "signals": ["loudness"],
            "note": format!(
                "the take measured {measured:.1} LUFS and the platform targets {target:.0}, so the voice takes {:+.1} dB",
                target - measured
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

/// The profile's own family names, mapped onto the vocabulary
/// `plan_validate` checks a decision against. The producer names a family
/// for what it measured (`pauses`, `sentenceType`, `wordConfidence`); the
/// vocabulary names it for the decision it serves (`pause`, `sentence`,
/// `words`). A brief that handed a critic the producer's name would give
/// it a signal it cannot cite, so every name is translated here and a
/// family with no vocabulary name is left out rather than offered.
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
        _ => return None,
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
        if let Some(rows) = reply["degradations"].as_array() {
            for row in rows.iter().filter_map(Value::as_str) {
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
        .filter_map(|name| decision_for(name).map(|decision| (name.clone(), json!(decision))))
        .collect();
    if !decides.is_empty() {
        digest.insert("decides".to_owned(), Value::Object(decides));
    }
    Value::Object(digest)
}

/// The decision a family informs, in the profile's own decision-policy
/// words. A family with no entry drives no timeline choice here, and the
/// digest leaves it out of `decides` rather than inventing one.
fn decision_for(family: &str) -> Option<&'static str> {
    Some(match family {
        "pause" | "emphasisWords" | "speechRate" | "rhythmLayers" | "motifs" => "cut",
        // A filler is stripped from the caption it sits in, so the
        // decision it informs is the caption's.
        "words" | "language" | "sentence" | "fillers" => "caption",
        "entities" | "speechActs" | "claims" => "graphic",
        "noiseFloor" | "musicSegments" | "speechRatio" | "forensic" => "bgm",
        "speakerChanges" | "speaker" | "topics" | "discourseMarkers" => "transition",
        "loudness" | "prosody" | "emotion" | "arc" => "pacing",
        _ => return None,
    })
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
            forensic
                .get("crest")
                .and_then(Value::as_f64)
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
                            // The script read as language: the topic
                            // tiling names the chapter boundaries, and
                            // the connective analysis names the pivots
                            // (`but`, `however`) and the abrupt seams.
                            // Both are signals the vocabulary lists and
                            // nothing measured before this call, so the
                            // transition stage had to decide a seam on
                            // the mood alone.
                            calls.push(call(
                                "sense_discourse",
                                json!({ "action": "segment_topics", "lines": lines.clone() }),
                            ));
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
    let level_source = takes.clone();
    let level_project = project.clone();
    steps.push(
        DerivedStep::react(
            Stage::Timeline,
            Box::new({
                let mut queue: Option<Vec<ToolCall>> = None;
                let mut graded = false;
                let mut mixed = false;
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
                        // The mix: the profile measured the take's own
                        // integrated loudness, and the plan already names
                        // the platform the cut ships to. The one number the
                        // two give is the gain that puts the voice on the
                        // platform's target - so the verify stage's
                        // loudness gate is met by measurement rather than
                        // by luck. A take already within a LU of target
                        // takes no edit.
                        if mixed {
                            return None;
                        }
                        mixed = true;
                        let target = match platform_target(log) {
                            Some(target) => target,
                            None => return None,
                        };
                        let measured = voice_reads(log, &level_source)
                            .iter()
                            .filter_map(|read| read.take_wide.integrated_lufs)
                            .fold(None::<f64>, |loudest, lufs| {
                                Some(loudest.map_or(lufs, |loudest: f64| loudest.max(lufs)))
                            })?;
                        let gain = round3(target - measured);
                        if gain.abs() < 1.0 {
                            return None;
                        }
                        let view = latest_view(log)?;
                        let media = media_paths(view);
                        let clip = view["timeline"]["clips"].as_array()?.iter().find(|clip| {
                            clip["media"]
                                .as_str()
                                .and_then(|id| media.iter().find(|(mid, _)| mid == id))
                                .is_some_and(|(_, path)| {
                                    level_source.iter().any(|take| &take.take == path)
                                })
                        })?;
                        let clip_id = clip["id"].as_str()?.to_owned();
                        return Some(call(
                            "clip_audio_levels_db",
                            json!({
                                "path": level_project,
                                "clipId": clip_id,
                                "baseDb": gain,
                                "replaceAutomation": false,
                            }),
                        ));
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
                // The take's measured tempo, when the prosody read found
                // one: the seam's blend is a fraction of the period the
                // energy actually repeats on, so a slow take dissolves
                // slowly and a clipped one snaps.
                let mut pending_period: Option<f64> = None;
                let mut pending_transition: Option<&'static str> = None;
                // The transition the last placed shot cut in with: the
                // next shot never wears the same move twice running.
                let mut last_transition: Option<&'static str> = None;
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
                            let best = &candidates[0];
                            used_ids.push(best.id.clone());
                            if let Some(hash) =
                                best.tile["hash"].as_str().filter(|hash| !hash.is_empty())
                            {
                                used_hashes.push(hash.to_owned());
                            }
                            shopped.push(SlotChoice {
                                id: format!("slot-{:02}", shopped.len()),
                                slot: slots[shopping].clone(),
                                candidates,
                                chosen: 0,
                                held: false,
                            });
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
                                json!({ "clips": continuity_manifest(&shopped) }),
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
                                    // the swap the alternates exist for.
                                    "colour" => shop.swap_past(&used_ids, &used_hashes),
                                    // A category run: the gate's own
                                    // remedy is the treatment's, and the
                                    // treatment the program has is the
                                    // hold - the A-roll covers the span.
                                    "monotony" => shop.held = true,
                                    // Scale, motion and stillness flags
                                    // ride readings this manifest did not
                                    // carry (one frame measures no motion
                                    // and no scale), so acting on them
                                    // would be acting on the defaults the
                                    // gate filled in. They stand
                                    // recorded, not spent.
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
                                    json!({ "clips": continuity_manifest(&shopped) }),
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
                                    pending_style = take_wide.cut_style.clone();
                                    pending_period = take_wide
                                        .dominant_period_sec
                                        // A period the rhythm read found
                                        // but could not put strength
                                        // behind is an autocorrelation
                                        // artefact, not a pulse: it must
                                        // not size a seam.
                                        .filter(|_| {
                                            voice_read_of(voice, &slot.take)
                                                .and_then(|read| read.pulse_strength())
                                                .is_some_and(|strength| strength >= 0.25)
                                        });
                                    pending_transition = Some(seam_transition(
                                        voice,
                                        chapters,
                                        pivots,
                                        &emotion_lines(log, &gather_emotion),
                                        &slot,
                                        last_transition,
                                    ));
                                    phase = GatherPhase::Place;
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
                                    placing += 1;
                                    phase = GatherPhase::Import;
                                    continue;
                                }
                            }
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
                                _ => match pending_style.as_str() {
                                    // The gap's own delivery was not
                                    // measured, so the take's derived cut
                                    // style decides the move.
                                    "punchy" => 1.10,
                                    "conversational" => 1.07,
                                    "slow" => 1.03,
                                    _ => 1.06,
                                },
                            };
                            commands.push(json!({
                                "op": "setClipKey",
                                "clipId": clip_id,
                                "property": "scale",
                                "at": 1.0,
                                "value": drift,
                                "ease": [0.0, 0.0, 0.58, 1.0],
                            }));
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
                    let voice = voice_reads(log, &caption_source);
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
                            // The fillers the profile timed: `um`, `uh` and
                            // their kin are safe removals, and a caption is
                            // the one place they are pure noise - the
                            // viewer hears the pause, not the syllable. The
                            // discourse markers (`like`, `you know`) are
                            // context calls the profile does not decide,
                            // so they stay: cutting them would be editing
                            // the speaker's meaning, not their noise.
                            let take_read = voice_read_of(&voice, &take);
                            let mut shown = text.to_owned();
                            if let Some(take_read) = take_read {
                                for filler in take_read.fillers_between(start, end) {
                                    shown = strip_word(&shown, &filler.word);
                                }
                            }
                            let shown = shown.trim();
                            if shown.is_empty() {
                                continue;
                            }
                            // The word-confidence gate: a line most of
                            // whose words the gate could not hear is not
                            // a line to set heavy or accent - it is a
                            // line to show plainly, so a misheard word
                            // does not become the caption's loudest claim.
                            let unsure_share = take_read
                                .map(|take_read| {
                                    let flagged = take_read.unsure_between(start, end).len();
                                    let total = take_read
                                        .line_at(start)
                                        .map(|line| line.text.split_whitespace().count())
                                        .unwrap_or(0);
                                    if total == 0 {
                                        0.0
                                    } else {
                                        flagged as f64 / total as f64
                                    }
                                })
                                .unwrap_or(0.0);
                            let gated = unsure_share > 0.5;
                            // A line delivered above the take's own
                            // integrated level is a line the speaker
                            // lifted, so the caption carries weight even
                            // when the mood is mild. This is the one place
                            // the profile's per-line `relativeLufs` reaches
                            // the picture; without it the reading was named
                            // in the pacing entry and spent by nothing.
                            let lifted = take_read
                                .and_then(|take_read| take_read.line_at(start))
                                .and_then(|line| line.relative_lu)
                                .is_some_and(|lu| lu >= 3.0);
                            // A caption that carries one of the take's own
                            // stressed words takes the accent even on a
                            // neutral line: the emphasis signal is the
                            // delivery's, not the mood's, and the log
                            // promises the caption stage spends it.
                            let stressed = !gated
                                && !emphasis.is_empty()
                                && shown
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
                            // The profile's own 6-semitone bar separates a
                            // line *performed* from one *reported*. A
                            // performed line has room for a heavier, larger
                            // setting; a reported one is set plainly even
                            // where its emotion is intense, because the
                            // delivery is not carrying the feeling - the
                            // words are. `None` (never measured) leaves the
                            // house setting alone rather than guessing.
                            let performed = take_read.and_then(|take_read| {
                                take_read
                                    .line_at(start)
                                    .and_then(|line| take_read.performs(line))
                            });
                            let size = match performed {
                                Some(true) if !gated => size + 0.003,
                                _ => size,
                            };
                            captions.push(json!({
                                "start": round3(at),
                                "end": round3(to),
                                "text": shown,
                                "weight": if (intensity >= 0.65 || lifted) && !gated {
                                    800
                                } else if performed == Some(true) && !gated {
                                    700
                                } else {
                                    600
                                },
                                "accent": (hot || stressed) && !gated,
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
                    // The script's own font: the profile's language read
                    // names the family a non-Latin script needs, and the
                    // house face is only the fallback when the read did
                    // not name one. A caption set in Hanken Grotesk over a
                    // Devanagari or Cyrillic take renders as boxes.
                    let font = voice
                        .iter()
                        .map(|read| read.take_wide.font.as_str())
                        .find(|font| !font.is_empty())
                        .map(bundled_font)
                        .unwrap_or("Hanken Grotesk");
                    let commands: Vec<Value> = kept
                        .iter()
                        .map(|caption| {
                            let mut style = json!({
                                "fontFamily": font,
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
                    let specs = specs.get_or_insert_with(|| {
                        let reads = voice_reads(log, &motion_source);
                        let motifs: Vec<Motif> = reads
                            .iter()
                            .flat_map(|read| read.motifs.iter().cloned())
                            .collect();
                        motion_specs(
                            &emotion_lines(log, &motion_source),
                            &entity_graphics(log, &motion_source),
                            &topic_boundaries(log, &motion_source),
                            &motifs,
                            &question_acts(log, &motion_source),
                            &uncovered_claims(log, &motion_source),
                        )
                    });
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
                            // A card that names its own slot uses it; the
                            // rest take the preset's first text slot, which
                            // is what the library's own labels say.
                            let chosen = spec.slot.map(str::to_owned).or_else(|| {
                                labels
                                    .as_ref()
                                    .and_then(|labels| {
                                        labels.iter().find(|(id, _)| id == spec.preset)
                                    })
                                    .map(|(_, slot)| slot.clone())
                            });
                            if let Some(slot) = chosen {
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
                        .any(|read| read.take_wide.music_present == Some(true))
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
                                // then the reading's own language hint,
                                // which the profile derived from the
                                // script the narration is written in.
                                // The hint is the third door rather than
                                // the second so a measured mood always
                                // outranks a script-level guess.
                                let query = dominant(&emotion_lines(log, &music_source))
                                    .and_then(|line| line.bgm_words.first().cloned())
                                    .or_else(|| {
                                        voice_reads(log, &music_source)
                                            .into_iter()
                                            .map(|read| read.take_wide.music_genre)
                                            .find(|genre| !genre.trim().is_empty())
                                    })
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
                                .filter_map(|read| read.take_wide.speech_ratio)
                                .fold(None::<f64>, |worst, ratio| {
                                    Some(worst.map_or(ratio, |worst: f64| worst.max(ratio)))
                                })
                                .map(|ratio| -6.0 * ratio)
                                .unwrap_or(0.0);
                            let base_db = round3((act_db + density_db).clamp(-30.0, -6.0));
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
        let specs = motion_specs(&[], &[], &[], &[], &[], &claims);
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
        let specs = motion_specs(&[], &[], &[], &[], &questions, &[]);
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
        }];
        let transcript = json!({
            "segments": [
                { "start": 0.0, "end": 3.0, "text": "we found the wreck" },
                { "start": 4.0, "end": 7.0, "text": "it was not empty" },
            ],
        });
        let view = json!({
            "settings": { "width": 1080, "height": 1920 },
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
                "wholeTake": {
                    "loudness": { "integratedLufs": -20.0 },
                    "forensic": { "noiseFloorDbfs": -60.0, "snrDb": 18.0 },
                },
                "pauses": [{ "start": 3.0, "end": 4.0, "class": "dead_air" }],
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
        let schema = crate::plan_schema::canonical();
        crate::schema::check(&schema, &plan, &schema, "plan")
            .expect("AUTO EDIT's own plan passes the schema plan_validate enforces");
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
        assert_eq!(evidence["decides"]["pause"], json!("cut"));
        assert_eq!(evidence["decides"]["words"], json!("caption"));
        assert_eq!(
            evidence["decides"]["forensic"],
            json!("bgm"),
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
            .filter(|family| decision_for(family).is_none())
            .collect();
        assert!(
            unmapped.is_empty(),
            "these families are spent by the edit but name no decision, so \
             the critic is never told what they drove: {unmapped:?}"
        );
        // And the mapping is not free-form: a family may only claim one of
        // the decisions the log's own gate accepts, or the critic would be
        // pointed at a choice the validator rejects.
        for family in crate::tools::signal_vocabulary() {
            let Some(decision) = decision_for(family) else {
                continue;
            };
            assert!(
                matches!(
                    decision,
                    "cut" | "caption" | "graphic" | "bgm" | "transition" | "pacing"
                ),
                "{family} claims {decision:?}, which is not a logged decision"
            );
        }
        assert_eq!(decision_for("fillers"), Some("caption"));
        assert_eq!(decision_for("pause"), Some("cut"));
        assert_eq!(decision_for("topics"), Some("transition"));
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
                "spectral_centroid_hz",
                "a spectral centroid describes the voice's timbre, not the \
                 cut: the noise type and flatness already decide the repair",
            ),
            (
                "voicing_rate",
                "how much of the take is voiced separates speech from noise, \
                 which `speechRatio` already reports at the take's own level",
            ),
            (
                "dynamic_range_db",
                "a take-wide crest factor restates what `crest` measures per \
                 transient, and the duck depth already reads the SNR",
            ),
            (
                "cut_rule",
                "the profile's own cut-style rule is prose: `cut_style` is \
                 the reading it explains, and that one sizes every shot",
            ),
        ];
        let source = include_str!("voice_read.rs");
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
        let unspent: Vec<&str> = fields
            .iter()
            .copied()
            .filter(|field| !NOT_SPENT.iter().any(|(name, _)| name == field))
            .filter(|field| {
                !body.contains(&format!("{field}("))
                    && !body.contains(&format!(".{field}"))
                    && !body.contains(&format!("\"{field}\""))
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
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
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
                    "words": [{ "start": 0.5, "end": 1.5 }],
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
        }];
        let view = json!({
            "settings": { "width": 1920.0, "height": 1080.0 },
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
                .filter(|row| row["decision"] == json!("pacing"))
                .filter(|row| {
                    row["signals"]
                        .as_array()
                        .is_some_and(|signals| signals.contains(&json!("loudness")))
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
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
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
            "settings": { "width": 1920.0, "height": 1080.0 },
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
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
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
            "settings": { "width": 1920.0, "height": 1080.0 },
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
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
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
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
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
            }],
            placements: Vec::new(),
            export: "/edits/Reel/export/auto-edit.mp4".to_owned(),
            sheet: "auto-edit-sheet.png".to_owned(),
            stock_provider: "pexels".to_owned(),
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
                    "words": [{ "start": 0.5, "end": 1.5 }],
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
            "settings": { "width": 1080, "height": 1920 },
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
        let import = (gather.next)(&log).expect("the looked-at hit is imported");
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
            earned_by: "emotion",
            chapter: false,
            slot: None,
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
