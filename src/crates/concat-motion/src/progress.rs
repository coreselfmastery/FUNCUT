// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The farm's progress stream: what a long render is doing, said as it
//! does it.
//!
//! `geneva farm` cuts a render into parts that survive their own failure,
//! and a forty-minute render cut into ten parts is still forty minutes an
//! agent cannot see into: the call blocks, the tool answer is silence
//! until it ends. The progress stream is the other half of the contract:
//! every state the farm passes through is one line of JSON, appended to
//! `progress.jsonl` in the farm's own directory as it happens, and read
//! back - mid-render, from another process - by [`progress`].
//!
//! The events, in the order they arrive:
//!
//! ```text
//! {"event":"plan","parts":10,"frames":1638,"audio":true}
//! {"event":"part","index":0,"from":0,"to":164,"frames":164,"state":"rendering"}
//! {"event":"part","index":0,"state":"done","seconds":41.2}
//! {"event":"part","index":1,...,"state":"rendering"}
//! ...
//! {"event":"audio","state":"done","seconds":9.9}
//! {"event":"join","parts":10}
//! {"event":"done","out":"reel.mp4","seconds":418.9}
//! ```
//!
//! The line is the unit: one `write` call per event, the file opened in
//! append mode, so a reader never sees half a line unless it races the
//! write - and a torn tail is a reader's to ignore, which [`progress`]
//! does. Nothing is buffered between events, because a progress stream
//! that only flushes at the end is a report wearing the name.
//!
//! What the reader aggregates: each part's state (`pending`, `rendering`,
//! `done`, `served` - a part the farm's own directory already had), the
//! frames finished against the frames planned, the wall-clock seconds of
//! every finished part, and - the number an agent actually wants - a
//! whole-render estimate from the pace of the parts drawn so far. A farm
//! with no progress file yet is `pending` whole; a farm whose last event
//! is `done` is finished, and says where the file landed.

use std::io::Write as _;
use std::path::Path;

use serde_json::{Value as Json, json};

use crate::engine::MotionError;

/// Where the stream lives: beside the parts it describes.
const PROGRESS_FILE: &str = "progress.jsonl";

/// One line, appended.
fn say(dir: &Path, event: &Json) -> Result<(), MotionError> {
    let mut line = event.to_string();
    line.push('\n');
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(PROGRESS_FILE));
    let mut file = file.map_err(|source| MotionError::Io {
        doing: "opening the farm's progress stream".into(),
        path: dir.join(PROGRESS_FILE),
        source,
    })?;
    file.write_all(line.as_bytes())
        .map_err(|source| MotionError::Io {
            doing: "writing an event to the farm's progress stream".into(),
            path: dir.join(PROGRESS_FILE),
            source,
        })
}

/// The plan is known: how many parts, how many frames, whether the audio
/// part is among them.
pub(crate) fn said_plan(
    dir: &Path,
    parts: usize,
    frames: u32,
    audio: bool,
) -> Result<(), MotionError> {
    say(
        dir,
        &json!({ "event": "plan", "parts": parts, "frames": frames, "audio": audio }),
    )
}

/// A part began drawing: the range it covers, in the engine's own frames.
pub(crate) fn said_part_start(
    dir: &Path,
    index: usize,
    from: u32,
    to: u32,
) -> Result<(), MotionError> {
    say(
        dir,
        &json!({ "event": "part", "index": index, "from": from, "to": to,
                  "frames": to.saturating_sub(from), "state": "rendering" }),
    )
}

/// The audio part changed state. It is not one of the picture parts the
/// plan counts - it carries no frames - so it is said under its own name
/// rather than borrowing an index.
pub(crate) fn said_audio(dir: &Path, state: &str, seconds: Option<f64>) -> Result<(), MotionError> {
    let mut event = json!({ "event": "audio", "state": state });
    if let Some(seconds) = seconds {
        event["seconds"] = json!(seconds);
    }
    say(dir, &event)
}

