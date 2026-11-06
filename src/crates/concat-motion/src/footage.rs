// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The footage-planning engine: what an edit has not shown yet, and what
//! stock footage should cover it.
//!
//! The sound side of the director already knows where the words are and
//! where the soundtrack lands its beats. What was missing is the answer to
//! "what does the picture do there". This module is that answer's engine:
//! it reads a beat grid and a transcript, finds the spans the edit leaves
//! uncovered (dialogue gaps) and the spans it drones over (monotony runs),
//! and hands them back as a [`FootagePlan`] - the plan the slot schema and
//! the stock-search tools spend in later work.
//!
//! The heavy lifting is deliberately borrowed, not rebuilt. Candidate
//! scoring against a gap is [`oximedia_auto`]'s own [`BRollSelector`], and
//! a gap is its [`CoverageGap`]; warm-to-cool colour jumps between adjacent
//! clips are its [`ColorContinuityChecker`]; cut positions come from its
//! [`CutDetector`]. What this module adds is the reading of the edit
//! itself: where the transcript goes quiet, where the beats stop landing,
//! and what the words either side of a quiet span ask a clip to show.
//!
//! ```text
//! beats + transcript
//!   -> dialogue gaps   [3.0 .. 5.4]   2.4 s of no words, tags either side
//!   -> monotony run    [14.2 .. 30.0] 15.8 s of speech, not one beat
//!   -> cut positions   J/L cuts at every dialogue boundary
//!   -> FootagePlan { gaps, suggestions, cuts }
//! ```
//!
//! [`oximedia_auto`]: https://docs.rs/oximedia-auto

use oximedia_auto::b_roll_selector::{
    BRollClip, BRollSelection, BRollSelector, BRollSelectorConfig, CoverageGap, SceneCategory,
};
use oximedia_auto::color_continuity::{
    ClipColorStats, ColorContinuityChecker, ContinuityConfig, Discontinuity,
};
use oximedia_auto::cuts::{Beat, CutConfig, CutDetector, CutPoint, DialogueSegment};
use oximedia_core::types::{Rational, Timestamp};

/// One stretch of spoken words, on the timeline's own clock.
///
/// The planner's input contract: start and end in seconds from the edit's
/// start, and the words themselves. The recogniser's output already is
/// this shape; what a caller without a recogniser passes is its own
/// business, as long as the times are seconds and the order is by start.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TranscriptSegment {
    /// When the words start, in seconds.
    pub start_s: f64,
    /// When the words end, in seconds; after `start_s`.
    pub end_s: f64,
    /// The words.
    pub text: String,
}

/// The words an edit speaks, in order.
///
/// An empty transcript is an edit with no readable speech - a music reel,
/// a soundscape - and the engine answers it with an empty plan rather than
/// a guess.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    /// The segments, in start order. Sorted where they are read; nothing
    /// here trusts the caller's order.
    pub segments: Vec<TranscriptSegment>,
}

impl Transcript {
    /// A transcript from segments, whatever order they arrived in.
    pub fn new(mut segments: Vec<TranscriptSegment>) -> Transcript {
        segments.retain(|s| s.start_s.is_finite() && s.end_s.is_finite() && s.end_s > s.start_s);
        segments.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        Transcript { segments }
    }

    /// Whether there is nothing readable to plan against.
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
}

/// What kind of span a suggestion is about.
///
/// The two reads the engine makes: a span the edit leaves uncovered, and a
/// span it covers with words but lets drone on the screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuggestionKind {
    /// No words cover the span - the picture is on its own.
    DialogueGap,
    /// Words cover the span but no beat lands in it - the screen has
    /// nothing new to cut to for a long stretch.
    MonotonyRun,
}

/// One answer to "does a beat need B-roll here": the span, what it asks
/// for, how long the cover should run and how urgently.
#[derive(Clone, Debug)]
pub struct FootageSuggestion {
    /// The span the suggestion is about, in the selector's own gap shape,
    /// carrying the topic tags the words either side asked for.
    pub gap: CoverageGap,
    /// Which of the two reads found it.
    pub kind: SuggestionKind,
    /// Whether the span wants B-roll. A [`SuggestionKind::MonotonyRun`]
    /// may also be answered by a treatment change - a graphic, a hold -
    /// rather than new footage; the flag says footage is the ask.
    pub need_b_roll: bool,
    /// The broad scene the span asks for. Tags carry the specifics; the
    /// category is the shelf they sort onto, `Unknown` until a caller
    /// knows better.
    pub category: SceneCategory,
    /// How long the cover should run, in milliseconds - the span itself,
    /// unclamped: the caller trims to the slot it builds.
    pub duration_ms: u64,
    /// How urgently the span wants covering, 0.0-1.0: longer gaps and
    /// longer runs score higher, so a plan spends its searching where the
    /// dead air is.
    pub priority: f32,
}

