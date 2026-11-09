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
   Set root `audioClipId` only when `project_get` has returned the real
   timeline clip id of the already-placed voiceover; otherwise omit it. The
   runner reuses that clip, or imports and places the supplied voiceover on
   its own lane. It never cuts or retimes audio behind the plan's back. If
   dead air was cut before planning, use that edited timeline clip and write
   segment times against its edited duration.
2. **A visual for every segment** — the canonical plan is segment-based;
   do not create a separate `slots` array. For `broll`, provide a concrete
   `query`, a 40+ character semantic `description`, optional fallback
   `queries`, `hero`, `motionMin`, intended `shotScale`, and semantic
   `category` for continuity. The Director
   ranks unique candidates, checks continuity, imports them, places them at
   the segment start, and trims them to the segment duration.
3. **Designed motion is executable, not a label.** For a `textCard` or
   `titleCard` that needs motion, include a complete `visual.storyboard`
   accepted by the `motion_reel` tool. FUNCUT renders it, imports the movie,
   creates a dedicated top video track, places it at the segment start, and
   trims it to the beat. A typographic segment without a storyboard must not
   be represented as already-rendered motion. Use `existing` only when its
   `clipId` or `mediaId` is a real asset already in the project. There is
   no universal B-roll/motion share target: choose the medium from the
   communication job and evidence of this film.
4. **SFX with `peakAt` on the segment's first frame** (the cut), not the word.
   See `prompts/sfx_design.md` for query writing.
5. **`transitionIn` only where the SECTION changes** — most segments take a
   plain cut (`null`). The 122 transitions and their uses:
   `prompts/transitions_guide.md`.
6. **Audio beds** — voiceover block mirrors the TTS you already made; bgm
   query per `prompts/music_and_duck.md` with `alignTo` and absolute `duckDb`
   gain targets. `sounds_add` has no gain field: apply each planned SFX dB
   value to its actual clip id with `clip_audio_levels_db`.
7. **Captions + grade** — style per niche (`prompts/niche_playbooks.md`,
   `prompts/color_grading.md`), ONE filter for the whole video.
8. **Export block** — `output` path, crf 20, codec h264, platform if told.

## The math that makes the plan executable

- **B-roll runs long, then trims:** choose a source at least as long as the
  segment. The Director rejects too-short candidates before editing, imports
  the selected source, and trims its end to the planned segment duration.
- **Motion reels are placed, not just rendered:** the storyboard's rendered
  duration must cover the segment; the resulting movie is imported and placed
  on a dedicated top video track at the segment start.
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
- [ ] Every B-roll segment has a concrete query and a 40+ character semantic
  description; no invented clip ids. Motion-led text/title segments have a
  valid storyboard; existing footage names a real imported clip/media id.
  Choose the footage/graphics/holds balance for this story rather than a
  universal percentage, and vary shot scale intentionally within each section.
- [ ] Captions use measured segment timing (the default transcription output
  is not word-aligned); keep text concise where cue duration permits, with
  style fields and safe area set (offsetY 0.30–0.40, fontSize 0.04–0.06).
- [ ] One filter; intensity within `effects_list`'s stated min/max.
- [ ] Export platform matches the brief's aspect (9:16 for shorts/reels/
  tiktok, 16:9 otherwise) and the segment spans sum to the runtime ±10%.
- [ ] The last segment loops the first (image or word).

## After the plan

Emit it in your reply (it is the artifact the user reviews), then execute
the segment-based Director pipeline: validate readiness, rank footage,
repair continuity, import/place/trim stock, render/import/place/trim motion,
export and wait, inspect the rendered frames/audio, and score the result.
A craft score below 7 is a stop requiring a new creative revision; never
publish it as complete. Changes mid-execution are amendments — restate the
affected segment, do not silently drift from the written plan.
