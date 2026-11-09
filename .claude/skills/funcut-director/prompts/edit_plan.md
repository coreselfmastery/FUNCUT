# Writing the edit plan

One reasoning pass, one JSON object, per `templates/edit_plan.schema.json`
(a complete worked example sits beside it). The plan is your contract: you
execute what you wrote, and the review checks it.

Before writing the plan, read `prompts/creative_visual_direction.md`.
The plan must be an original response to this film's story, not a reusable
layout. Choose the visual medium beat by beat from the communication job;
do not enforce a universal B-roll/motion percentage. Major visual beats
need a short rationale (job, medium, reason, film-specific style, transition,
fallback). Use presets as starting points only; inspect, customize, preview,
bake, import, place, and verify designed motion. Inspect stock candidates
and act on continuity feedback. An asset not placed on the timeline is not
an executed visual decision.

After drafting, call
`plan_validate` with the complete object and fix errors before gathering
assets; it validates structure and timeline continuity, not creative quality.

## Fill order

1. **Segments from the transcript.** Each `{start,end}` comes from the
   transcription's segments (already tightened by `clip_cut_spans`). Sort
   them; adjacent segments share boundaries; a gap or overlap is a plan error.
2. **A visual for every segment** — `broll` with a query, or
   `textCard`/`titleCard` when search fails or the beat is typographic.
   Choose motion per `prompts/keyframes_motion.md`.
2b. **Slots for the footage openings**, beside the segments. One slot per
   opening the picture must spend: `slotId` (act plus number, `"hook.1"`),
   `role` (the seven acts), `treatment` (`broll` shops; `hold`/`graphic`/
   `black` are drawn and take no queries), `job`, `entry`/`exit` (against
   the emphasis words and the beat grid), the rich `description` (twelve
   to twenty-five words of nouns and adjectives, forty characters
   minimum — the search's ranked contract), fallback `queries` (keywords
   only; b-roll takes at least one), `hero`, `motionMin`, `durationS`.
   There is no universal B-roll share target; decide the mix from this
   film's story, evidence, emotion, and available material. Phase 4 shops each
   b-roll slot with `openvid_rank_for_slot` and gates the filled
   manifest with `footage_continuity` before anything is fetched.
3. **SFX with `peakAt` on the segment's first frame** (the cut), not the word.
   See `prompts/sfx_design.md` for query writing.
4. **`transitionIn` only where the SECTION changes** — most segments take a
   plain cut (`null`). The 122 transitions and their uses:
   `prompts/transitions_guide.md`.
5. **Audio beds** — voiceover block mirrors the TTS you already made; bgm
   query per `prompts/music_and_duck.md` with `alignTo` and absolute `duckDb`
   gain targets. `sounds_add` has no gain field: apply each planned SFX dB
   value to its actual clip id with `clip_audio_levels_db`.
6. **Captions + grade** — style per niche (`prompts/niche_playbooks.md`,
   `prompts/color_grading.md`), ONE filter for the whole video.
7. **Export block** — `output` path, crf 20, codec h264, platform if told.

## The math that makes the plan executable

- **B-roll runs long, then trims:** prefer a source clip 1.5–2× the segment's
  length; set `trimIn` past the clip's soft opening (first ~0.5 s often
  settles) and let `trimOut` overshoot — `clip_trim` lands the exact edge
  after placing.
- **J-cut feel:** the visual for segment N+1 enters 100–200 ms before its
  narration starts. Compute: `visualStart = max(0, segment.start - 0.15)` so
  the first shot never starts before the timeline.
- **Peaks on cuts:** `sfx.peakAt = segment.start` (± 1 frame). If the SFX is
  a riser, it must ARRIVE at the cut: `peakAt = segment.start`, started
  1–2 s earlier — sounds support the cut, they never follow it.
- **Downbeats:** with `analyze_beats` giving `beatsMs[]`, downbeats ≈ every
  4th beat; section boundaries move to the nearest downbeat (± ≤300 ms is
  fine for speech integrity — speech wins over beats when they conflict).

## Quality gates on the plan itself

- [ ] Every segment has a visual; no two segments reference the same clip id
  within 20 s (no visible reuse).
- [ ] Every query is concrete: noun + action/setting + shot feel ("aerial
  canyon river sunrise"), not a mood word alone.
- [ ] Every slot description is twelve to twenty-five words of nouns and
  adjectives (forty characters minimum); drawn treatments carry no queries,
  b-roll at least one; choose the footage/graphics/holds balance for this
  story rather than meeting a universal percentage, and vary shot scale
  intentionally within each section.
- [ ] Captions use measured segment timing (the default transcription output
  is not word-aligned); keep text concise where cue duration permits, with
  style fields and safe area set (offsetY 0.30–0.40, fontSize 0.04–0.06).
- [ ] One filter; intensity within `effects_list`'s stated min/max.
- [ ] Export platform matches the brief's aspect (9:16 for shorts/reels/
  tiktok, 16:9 otherwise) and the segment spans sum to the runtime ±10%.
- [ ] The last segment loops the first (image or word).

## After the plan

Emit it in your reply (it is the artifact the user reviews), then execute
Phase 4–8 of SKILL.md. Changes mid-execution are amendments — restate the
affected segment, do not silently drift from the written plan.