impl FootageSuggestion {
    /// The span's start, in seconds.
    pub fn start_s(&self) -> f64 {
        ms_to_s(self.gap.start_ms)
    }

    /// The span's end, in seconds.
    pub fn end_s(&self) -> f64 {
        ms_to_s(self.gap.end_ms)
    }
}

/// The footage plan a beat grid and a transcript produce: the spans the
/// edit has not shown anything for, what each span asks for, and the cut
/// positions the dialogue's own boundaries propose.
///
/// This is the engine's answer, not the plan document: the slot schema
/// that a plan validates and a search fills is later work. Nothing here
/// searches, fetches or places a clip - it says where the openings are.
#[derive(Clone, Debug, Default)]
pub struct FootagePlan {
    /// Every span the edit leaves uncovered, in the selector's own
    /// [`CoverageGap`] shape, so a caller with a candidate pool can hand
    /// them straight to [`BRollSelector::select_sequence`].
    pub gaps: Vec<CoverageGap>,
    /// What each flagged span - uncovered or monotonous - asks for.
    pub suggestions: Vec<FootageSuggestion>,
    /// Cut positions at the dialogue boundaries: a J-cut before every
    /// stretch of speech and an L-cut after, which is where an editor
    /// would put the picture's own joins. Sorted by time.
    pub cuts: Vec<CutPoint>,
}

/// The suggester: reads a beat grid and a transcript, flags the spans
/// that want footage.
///
/// Two reads, in order. A **dialogue gap** is a span between consecutive
/// segments where no words run for at least [`BRollSuggester::MIN_GAP_S`]:
/// the A-roll has nobody on screen, and stock covers it. A **monotony
/// run** is a chain of back-to-back segments running at least
/// [`BRollSuggester::MIN_MONOTONY_S`] with no beat landing inside it: the
/// voice-over drones while the screen sits still.
#[derive(Clone, Debug)]
pub struct BRollSuggester {
    min_gap_s: f64,
    min_monotony_s: f64,
}

impl BRollSuggester {
    /// The shortest quiet span a gap has to be before it asks for footage.
    /// A beat-length pause between sentences is an edit, not a hole.
    pub const MIN_GAP_S: f64 = 1.2;

    /// The shortest no-beat stretch of continuous speech a monotony run
    /// has to be. Eight seconds is the long side of a shot that works.
    pub const MIN_MONOTONY_S: f64 = 8.0;

    /// How much slack between two segments still counts as one stretch of
    /// speech, for the monotony read: a breath does not break a run.
    const TOUCH_S: f64 = 0.25;

    /// A suggester with the default thresholds.
    pub fn new() -> BRollSuggester {
        BRollSuggester {
            min_gap_s: Self::MIN_GAP_S,
            min_monotony_s: Self::MIN_MONOTONY_S,
        }
    }

    /// A suggester with its own gap threshold, in seconds.
    pub fn with_min_gap_s(mut self, min_gap_s: f64) -> Self {
        self.min_gap_s = min_gap_s.max(0.0);
        self
    }

    /// A suggester with its own monotony threshold, in seconds.
    pub fn with_min_monotony_s(mut self, min_monotony_s: f64) -> Self {
        self.min_monotony_s = min_monotony_s.max(0.0);
        self
    }