/// A part finished: it landed, or the farm's own directory already had it
/// whole.
pub(crate) fn said_part_end(
    dir: &Path,
    index: usize,
    state: &str,
    seconds: Option<f64>,
) -> Result<(), MotionError> {
    let mut event = json!({ "event": "part", "index": index, "state": state });
    if let Some(seconds) = seconds {
        event["seconds"] = json!(seconds);
    }
    say(dir, &event)
}

/// A new render begins a new story: the stream is truncated, because a
/// farm directory that survives its renders would otherwise tell two
/// stories in one file and the reader cannot tell where one ended.
pub(crate) fn restart(dir: &Path) -> Result<(), MotionError> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(dir.join(PROGRESS_FILE));
    file.map(|_| ()).map_err(|source| MotionError::Io {
        doing: "beginning the farm's progress stream".into(),
        path: dir.join(PROGRESS_FILE),
        source,
    })
}

/// The join began.
pub(crate) fn said_join(dir: &Path, parts: usize) -> Result<(), MotionError> {
    say(dir, &json!({ "event": "join", "parts": parts }))
}

/// The render finished: the file it wrote and the seconds the whole call
/// took.
pub(crate) fn said_done(dir: &Path, out: &Path, seconds: f64) -> Result<(), MotionError> {
    say(
        dir,
        &json!({ "event": "done", "out": out.display().to_string(), "seconds": seconds }),
    )
}

/// One part as the reader sees it.
#[derive(Clone, PartialEq, Debug)]
pub struct PartState {
    /// The part's index, in timeline order.
    pub index: usize,
    /// `pending`, `rendering`, `done`, or `served`.
    pub state: String,
    /// How long this part took to draw, when it drew.
    pub seconds: Option<f64>,
}

/// What the stream says about the farm right now.
#[derive(Clone, PartialEq, Debug)]
pub struct FarmProgress {
    /// The parts the plan named, each with the state it is in. A part the
    /// stream has not mentioned yet - the plan arrived, the part has not
    /// started - is `pending`.
    pub parts: Vec<PartState>,
    /// Whether the audio part is among the plan.
    pub audio: bool,
    /// The audio part's state - `rendering`, `done`, `served` - when the
    /// plan carries one and the stream has said it.
    pub audio_state: Option<String>,
    /// The frames planned, across every picture part.
    pub frames: u32,
    /// The frames finished - every `done` or `served` picture part's
    /// frames, summed.
    pub frames_done: u32,
    /// The wall-clock seconds of every part that drew, summed: what the
    /// render has cost so far.
    pub seconds: f64,
    /// The whole-render estimate, from the pace of the parts drawn so far:
    /// the average seconds a frame costs, times the frames not yet done.
    /// `None` before any part has drawn.
    pub eta_seconds: Option<f64>,
    /// The file the finished render wrote, when the stream said `done`.
    pub out: Option<String>,
}

impl FarmProgress {
    /// The progress as one JSON document, for the agent binary and any
    /// caller that answers to a tool.
    pub fn to_json(&self) -> Json {
        json!({
            "live": self.out.is_none(),
            "parts": self.parts.iter().map(|part| json!({
                "index": part.index,
                "state": part.state,
                "seconds": part.seconds,
            })).collect::<Vec<Json>>(),
            "audio": self.audio,
            "audioState": self.audio_state,
            "frames": self.frames,
            "framesDone": self.frames_done,
            "percent": if self.frames > 0 {
                (self.frames_done as f64 / self.frames as f64 * 1000.0).round() / 10.0
            } else {
                0.0
            },
            "seconds": self.seconds,
            "etaSeconds": self.eta_seconds,
            "out": self.out,
        })
    }
}

