// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The captions importer: SRT and whisper-shaped JSON into a comp of timed
//! text nodes. Every cue becomes one text node that lives exactly its cue's
//! window, in the caption zone of a 1920x1080 frame - the comp a plate
//! plays under, the same shape the captions module builds by hand.
//!
//! The two formats are sniffed, not asked for: a document with a `-->`
//! timestamp line is SRT; one with a `segments` array is whisper JSON (the
//! shape both openai-whisper and whisper.cpp write). Anything else is
//! refused as neither.

use concat_motion::comp::{Comp, FORMAT, Node, NodeKind, Rgba, TextAlign, TextNode, Transform};
use serde_json::Value as Json;

use crate::{AdapterError, Import};

/// The character-class slug the other adapters share.
fn slug(name: &str) -> String {
    let mut slug = String::with_capacity(name.len());
    for (i, ch) in name.char_indices() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
        } else if i > 0 && !slug.ends_with('-') && !slug.is_empty() {
            slug.push('-');
        }
    }
    slug.trim_matches('-').to_string()
}

/// `HH:MM:SS,mmm` or `HH:MM:SS.mmm` into seconds.
fn seconds_of(stamp: &str) -> Option<f64> {
    let stamp = stamp.trim();
    let (clock, fraction) = match stamp.split_once([',', '.']) {
        Some((clock, fraction)) => (clock, fraction),
        None => (stamp, "0"),
    };
    let parts: Vec<f64> = clock
        .split(':')
        .map(|part| part.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    if parts.len() != 3 {
        return None;
    }
    let millis: f64 = fraction.parse().ok()?;
    Some(parts[0] * 3600.0 + parts[1] * 60.0 + parts[2] + millis / 1000.0)
}

/// The cues of an SRT document: blocks separated by blank lines, each an
/// optional index line, a `start --> end` line, and the text.
fn srt_cues(text: &str) -> Option<Vec<(f64, f64, String)>> {
    let mut cues = Vec::new();
    for block in text.split("\n\n") {
        let lines: Vec<&str> = block.lines().map(str::trim).collect();
        if lines.is_empty() {
            continue;
        }
        // The line with the arrow is the timing line, wherever it sits.
        let Some(timing) = lines.iter().position(|line| line.contains("-->")) else {
            continue;
        };
        let (start, end) = line_cues(lines[timing])?;
        let body: Vec<&str> = lines[timing + 1..]
            .iter()
            .copied()
            .filter(|line| !line.is_empty())
            .collect();
        if body.is_empty() {
            continue;
        }
        cues.push((start, end, body.join("\n")));
    }
    Some(cues)
}

/// `start --> end` on one line, with the end's trailing settings ignored.
fn line_cues(line: &str) -> Option<(f64, f64)> {
    let (start, rest) = line.split_once("-->")?;
    let end = rest.split_whitespace().next().unwrap_or("");
    let start = seconds_of(start)?;
    let end = seconds_of(end).filter(|end| *end > start)?;
    Some((start, end))
}

/// The cues of a whisper-shaped document: `segments: [{start, end, text}]`.
fn whisper_cues(document: &Json) -> Option<Vec<(f64, f64, String)>> {
    let segments = document.get("segments")?.as_array()?;
    let mut cues = Vec::with_capacity(segments.len());
    for segment in segments {
        let start = segment.get("start")?.as_f64()?;
        let end = segment.get("end")?.as_f64()?;
        let text = segment.get("text")?.as_str()?.trim().to_string();
        if end <= start || text.is_empty() {
            continue;
        }
        cues.push((start, end, text));
    }
    Some(cues)
}

/// Convert a captions document - SRT or whisper JSON - into a comp of timed
/// text nodes, one per cue, in the caption zone.
pub fn import(text: &str, stem: &str) -> Result<Import, AdapterError> {
    let not_captions = |said: &str| AdapterError::NotTheFormat {
        format: "captions (SRT or whisper JSON)",
        said: said.into(),
    };
    let document: Json = serde_json::from_str(text.trim()).unwrap_or(Json::Null);
    let cues = if document.get("segments").and_then(Json::as_array).is_some() {
        whisper_cues(&document)
    } else if text.contains("-->") {
        srt_cues(text)
    } else {
        None
    };
    let Some(cues) = cues else {
        return Err(not_captions(
            "the document is neither an SRT (no `-->` timing line) nor whisper JSON (no `segments` array)",
        ));
    };
    if cues.is_empty() {
        return Err(not_captions("the document carries no cues"));
    }
    let name = slug(stem);
    let id = format!(
        "captions.{}",
        if name.is_empty() {
            "transcript"
        } else {
            name.as_str()
        }
    );
    let duration = cues.iter().map(|(_, end, _)| *end).fold(0.0_f64, f64::max) + 0.4;
    let mut import = Import::clean(Comp {
        format: FORMAT,
        id,
        name: "Captions".into(),
        width: 1920,
        height: 1080,
        fps: 30.0,
        duration,
        background: Rgba::new(0.0, 0.0, 0.0, 0.0),
        assets: Default::default(),
        curves: Default::default(),
        nodes: Vec::with_capacity(cues.len()),
        comps: Default::default(),
        slots: Vec::new(),
    });
    for (index, (start, end, text)) in cues.iter().enumerate() {
        import.comp.nodes.push(Node {
            id: format!("cue-{}", index + 1),
            parent: None,
            start: *start,
            duration: (end - start).max(0.2),
            transform: Transform {
                position: Some(concat_motion::comp::Point::new(960.0, 872.0)),
                ..Transform::default()
            },
            opacity: 1.0,
            blend: concat_motion::comp::Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind: NodeKind::Text(Box::new(TextNode {
                text: text.clone(),
                font: "Inter, DejaVu Sans".into(),
                size: 64.0,
                weight: 700,
                color: Rgba::hex("#f4f1ea"),
                align: TextAlign::Center,
                padding: 22.0,
                background: Rgba::new(0.047, 0.051, 0.063, 0.92),
                radius: 16.0,
                ..TextNode::default()
            })),
        });
    }
    Ok(import)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An SRT with three cues becomes three timed text nodes, the last cue
    /// ending exactly where the comp runs out.
    #[test]
    fn an_srt_becomes_timed_text_nodes() {
        let srt = "
1
00:00:01,000 --> 00:00:04,000
First line of the caption.

2
00:00:05.5 --> 00:00:07,250
Second, with a dot clock.

00:00:08,000 --> 00:00:10,000
Third, no index line.
";
        let made = import(srt, "voiceover").expect("converts");
        assert_eq!(made.comp.id, "captions.voiceover");
        assert_eq!(made.comp.nodes.len(), 3);
        assert!(
            (made.comp.duration - 10.4).abs() < 1e-9,
            "{}",
            made.comp.duration
        );
        let first = made.comp.node("cue-1").expect("the first cue");
        assert_eq!(first.start, 1.0);
        assert!((first.duration - 3.0).abs() < 1e-9);
        let NodeKind::Text(text) = &first.kind else {
            panic!("a text node");
        };
        assert_eq!(text.text, "First line of the caption.");
        let third = made.comp.node("cue-3").expect("the third cue");
        assert_eq!(third.start, 8.0);
        // Every cue validates by the library's own bar.
        assert!(
            made.comp.validate().is_empty(),
            "{:?}",
            made.comp.validate()
        );
    }

    /// Whisper JSON - the shape both openai-whisper and whisper.cpp write -
    /// reads the same, segments and all.
    #[test]
    fn whisper_json_reads_the_same() {
        let json = r#"{
            "language": "en",
            "segments": [
                {"id": 0, "start": 0.5, "end": 2.0, "text": " Hello"},
                {"id": 1, "start": 2.4, "end": 4.8, "text": " world."}
            ],
            "text": " Hello world."
        }"#;
        let made = import(json, "take-one").expect("converts");
        assert_eq!(made.comp.id, "captions.take-one");
        assert_eq!(made.comp.nodes.len(), 2);
        let second = made.comp.node("cue-2").expect("the second cue");
        assert!((second.start - 2.4).abs() < 1e-9);
        let NodeKind::Text(text) = &second.kind else {
            panic!("a text node");
        };
        assert_eq!(text.text, "world.");
    }

    /// A document that is neither is refused, and so is one with no cues.
    #[test]
    fn neither_format_is_refused() {
        assert!(import("just words", "x").is_err());
        assert!(import("1\n00:00 --> 00:02\n\n2\nbad", "y").is_err());
    }
}