    /// Flag the spans that want footage, as suggestions. The returned
    /// order is by time: gaps and runs interleaved as they land.
    pub fn suggest(&self, beats: &[Beat], transcript: &Transcript) -> Vec<FootageSuggestion> {
        let mut suggestions = Vec::new();
        for gap in self.dialogue_gaps(transcript) {
            let tags = tags_around(transcript, gap.start_ms, gap.end_ms);
            let span_ms = (gap.end_ms - gap.start_ms).max(0);
            suggestions.push(FootageSuggestion {
                gap: topic_gap(gap, &tags),
                kind: SuggestionKind::DialogueGap,
                need_b_roll: true,
                category: SceneCategory::Unknown,
                duration_ms: span_ms as u64,
                priority: (span_ms as f64 / 6000.0).clamp(0.25, 1.0) as f32,
            });
        }
        for run in self.monotony_runs(beats, transcript) {
            let tags = tags_around(transcript, run.start_ms, run.end_ms);
            let span_ms = (run.end_ms - run.start_ms).max(0);
            suggestions.push(FootageSuggestion {
                gap: topic_gap(run, &tags),
                kind: SuggestionKind::MonotonyRun,
                need_b_roll: true,
                category: SceneCategory::Unknown,
                duration_ms: span_ms as u64,
                priority: (span_ms as f64 / 24_000.0).clamp(0.3, 0.8) as f32,
            });
        }
        suggestions.sort_by_key(|s| s.gap.start_ms);
        suggestions
    }

    /// The spans between consecutive segments where no words run for at
    /// least the gap threshold. Leading and trailing silence are the
    /// edit's own choices, not holes - only the quiet between words asks.
    fn dialogue_gaps(&self, transcript: &Transcript) -> Vec<CoverageGap> {
        let mut gaps = Vec::new();
        for pair in transcript.segments.windows(2) {
            let quiet_ms = to_ms(pair[1].start_s) - to_ms(pair[0].end_s);
            if quiet_ms >= to_ms(self.min_gap_s) {
                gaps.push(CoverageGap {
                    start_ms: to_ms(pair[0].end_s),
                    end_ms: to_ms(pair[1].start_s),
                    topic_tags: Vec::new(),
                });
            }
        }
        gaps
    }

    /// The chains of back-to-back speech running past the monotony
    /// threshold with no beat landing inside them. A chain is segments
    /// that touch within [`BRollSuggester::TOUCH_S`]; a beat anywhere in
    /// the chain's span breaks the run, because a cut the soundtrack
    /// asks for is variation, whatever the words are doing.
    fn monotony_runs(&self, beats: &[Beat], transcript: &Transcript) -> Vec<CoverageGap> {
        let mut runs = Vec::new();
        let mut chain: Option<(f64, f64)> = None;
        for segment in &transcript.segments {
            match &mut chain {
                Some((_, end)) if to_ms(segment.start_s) - to_ms(*end) <= to_ms(Self::TOUCH_S) => {
                    *end = segment.end_s;
                }
                _ => chain = Some((segment.start_s, segment.end_s)),
            }
            let Some((start, end)) = chain else {
                continue;
            };
            let span_ms = to_ms(end) - to_ms(start);
            if span_ms < to_ms(self.min_monotony_s) {
                continue;
            }
            let beaten = beats.iter().any(|beat| {
                let at = beat.timestamp.to_seconds();
                at > start && at < end
            });
            if !beaten {
                runs.push(CoverageGap {
                    start_ms: to_ms(start),
                    end_ms: to_ms(end),
                    topic_tags: Vec::new(),
                });
                // One flag per droning stretch: the chain is closed out by
                // the flag itself, so a run that keeps going is one ask.
                chain = None;
            }
        }
        runs
    }
}

impl Default for BRollSuggester {
    fn default() -> Self {
        BRollSuggester::new()
    }
}

/// The footage plan for a beat grid and a transcript: where the edit has
/// not shown anything, what each span asks for, and where the dialogue's
/// own boundaries put the picture's joins.
///
/// One entry point on purpose: everything the planning layer needs from
/// beats and words, in one call, with no search and no fetching behind it.
/// An empty transcript is not an error - an edit with no readable speech
/// gets an empty plan, the same way an empty beat grid gets no cuts.
pub fn suggest_footage(beats: &[Beat], transcript: &Transcript) -> FootagePlan {
    let suggester = BRollSuggester::new();
    let suggestions = suggester.suggest(beats, transcript);
    let gaps = suggestions
        .iter()
        .filter(|s| s.kind == SuggestionKind::DialogueGap)
        .map(|s| s.gap.clone())
        .collect();
    FootagePlan {
        gaps,
        suggestions,
        cuts: dialogue_boundary_cuts(beats, transcript),
    }
}

