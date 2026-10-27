# Text, titles and captions

Text is a system, not a decoration: four zones, one palette, every
element moving with intent.

## The zones (one occupant at a time)

| Zone | Where | What lives there |
| --- | --- | --- |
| Caption band | ~70–80% of frame height, centered | captions (`captions_add` or the kinetic comps) |
| Upper third | top 33%, centered or left | section titles, chapter words, hooks |
| Lower third | bottom 33%, anchored LEFT | lower thirds (names, roles, sources) |
| Center | the middle | stat cards, chapter chips, big claims — the comp layer's home |

Two text elements in one zone at the same moment is a collision — check
at review with `preview_frame` at the busiest frame. On 9:16, keep 10%
clear at the top (platform chrome: username, follow button) and 12% at
the bottom (caption bar, buttons); the caption band sits INSIDE that,
not over it.

## The palette (fixed in the plan)

Four tokens, chosen once:

- **ink** — the text color: white (#F7F9FF) over dark or busy footage,
  near-black (#0A0C10) over bright footage. Pick by looking at the actual
  frames, not by habit.
- **accent** — ONE color per video, from the niche palette (finance:
  #22D3C5 teal or #FFC247 amber; tech: a single electric hue). The accent
  appears in captions' active word, chapter chips' bars, stat numbers —
  one accent, everywhere, is what makes it a system.
- **plate** — the contrast layer under text over busy footage: ink at
  60–80% alpha, or a blurred scrim (`backdrop-filter` in the motion
  comps), radius 12–22 px, padding 16–28 px.
- **background** — the timeline's own background or the comp's backdrop;
  text never floats over unmanaged footage without a plate.

Contrast is checked against the FRAME at the busiest moment — 4.5:1
minimum for body-size text. `preview_frame` is the check.

## Captions

Two honest kinds:

1. **Segment-timed** (`captions_add`): the transcription's own segments.
   2–4 words a cue where the segment permits, bold 600+, stroke or plate
   contrast, `offsetY` ≈ 0.35, never touching edges. Never call them
   word-synced.
2. **Word-timed kinetic** (`captions_animate`): measured [text, start,
   end] words — each line a kinetic comp, active word highlighted,
   baked to one video, imported and placed at the voiceover's start.
   This is the short-form standard. Whisper SEGMENTS with per-word
   estimates are not timing; only measured word times are.

## Entrances and exits (every element)

Nothing just appears. Nothing just vanishes.

- Titles: rise 12–24 px + fade 250–400 ms in (`cubic-bezier(0.16,1,0.3,1)`
  feel — `keyframe_set` with an out-quart), fade 200–300 ms out.
- Lower thirds: slide from left 240–480 ms with a settle, exit slide-back
  300 ms. Built as the `funcut.lower-third.glass` comp with REAL copy and
  the fake timecode deleted — never the timeline's 2016 lime-plate
  preset.
- Captions: per-line fade/pop 120–180 ms (the kinetic comp does this
  itself).
- Stat cards/charts: count or grow with their `data-*` sound.

## The modern lower third (what "2016" means and how to leave it)

2016 is: a solid lime bar, slide-in, weight 600, "Name — Title", drop
shadow, gone. 2025 is: a glass plate (`backdrop-filter: blur`) or a
scrim bar, 800-weight name over 400-weight role, one accent underline
3–6% of frame wide, a spring settle not a linear slide, an exit, and the
whole thing built as a motion comp so it moves as one object. Build it
with `motion_bake` on `funcut.lower-third.glass`, edit the slots, delete
the badge you do not need.

## Kinetic text rules

- One emphasis device per piece: active-word color, OR scale pop, OR
  chip plate — not all three on every word.
- The accent color is the highlight; the ink is everything else. A
  highlight that changes color per word is a rainbow, not a system.
- Line breaks at meaning: never break "5.2%" or a hyphenated pair.
- Uppercase for 1–3 word HOOKS only; sentence case for reading text.
