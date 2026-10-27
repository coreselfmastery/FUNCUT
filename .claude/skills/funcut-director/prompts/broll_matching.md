# B-roll matching

Stock footage is the floor of the edit, not the ceiling. This is how the
floor stays interesting: every segment a new subject, every clip fitted
to the frame, nothing reused until it has been forgotten.

## Queries: one new noun per segment

Write the whole query list in the plan BEFORE searching. Then apply the
two laws:

1. **No noun repeats within 30 s of plan time.** "stock market" at 0:12
   means "trading floor" at 0:18 is fine (different noun) but "stock
   market" again at 0:26 is not.
2. **Concrete beats abstract.** "money" is weak; "coins stacking",
   "counting bills", "card tapping at a terminal" are strong. A query a
   camera operator could film is a query that will find footage.

Score what comes back: does the caption actually describe the clip
(OpenVid captions are the dataset's own alt-text and can be wrong — when
a top result feels off, check it with a `preview_frame` after import),
does the aesthetic score clear the shelf's average, is the motion right
for the pacing (static drone shots in a hype piece)?

## Aspect and fit (this is not optional)

**OpenVid has no vertical footage.** Never search `orientation:
"vertical"` — the result is always empty. For a 9:16 canvas:

1. Search without orientation; prefer 1080p/720p landscape sources (the
   512×512 shelf upscales badly to 1080×1920 — a 3.75× blow-up looks like
   it sounds).
2. `clip_frame {framing: "fill"}` EVERY clip — cover the canvas, crop the
   overhang. This is the default, not the fallback.
3. `timeline_background {kind: "blur"}` is a deliberate style choice
   (soft letterbox), not a substitute for step 2.

A letterboxed or pillarboxed frame at review is a defect. So is a clip
playing at 512² in the middle of a 1080×1920 canvas.

## Shot-scale ladders

Within a section, climb: **wide → medium → close → detail**. A section
that is all wides is a screensaver; all closes is claustrophobic. Plan
the ladder per section (the query list implies it: "city skyline" →
"office floor" → "hands on keyboard" → "close-up of candlestick chart").

## The fallback ladder

A thin search is a fact, not a failure. In order:

1. Simplify: drop one word ("aerial city skyline dusk" → "city skyline").
2. Synonym the subject ("stock market crash" → "falling chart" / "trading
   floor panic").
3. Go one abstraction up ("VW Beetle" → "classic car").
4. Change the *kind* of visual: a `motion_comp` stat card or chart IS a
   legitimate visual for a claim segment — often the better one.
5. A text card (`title_add`, styled, animated) is the last resort — and
   must follow the text zones, not just sit somewhere.

Never invent a clip id. Never leave a segment without a visual.

## Reuse

- Reel: a clip id once, total.
- Longform: once per 60 s, never twice in one section; the same SOURCE at
  different trims/framings counts as different shots but still needs the
  60 s gap to read as intentional.

## The pacing tie-in

One clip per segment, ~1 clip per 2.5–4 s of runtime at energetic pacing
(the 4-second law is the cap). `openvid_search` with `minSeconds` /
`maxSeconds` near your segment lengths wastes less footage; trim to the
word, lead the visual 100–200 ms.
