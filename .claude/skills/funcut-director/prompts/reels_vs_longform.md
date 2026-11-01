# Reels and longform are different products

A reel is retention engineering; longform is an argument with chapters.
The plan's `mode` field says which one you are making, and the numbers
below change with it.

## The two structures

**Reel (≤ 60 s, 9:16 default)**

| Section | Span | Job |
| --- | --- | --- |
| HOOK | 0–3 s | the claim, the stakes, or the receipt — one sentence, one visual punch |
| BODY | 3 s–(N−8) | 2–4 escalations, each a pattern interrupt every 2–4 s |
| CTA / LOOP | last 5–8 s | one action OR a loop: the last frame cuts back to the first |

No intro card. No branding. The hook IS the intro.

**Longform (> 60 s, any aspect)**

| Section | Span | Job |
| --- | --- | --- |
| HOOK | 0–3 s | the promise, cold-open style — what the viewer walks away with |
| INTRO / TITLE | 3–8 s | wordmark sting or chapter chip; who you are in one beat |
| BODY | chapters of 20–40 s | each chapter: open loop → payoff, opened by a chapter chip on a downbeat, closed by a transition |
| RE-HOOK / MID CTA | at the 50–60% retention dip | the promise again, or one soft ask |
| OUTRO / CTA | last 5–8 s | one action, one promise, end card |

## The 8–20 minute longform (the YouTube watch-time product)

The numbers above cover the short longform. A true YouTube video — 8 to
20 minutes — is a third product, with its own laws:

- **Duration:** 13–23 minutes is the sweet spot — long enough for
  mid-rolls and real session time, short enough that completion does not
  collapse. 10–13 min for one tight story; 15–20 min for a list or a
  multi-part investigation. Never pad to reach a number: a 9-minute
  argument beats a padded 15-minute one on every metric that matters.
- **Chapters of 60–120 s, not 20–40 s.** Each chapter is a mini-reel: its
  own open loop in the first two sentences, its own payoff, a chapter
  chip on a downbeat. Six to twelve chapters total.
- **Visual vocabulary refresh per chapter.** The no-noun-repeat law is
  scoped per chapter, not per video: a 15-minute piece needs ~40+
  distinct query nouns. Give each chapter cluster its own shot family
  (aerial for the setup, macro for the detail, archival-style for the
  history beat) so the refresh is visible, not merely technical.
- **One comp system per chapter cluster, alternating families:** chapter
  chips everywhere, but rotate the payload — ticker counts in the data
  chapters, split compares in the case studies, pull quotes in the story
  ones. One full-screen designed beat per 60–90 s minimum, on top of the
  per-15 s floor for the designed moments.
- **Re-hook cadence: every third chapter, plus the 50–60% dip.** A pattern
  interrupt wherever attention historically sags — a new music entry, a
  hard cut to a different shot family, a "but here is where it gets
  strange" line. The single mid-video re-hook of a 3-minute piece is not
  enough up here.
- **Texture reset at the midpoint:** vary the grade intensity within the
  one look (the same filter family, intensity stepped up for the climax
  chapters, down for the resolution) and let the BGM drop to near-silence
  once before the final chapter — the quiet is what makes the last build
  land.
- **Deliberate holds are structural here:** one 6–10 s held shot per
  chapter (a slow drift over a strong frame) reads as confidence, not
  slowness, when the surrounding pace is 12–18 cpm.

## What changes between them

| | Reel | Longform |
| --- | --- | --- |
| Pacing | 20–30 cpm, interrupt every 2–4 s | hook 20+ cpm → body 12–18 → re-hook spike → outro 10 |
| Captions | word-timed kinetic (`captions_animate`), 1–3 words a cue | segment-timed (`captions_add`) or phrase comps; captions are support, not the star |
| Grade | bolder — the look IS the brand | consistent, calmer |
| SFX density | near every cut | section changes + key moments |
| BGM | one bed, drops on the beat at escalations | structure: build across chapters, dip at the re-hook, resolve at the outro |
| Comps | hook card, 2–3 stat/chart beats, end card | chapter chips per chapter, 1+ comp per 15 s, outro card |
| Endings | loop (last frame = first frame) | outro card, one CTA |
| Transitions | whips/glitches on the beat | wipes/dissolves at chapters |

## CTA placement

- Reel: verbal CTA in the last 3 s over the loop point, or none at all —
  the loop IS the CTA.
- Longform: soft ask at the mid re-hook ("if this is helping, you know
  where the button is"), real ask on the outro card (one action, one
  promise: "Subscribe — next week: how the same trick hides in your
  grocery bill").
- The outro card is a `funcut.cta.subscribe` comp, real copy, ~5 s, held
  long enough to read twice.

## Aspect by product

Reels are 9:16 (1080×1920). Longform is 16:9 for YouTube, 9:16 for
vertical-first platforms. The plan's `aspect` sets the canvas via
`project_set_video` BEFORE assets are gathered; a canvas changed after
placement invalidates every fit and every comp bake.
