# Phase 0 — Creative direction

The single biggest reason an AI edit looks like a "$10 video" is not the
encoder, the transitions or the font. It is that nobody decided what the
video is *about* for a specific person. A cut can obey every hard floor in
this skill and still be forgettable. Fix that first.

## The brief you must write before any cut

Call `creative_brief {path, action:"set", brief:{...}}`. It persists as
`creative-brief.json` in the project, so every later turn — and every other
process — reads the same intent instead of re-inventing it.

| Field | What good looks like | What bad looks like |
| --- | --- | --- |
| `audience` | "new freelancers who just lost their first client" | "everyone", "people interested in tech" |
| `platform` | "tiktok, 9:16, sound off by default" | omitted |
| `goal` | "make them re-check their own contract today" | "get views" |
| `thesis` | one arguable sentence | a topic noun: "AI" |
| `curiosityGap` | a question the first 1.5 s opens and the viewer *needs* answered | a vague hook word: "amazing" |
| `payoff` | the specific reveal, number, reversal or demonstration | "they learn something" |
| `emotionalArc` | 3+ different beats: `unease -> curiosity -> relief -> resolve` | one mood repeated |
| `referenceStyle` | name the grammar you are borrowing: "Kurzgesagt's scale shifts; Fincher's restrained grading" | "cinematic" |
| `tone` | dry, urgent, awed, conspiratorial... | "professional" |
| `avoid` | the clichés this niche always reaches for | empty |

`creative_brief` returns a `directorPrompt`. Paste it at the top of your
working context and use it to judge every later decision. If the user gives
only a topic, invent a sharp angle, state it plainly, and put it in the
brief — do not ask for permission to have a point of view.

## Turn the angle into structure

1. **The hook is the gap, not a greeting.** The first shot and first line
   must make the `curiosityGap` felt. No logo sting, no "hey guys", no
   slow establishing shot. Under 3 seconds.
2. **Each body beat changes the emotional state.** If two consecutive
   sections leave the viewer feeling the same thing, merge them or change
   one. The `emotionalArc` is a checklist, not decoration.
3. **The payoff earns the ending.** Write the last line first. If you
   cannot say exactly what the viewer gets, the video has no reason to
   exist and no amount of motion will fix it.
4. **Reference style means specific borrowings.** Name what you take:
   cut rhythm, colour temperature, type scale, camera language, sound
   design. "Cinematic" is not a direction.
5. **Shot meaning beats shot prettiness.** For each segment ask: what does
   this image *say* that the voiceover does not? If the answer is
   "nothing, it fills space", the segment is a slideshow frame. Find a
   different visual with `stock_contact_sheet`.

## Anti-patterns the creative gate catches

`plan_validate` now returns `creativeReady`, `creativeBlockers`,
`craftScore` and `creativeNotes` alongside `valid`. It blocks:

- a missing or thin `creativeBrief`;
- an `emotionalArc` with fewer than three distinct beats;
- a hook longer than ~3 s, or no cta/outro after 12 s;
- fewer than half as many distinct visuals as segments, or 3+ identical
  visuals in a row;
- no designed comp or text card in a piece over 15 s;
- long runs of hard cuts and segments with no entrance (escalated from
  warnings).

`creativeReady: false` is a rejection. Fix the blockers and validate again.
The gate cannot judge taste — that is what `craft_critique` and a human are
for — but it can stop the most reliable ways an agent produces generic
work.

## A worked micro-example

Topic: "why small YouTube channels burn out".

```
audience: "creators at 1-10k subs making their 40th video"
platform: "youtube shorts, 9:16"
goal: "make them stop measuring every upload against the last"
thesis: "burnout is a measurement error, not a motivation problem"
curiosityGap: "what if the number that makes you quit is the one that lies?"
payoff: "views per upload is a lagging, noisy signal; watch-time per hour
         of work is the fair one - here is the five-minute calculation"
emotionalArc: ["recognition", "irritation", "curiosity", "relief", "resolve"]
referenceStyle: "Veritasium's reveal structure; restrained type, no memes"
tone: "quietly candid"
avoid: "burning-out stock footage of a laptop at 2am, red arrows, shock faces"
```

Every later phase now has something to be faithful to.