/// Fill coverage gaps from a pool of candidate clips, with the crate's own
/// selector: coverage, variety and topic relevance, reuse penalised.
///
/// A gap the pool cannot cover comes back `None` rather than a bad pick -
/// the plan's job is to say what is missing, not to fill it with anything.
/// A seam the search-facing layers spend: [`suggest_footage`] plans the
/// openings, this one scores candidates against them.
pub fn fill_from_pool(gaps: &[CoverageGap], pool: &[BRollClip]) -> Vec<Option<BRollSelection>> {
    let mut selector = BRollSelector::new(pool.to_vec(), BRollSelectorConfig::new());
    selector.select_sequence(gaps).unwrap_or_default()
}

/// Flag the adjacent-clip colour jumps the continuity checker catches,
/// warm-to-cool shifts above its threshold among them.
///
/// The stats a caller passes come from frames it has already looked at;
/// the checker is the crate's own arithmetic, not a second opinion here.
/// A seam the plan-level continuity gate spends: adjacent clips' stats
/// in, the warm-to-cool jumps and their remedies out.
pub fn colour_jumps(stats: &[ClipColorStats]) -> Vec<Discontinuity> {
    ColorContinuityChecker::new(ContinuityConfig::new())
        .flagged(stats)
        .unwrap_or_default()
}

/// The cut positions the dialogue's own boundaries propose: a J-cut
/// before every stretch of speech, an L-cut after, beat-aligned once
/// scene changes join them. Sorted by time, as [`CutDetector`] leaves them.
fn dialogue_boundary_cuts(beats: &[Beat], transcript: &Transcript) -> Vec<CutPoint> {
    let dialogue: Vec<DialogueSegment> = transcript
        .segments
        .iter()
        .map(|s| DialogueSegment::new(at_seconds(s.start_s), at_seconds(s.end_s), 1.0))
        .collect();
    CutDetector::new(CutConfig::new())
        .detect_cuts(&[], Some(beats), Some(&dialogue))
        .unwrap_or_default()
}

/// A timestamp on the planning timebase: one millisecond a tick, so a
/// transcript's seconds convert by ×1000 and back without drift.
fn at_seconds(seconds: f64) -> Timestamp {
    Timestamp::new(to_ms(seconds), Rational::new(1, 1000))
}

/// Seconds to the planner's millisecond PTS, rounded.
fn to_ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