/// Read a farm's progress: the stream aggregated into what an agent can
/// answer "how long now?" with.
///
/// A farm directory with no stream yet is not an error - a render that has
/// not asked the engine for its plan has nothing to say, and the answer is
/// a whole farm `pending`, because the parts are known the moment the plan
/// is. A stream that ends mid-line (a reader racing the writer) has its
/// torn tail ignored: the next read will see the whole line.
pub fn progress(farm_dir: &Path) -> Result<FarmProgress, MotionError> {
    let text = match std::fs::read_to_string(farm_dir.join(PROGRESS_FILE)) {
        Ok(text) => text,
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
            // No stream: the farm has run nothing yet, or the directory is
            // not a farm. Both say the same honest thing.
            return Ok(FarmProgress {
                parts: Vec::new(),
                audio: false,
                audio_state: None,
                frames: 0,
                frames_done: 0,
                seconds: 0.0,
                eta_seconds: None,
                out: None,
            });
        }
        Err(source) => {
            return Err(MotionError::Io {
                doing: "reading the farm's progress stream".into(),
                path: farm_dir.join(PROGRESS_FILE),
                source,
            });
        }
    };

    let mut read = FarmProgress {
        parts: Vec::new(),
        audio: false,
        audio_state: None,
        frames: 0,
        frames_done: 0,
        seconds: 0.0,
        eta_seconds: None,
        out: None,
    };
    let mut frames_of: std::collections::BTreeMap<usize, u32> = std::collections::BTreeMap::new();
    for line in text.lines() {
        let Ok(event) = serde_json::from_str::<Json>(line) else {
            // A torn tail or a line another writer mangled: ignored, and
            // the events around it still read.
            continue;
        };
        match event.get("event").and_then(Json::as_str) {
            Some("plan") => {
                let planned = event.get("parts").and_then(Json::as_u64).unwrap_or(0) as usize;
                read.audio = event.get("audio").and_then(Json::as_bool).unwrap_or(false);
                read.frames = event.get("frames").and_then(Json::as_u64).unwrap_or(0) as u32;
                read.parts = (0..planned)
                    .map(|index| PartState {
                        index,
                        state: "pending".into(),
                        seconds: None,
                    })
                    .collect();
            }
            Some("part") => {
                let Some(index) = event.get("index").and_then(Json::as_u64) else {
                    continue;
                };
                let index = index as usize;
                // The frames a part covers arrive on its `rendering` event,
                // which the farm always says before the end state; keeping
                // them here means the end event - the line that says `done`
                // - can sum them the moment it arrives.
                if let Some(frames) = event.get("frames").and_then(Json::as_u64) {
                    frames_of.insert(index, frames as u32);
                }
                let Some(state) = event.get("state").and_then(Json::as_str) else {
                    continue;
                };
                let seconds = event.get("seconds").and_then(Json::as_f64);
                let slot = PartState {
                    index,
                    state: state.into(),
                    seconds,
                };
                match read.parts.iter().position(|part| part.index == index) {
                    Some(at) => read.parts[at] = slot,
                    None => read.parts.push(slot),
                }
                if matches!(state, "done" | "served") {
                    if let Some(seconds) = seconds {
                        read.seconds += seconds;
                    }
                    read.frames_done += frames_of.get(&index).copied().unwrap_or(0);
                }
            }
            Some("audio") => {
                read.audio_state = event
                    .get("state")
                    .and_then(Json::as_str)
                    .map(ToString::to_string);
            }
            Some("join") => {}
            Some("done") => {
                read.out = event
                    .get("out")
                    .and_then(Json::as_str)
                    .map(ToString::to_string);
            }
            _ => {}
        }
    }
    // The estimate: frames drawn so far cost `seconds`; the frames not yet
    // drawn cost the same per frame. Before any part draws there is no
    // pace to read, and `None` says so.
    if read.seconds > 0.0 && read.frames_done > 0 {
        let per_frame = read.seconds / read.frames_done as f64;
        let left = read.frames.saturating_sub(read.frames_done);
        read.eta_seconds = Some((left as f64 * per_frame).round());
    }
    Ok(read)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stream read back whole: the plan, each part's states in order,
    /// the done.
    #[test]
    fn a_stream_reads_back_as_it_was_said() {
        let dir = tempfile::tempdir().unwrap();
        said_plan(dir.path(), 2, 60, true).unwrap();
        said_part_start(dir.path(), 0, 0, 30).unwrap();
        said_part_end(dir.path(), 0, "done", Some(4.0)).unwrap();
        said_part_start(dir.path(), 1, 30, 60).unwrap();
        said_part_end(dir.path(), 1, "served", Some(0.1)).unwrap();
        said_audio(dir.path(), "done", Some(1.2)).unwrap();
        said_join(dir.path(), 2).unwrap();
        said_done(dir.path(), Path::new("reel.mp4"), 8.5).unwrap();

        let read = progress(dir.path()).unwrap();
        assert_eq!(read.parts.len(), 2);
        assert_eq!(read.parts[0].state, "done");
        assert_eq!(read.parts[1].state, "served");
        assert!(read.audio);
        assert_eq!(read.audio_state.as_deref(), Some("done"));
        assert_eq!(read.out.as_deref(), Some("reel.mp4"));
        // The JSON an agent answers with names the shape it wants.
        let said = read.to_json();
        assert_eq!(said["live"], json!(false));
        assert_eq!(said["percent"], json!(100.0));
    }

    /// A farm that has said nothing is a whole farm pending - not an
    /// error, because asking about a render that has not started is a
    /// legitimate question with a boring answer.
    #[test]
    fn a_farm_that_has_said_nothing_is_pending_whole() {
        let dir = tempfile::tempdir().unwrap();
        let read = progress(dir.path()).unwrap();
        assert!(read.parts.is_empty());
        assert_eq!(read.frames, 0);
        assert_eq!(read.eta_seconds, None);
        assert!(read.out.is_none());
    }

    /// A torn tail - a reader racing the writer - is ignored, and the
    /// events around it still read.
    #[test]
    fn a_torn_tail_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        said_plan(dir.path(), 2, 60, false).unwrap();
        said_part_start(dir.path(), 0, 0, 30).unwrap();
        said_part_end(dir.path(), 0, "done", Some(2.0)).unwrap();
        std::fs::write(
            dir.path().join(PROGRESS_FILE),
            format!(
                "{}{{\"event\":\"part\",\"index\":1,\"state\":\"rend",
                std::fs::read_to_string(dir.path().join(PROGRESS_FILE)).unwrap(),
            ),
        )
        .unwrap();
        let read = progress(dir.path()).unwrap();
        assert_eq!(read.parts[0].state, "done");
        assert_eq!(
            read.parts[1].state, "pending",
            "the torn line never arrived"
        );
    }

    /// The estimate reads the pace of the parts drawn so far: half the
    /// frames done at four seconds a part says the other half is four
    /// seconds away.
    #[test]
    fn the_estimate_reads_the_pace() {
        let dir = tempfile::tempdir().unwrap();
        said_plan(dir.path(), 2, 60, false).unwrap();
        // Part 0 carried 30 frames (the plan says 60 across two parts).
        said_part_start(dir.path(), 0, 0, 30).unwrap();
        said_part_end(dir.path(), 0, "done", Some(4.0)).unwrap();
        let read = progress(dir.path()).unwrap();
        assert_eq!(read.frames_done, 30);
        assert_eq!(
            read.eta_seconds,
            Some(4.0),
            "the same pace, the same frames left"
        );
    }

    /// A live farm - the stream says no `done` - answers `live`.
    #[test]
    fn a_farm_mid_render_is_live() {
        let dir = tempfile::tempdir().unwrap();
        said_plan(dir.path(), 2, 60, false).unwrap();
        said_part_start(dir.path(), 0, 0, 30).unwrap();
        let read = progress(dir.path()).unwrap();
        let said = read.to_json();
        assert_eq!(said["live"], json!(true));
        assert_eq!(read.parts[1].state, "pending");
    }
}
