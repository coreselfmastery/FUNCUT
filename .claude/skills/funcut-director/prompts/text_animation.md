# Text animation: titles, captions, kinetic type

Type is a first-class visual in faceless video — often THE visual. The engine
gives you 31 text presets (`title_add`), word-synced captions
(`captions_add`), full style control (fontFamily, fontSize as a fraction of
frame height, fontWeight 100–900, color, strokeWidth/strokeColor, shadow,
background plate with radius, align, opacity), and keyframes for motion.

## The 31 presets (text-presets; `title_add` styles)

atomic, blackletter, block, bold-caption, candy, cartoon, clean, code, comic,
diner, editorial, fashion, friendly, hairline, handwritten, hebrew, impact,
japanese, lower-third, marker, mission, mystery, notes, pixel, poster,
retro, scribble, stencil, tech, western, zoo (+ default).

Preset-choosing per niche: impact/bold-caption/poster for short-form hooks ·
tech/code/pixel for tech & gaming · editorial/fashion/hairline for luxury ·
handwritten/marker/scribble for history & how-to · mystery/stencil for
horror · candy/cartoon/friendly for family content · lower-third for
documentary-style nameplates.

## Captions (word-synced)

`captions_add` syncs to the transcription. Defaults worth overriding:

- `fontSize`: 0.04 of frame height is the engine default — use **0.05–0.06
  for 9:16** (42 px+ rule at 1080-wide equivalences).
- `fontWeight`: 600 default → **700–800 short-form**; stroke or plate for
  contrast over busy footage; `offsetY` 0.30–0.35 (keep the lowest line's
  top above the platform UI zone — bottom ~15 % of a 9:16 frame is burned
  by TikTok/Shorts/Reels chrome).
- 2–4 words on screen at a time: group by the plan segments, never by
  sentence.
- Emphasis: ONE key word per segment in the niche's accent color (the plan's
  `emphasisColor`); two colored words in a row means neither is emphasized.

## Kinetic type patterns (keyframes on text clips)

Text arrives MOVING or it arrives dead. Recipes (properties via
`keyframe_set`; `at` = fraction of the clip, `ease` = cubic-bezier):

| Pattern | Keys | Feel |
| --- | --- | --- |
| Pop-in | scale 0.9→1.0 + opacity 0→1 over 150 ms, `ease [0.2,1.4,0.4,1]` (overshoot) | UI/punch, best friend of `Impact` SFX |
| Slide-in | offsetX ±0.08→0 + opacity, 200–250 ms, `ease-out` [0.16,1,0.3,1] | list steps, lower thirds |
| Rise | offsetY 0.04→0 + opacity, 250 ms | editorial, quiet emphasis |
| Tracking expand | scale 1.0→1.06 over the WHOLE clip, linear | luxury poster, title cards |
| Blur-free depth | opacity 0→1 in 80 ms then scale 1.0→1.03 over the card | (no blur keyframe exists — use fast opacity + slow scale) |
| Exit | mirror the entrance at `at` 0.9–1.0, 150 ms | only for cards that hand off |

Never animate BOTH scale and offset and rotation on one card — one axis of
motion + opacity is the professional dose.

## Title cards (title_add)

- Hook title (0–3 s): 3–7 words, impact-class preset, pop-in on the SFX,
  positioned upper-third (`offsetY` ≈ −0.25) to keep captions clean.
- Section plates: lower-third preset, slide-in, hold 2–3 s.
- Payoff/CTA card: poster-class, full center, punch zoom, matches the hook's
  family (same preset family, same accent color — bookends).

## Numbers and stats

Numbers get their own card ALWAYS when they matter: number huge (fontSize
0.10–0.14), unit/label small beneath, Impact or Riser SFX peaked on the pop.
"4,000,000,000 YEARS" as a textCard with a punch zoom beats narration alone
every time it appears in a script.

## Self-check

- [ ] One font family per video (presets from one style family).
- [ ] Captions: 2–4 words, 42 px+, stroke or plate, safe area respected.
- [ ] Every text card has ONE entrance motion + its SFX, ≤ 250 ms.
- [ ] One emphasized word per segment max; accent color consistent.
- [ ] Numbers that matter got their own card.