/// The planner's millisecond PTS back to seconds.
fn ms_to_s(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

/// A gap carrying the topic tags the words around it asked for: the
/// lowercase words of the segments either side, short enough to be
/// content, few enough to stay a summary.
fn topic_gap(mut gap: CoverageGap, tags: &[String]) -> CoverageGap {
    gap.topic_tags = tags.to_vec();
    gap
}

/// The words of the segments either side of a span, lowercased, three
/// characters or more, none of them filler, deduplicated in order, at
/// most six: the summary a topic-relevance score reads, not a caption.
fn tags_around(transcript: &Transcript, start_ms: i64, end_ms: i64) -> Vec<String> {
    /// The glue words every caption carries, which score relevance on
    /// nothing: a tag of `the` matches every clip and sorts none of them.
    const STOPWORDS: [&str; 8] = ["the", "and", "but", "for", "that", "with", "from", "this"];
    let mut tags: Vec<String> = Vec::new();
    for segment in &transcript.segments {
        let seg_start = to_ms(segment.start_s);
        let seg_end = to_ms(segment.end_s);
        // A segment flanks the span when it ends at or after its start or
        // begins at or before its end: the words leading in and leading out.
        let flanks = (seg_end >= start_ms && seg_end <= end_ms)
            || (seg_start >= start_ms && seg_start <= end_ms)
            || (seg_start <= start_ms && seg_end >= end_ms);
        if !flanks {
            continue;
        }
        for word in segment.text.split(|c: char| !c.is_alphanumeric()) {
            let word = word.to_lowercase();
            if word.len() >= 3 && !STOPWORDS.contains(&word.as_str()) && !tags.contains(&word) {
                tags.push(word);
                if tags.len() == 6 {
                    return tags;
                }
            }
        }
    }
    tags
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A beat at `seconds`, strong and on the downbeat, on the planning
    /// timebase.
    fn beat(seconds: f64, strength: f64) -> Beat {
        Beat::new(at_seconds(seconds), strength, true)
    }

    /// A spoken stretch from `start` to `end` seconds.
    fn said(start: f64, end: f64, text: &str) -> TranscriptSegment {
        TranscriptSegment {
            start_s: start,
            end_s: end,
            text: text.into(),
        }
    }

    /// A transcript with three obvious dialogue gaps - 2.4 s, 3.0 s and
    /// 2.7 s of no words between sentences - and one monotony run: 15.8 s
    /// of continuous speech with not one beat landing in it. The beats
    /// land inside the shorter stretches, where the edit is alive.
    fn synthetic_transcript() -> (Transcript, Vec<Beat>) {
        let transcript = Transcript::new(vec![
            said(0.5, 3.0, "the city skyline at dusk"),
            said(5.4, 7.0, "crowds crossing the street"),
            said(10.0, 11.5, "the market opens"),
            said(
                14.2,
                30.0,
                "a long explanation that runs on while the screen sits still",
            ),
        ]);

        let beats = vec![
            beat(1.0, 0.9),
            beat(2.0, 0.6),
            beat(6.0, 0.7),
            beat(10.5, 0.8),
            beat(12.0, 0.5),
        ];
        (transcript, beats)
    }

    /// The done-when: three dialogue gaps and one monotony run, each
    /// flagged with the kind that found it, in time order.
    #[test]
    fn three_dialogue_gaps_and_one_monotony_run_are_flagged() {
        let (transcript, beats) = synthetic_transcript();
        let plan = suggest_footage(&beats, &transcript);

        let gaps: Vec<_> = plan
            .suggestions
            .iter()
            .filter(|s| s.kind == SuggestionKind::DialogueGap)
            .collect();
        assert_eq!(gaps.len(), 3, "{:?}", plan.suggestions);
        assert_eq!(
            plan.gaps.len(),
            3,
            "the plan's own gaps are the dialogue gaps only"
        );
        // The gaps carry the words' own ask: the tags around the first
        // quiet span are the city's, lowercase, filler dropped.
        assert!(
            plan.gaps[0].topic_tags.starts_with(&[
                "city".to_owned(),
                "skyline".to_owned(),
                "dusk".to_owned()
            ]),
            "{:?}",
            plan.gaps
        );
        // The gaps, in seconds, where the words go quiet.
        let spans: Vec<(f64, f64)> = gaps.iter().map(|s| (s.start_s(), s.end_s())).collect();
        for (start, end) in &spans {
            assert!(
                end - start >= BRollSuggester::MIN_GAP_S,
                "every flagged gap clears the threshold: {spans:?}"
            );
        }
        assert!(
            spans.contains(&(3.0, 5.4)) && spans.contains(&(7.0, 10.0)),
            "the quiet spans are the transcript's own: {spans:?}"
        );

        let runs: Vec<_> = plan
            .suggestions
            .iter()
            .filter(|s| s.kind == SuggestionKind::MonotonyRun)
            .collect();
        assert_eq!(runs.len(), 1, "one monotony run: {:?}", plan.suggestions);
        let run = runs[0];
        assert!(
            (run.start_s() - 14.2).abs() < 1e-6 && (run.end_s() - 30.0).abs() < 1e-6,
            "the run is the long no-beat stretch: {:?}",
            run.gap
        );
        assert!(run.need_b_roll);
        // Every suggestion says how long its cover should run.
        for suggestion in &plan.suggestions {
            assert!(suggestion.duration_ms > 0, "{suggestion:?}");
            assert!((0.0..=1.0).contains(&suggestion.priority), "{suggestion:?}");
        }
    }

    /// An edit with no readable speech - and no beats - plans nothing and
    /// panics nowhere.
    #[test]
    fn an_empty_transcript_plans_nothing_and_panics_nowhere() {
        let plan = suggest_footage(&[], &Transcript::default());
        assert!(plan.gaps.is_empty(), "{plan:?}");
        assert!(plan.suggestions.is_empty(), "{plan:?}");
        assert!(plan.cuts.is_empty(), "{plan:?}");

        // Beats without words: no gaps, no runs, and no cuts either - the
        // boundaries the cut detector proposes are the dialogue's own.
        let (transcript, beats) = synthetic_transcript();
        let empty = Transcript::default();
        let plan = suggest_footage(&beats, &empty);
        assert!(plan.suggestions.is_empty(), "{plan:?}");
        assert!(plan.cuts.is_empty(), "{plan:?}");
        // And the real transcript without beats still finds its gaps: the
        // two reads are independent.
        let plan = suggest_footage(&[], &transcript);
        assert_eq!(plan.suggestions.len(), 4, "{plan:?}");
    }

    /// The selector fills a gap from a pool by the tags the words asked
    /// for, and leaves a gap the pool cannot cover untouched.
    #[test]
    fn the_selector_fills_a_gap_by_the_words_around_it() {
        let pool = vec![
            BRollClip::new("city.aerial", 4_000, vec!["city".into(), "skyline".into()])
                .with_category(SceneCategory::Aerial),
            BRollClip::new("ocean.waves", 6_000, vec!["ocean".into(), "waves".into()])
                .with_category(SceneCategory::Outdoor),
        ];
        let gap = CoverageGap {
            start_ms: 3_000,
            end_ms: 5_400,
            topic_tags: vec!["city".into(), "skyline".into()],
        };
        let filled = fill_from_pool(std::slice::from_ref(&gap), &pool);
        let selection = filled[0].as_ref().expect("the city clip covers it");
        assert_eq!(selection.clip_id, "city.aerial");
        assert!(selection.score > 0.0);

        // A gap the pool has nothing topical for still gets the best clip
        // there is - the selector covers first, scores relevance second -
        // but it scores plainly: no tag overlap, lower composite.
        let alien = CoverageGap {
            start_ms: 0,
            end_ms: 2_000,
            topic_tags: vec!["underwater".into(), "volcano".into()],
        };
        let filled = fill_from_pool(&[alien], &pool);
        let fallback = filled[0].as_ref().expect("the pool still covers it");
        assert_eq!(fallback.relevance_score, 0.0, "no tag overlap");
        assert!(
            fallback.score < selection.score,
            "the topical clip outscores the fallback: {fallback:?} vs {selection:?}"
        );
        // An empty pool leaves the gap unfilled: no candidates, no pick.
        let gap = CoverageGap {
            start_ms: 3_000,
            end_ms: 5_400,
            topic_tags: vec!["city".into()],
        };
        assert!(fill_from_pool(&[gap], &[])[0].is_none());
    }

    /// A warm clip butting a cool one is flagged: the checker's own
    /// arithmetic, wired through the planning layer.
    #[test]
    fn a_warm_to_cool_jump_is_flagged() {
        let mut warm = ClipColorStats::new(0, at_seconds(0.0), at_seconds(4.0));
        warm.mean_luminance = 0.5;
        warm.luminance_std = 0.2;
        warm.mean_saturation = 0.4;
        warm.color_temperature = 0.8;
        warm.luminance_histogram = [0.5; 16];
        let mut cool = warm.clone();
        cool.clip_index = 1;
        cool.start = at_seconds(4.0);
        cool.end = at_seconds(8.0);
        cool.color_temperature = -0.8;

        let flags = colour_jumps(&[warm.clone(), cool.clone()]);
        assert_eq!(flags.len(), 1, "{flags:?}");
        assert!(
            flags[0].temperature_delta > 1.0,
            "the jump is the temperature: {flags:?}"
        );

        // Two clips of one temperature keep the jump to the one real
        // edge: warm-warm-cool flags exactly the warm-cool pair.
        let mut warm_again = cool.clone();
        warm_again.clip_index = 2;
        warm_again.color_temperature = 0.8;
        let flags = colour_jumps(&[warm, warm_again, cool]);
        assert_eq!(flags.len(), 1, "one real edge: {flags:?}");
        assert!(flags[0].temperature_delta > 1.0, "{flags:?}");
    }

    /// The cut positions land on the dialogue's own boundaries: a J-cut
    /// before each stretch of speech, an L-cut after, in time order.
    #[test]
    fn cuts_land_on_the_dialogue_boundaries() {
        let (transcript, beats) = synthetic_transcript();
        let plan = suggest_footage(&beats, &transcript);
        // Four stretches of speech, a J and an L each.
        assert_eq!(plan.cuts.len(), 8, "{:?}", plan.cuts);
        for pair in plan.cuts.windows(2) {
            assert!(
                pair[0].timestamp.pts <= pair[1].timestamp.pts,
                "the cuts are in time order: {:?}",
                plan.cuts
            );
        }
        for (cut, segment) in plan.cuts.chunks(2).zip(transcript.segments.iter()) {
            assert_eq!(cut[0].cut_type, oximedia_auto::cuts::CutType::JCut);
            assert_eq!(cut[1].cut_type, oximedia_auto::cuts::CutType::LCut);
            assert_eq!(
                cut[0].timestamp.pts,
                to_ms(segment.start_s) - 200,
                "the J-cut leads the words by 200 ms"
            );
            assert_eq!(cut[1].timestamp.pts, to_ms(segment.end_s) + 200);
        }
    }
}
