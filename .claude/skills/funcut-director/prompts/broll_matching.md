# B-roll matching

Stock footage is the floor of the edit, not the ceiling. This is how the
floor stays interesting: every segment a new subject, every clip fitted
to the frame, nothing reused until it has been forgotten.

## Shopping by the slot (when the plan carries slots)

A keyword query is what BM25 gets; a slot's `description` is what CLIP
ranks against. When the plan carries `slots`, shop each `broll` slot
with `openvid_rank_for_slot`: the slot's description verbatim, its
fallback queries, `hero`, `motion_min`, and `exclude_ids` carrying every
clip id the edit has already chosen — the hard filters are promises, not
hopes: a used id or a clip under the floor is not ranked lower, it is
not there. With every slot filled, assemble the manifest (each clip's
measured `motion`, its `camera` lead as the direction, the slot's own
scale and category, the colour bias read off the contact sheet) and run
`footage_continuity`; apply each flag's remedy — re-shop that slot with
the `avoid` hint and the rejected id excluded, or take the downgrade and
let the comp or the A-roll carry the span — before anything is fetched.
The keyword laws below still govern the fallback `queries`, and
slot-less plans shop exactly as the next section says.

## Queries: one new noun per segment

Write the whole query list in the plan BEFORE searching. Then apply the
two laws:

1. **No noun repeats within 30 s of plan time.** "stock market" at 0:12
   means "trading floor" at 0:18 is fine (different noun) but "stock
   market" again at 0:26 is not.
2. **Concrete beats abstract.** "money" is weak; "coins stacking",
   "counting bills", "card tapping at a terminal" are strong. A query a
   camera operator could film is a query that will find footage.
3. **Two or three words, not a sentence.** `openvid_search` words are
   ANDed and prefix-matched, with the stop words dropped for you
   ("rain on a window" is `rain window"): "coin drop", "skyline dusk",
   "keyboard close-up" - not "coins dropping onto a wooden table in slow
   motion", where every remaining word must match and the search drifts
   into the OR fallback. Cut to the noun that changes plus at most one
   modifier; the search retries wider on its own when the AND finds
   nothing, and a short query ranks better either way.
   `openvid_search_batch` takes the whole list at once - use it.

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

## Claims are comps first

A number, a comparison, a process, a definition or an abstract claim is
NOT a stock search. There is no footage of "73%" or "twice as fast", and
the generic-query results for one are the same clips every other channel
in the niche already uses. Plan those segments with a comp
(`funcut.ticker.count`, `funcut.chart.bars`, `funcut.stats.triptych`,
`funcut.compare.split`, a terminal, a pull quote) and reach for stock
only when the script names a thing a camera could actually film. The
comp is not the fallback; it is the default for exactly the segments
where stock is weakest. The emotion extract's per-line `queries` are the
primary source for the filmable nouns — the niche playbooks' query lists
are starter examples to be replaced with the script's specifics, not
lists to copy.

## The fallback ladder

A thin search is a fact, not a failure. In order:

1. Simplify: drop one word ("aerial city skyline dusk" → "city skyline").
2. Synonym the subject ("stock market crash" → "falling chart" / "trading
   floor panic").
3. Go one abstraction up ("VW Beetle" → "classic car").
4. Change the *kind* of visual: if the segment turned out to be a claim
   after all, plan a comp for it (see "Claims are comps first" above) —
   that is a promotion, not a fallback.
5. A text card (`title_add`, styled, animated) is the last resort — and
   must follow the text zones, not just sit somewhere.

Never invent a clip id. Never leave a segment without a visual.

## Reuse

- Reel: a clip id once, total.
- Longform: once per 60 s, never twice in one section; the same SOURCE at
  different trims/framings counts as different shots but still needs the
  60 s gap to read as intentional.

## The pacing tie-in

One clip per segment is the default, and ~1 clip per 2.5–4 s of runtime
at energetic pacing (the 4-second law is the cap) is the *center of
gravity*, not a metronome: a grid of equal-length cuts reads as
assembled, which is the exact complaint "robotic" names. So break the
cadence in every section at least once — one strong shot held 6–8 s with
drift or parallax to breathe, or one montage burst of 0.4–0.8 s cuts —
and no three consecutive segments share a length within 0.2 s.
`openvid_search` with `minSeconds` /
`maxSeconds` near your segment lengths wastes less footage; trim to the
word, lead the visual 100–200 ms.
