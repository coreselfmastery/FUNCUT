---
name: motion
description: Design and render motion graphics inside FUNCUT (the Concat video editor) — titles, lower thirds, kinetic captions, stat cards, charts, quotes, chapters, terminals, before/after splits, logo stings, backdrops, and hand-authored comps with masks, effects, blends and spring curves. Use this whenever a video needs designed type or animated graphics, whenever you are asked to animate text, caption a transcript word by word, build a title package or a showreel, or retime/restyle an existing comp. Covers the design doctrine (hooks, rhythm, contrast, one idea per beat), the comp JSON document down to every node kind, the twelve shipped presets and how to break out of them, the full operation vocabulary (set-slot, set-words, set-track, add-text/add-rect/add-group, spring/bezier/steps/points curves), the `concat-motion-agent` CLI (list, inspect, edit, frame, bake, reel, captions), the Geneva engine's real limits, and the verification loop: inspect, edit in one validated batch, look at three frames, bake at cut size.
---

# Motion comps

A comp is a small scene graph — shapes, text, markup, nested comps — stored as JSON, baked to an
**RGBA PNG sequence**, and composited like any other picture in the project. It is the design layer:
what turns footage into an authored video.

The default engine is **Geneva**, a browserless HTML/CSS renderer (one binary, no Chrome): flexbox,
linear/radial gradients, `backdrop-filter`, `box-shadow`, `background-clip: text` and `@keyframes` all
draw. The `native` engine draws shapes/text/comps with no sidecar. A comp with `html` nodes needs
Geneva (`inspect` reports `needs_markup`).

The full reference is `docs/MOTION.md`; this file is the working manual **and the brief**.

---

## 0. You are the director, not the operator

The twelve presets are a **vocabulary**, not a menu. A reel that runs the presets in order is a
catalogue; a reel that *argues* something is design. This layer is one of the most expressive things in
the product — masks, blends, effects, five curve families, markup that can draw anything CSS can draw —
and it is wasted if every scene is "preset, default slot, three seconds".

**The freedom you have is larger than the verbs suggest.** You may:

- hand-author a comp document from scratch and ship it as `comps/mine.json` (this is the real surface:
  every node kind, mask, effect, blend, gradient, dash, curve); the presets were written that way;
- compose presets **and** your own comps in one sequence, and re-cut any preset on the way in;
- retime, move, delete, re-fill, re-blend, add to and subtract from any comp with operations;
- write markup (HTML + CSS) inside a comp — the whole CSS box model, gradients, backdrop filters,
  `clip-path`, pseudo-elements, keyframes;
- animate anything numeric: position, scale, rotation, opacity, blur, dash, and shader params.

**What you may not do** is ship the *first* idea. Before a single op is written, answer the brief in
§4. If two consecutive scenes have the same entrance, the same position and the same weight, one of
them is wrong.

**Never claim a comp is finished from JSON alone.** Three frames and one bake, every time (§10).

---

## 1. The loop — never skip a step

```
brief  →  list / inspect  →  one edit batch  →  frame at 2–3 moments  →  bake  →  place / reel
```

1. **Brief it first** (§4). Name the idea, the accent, the type pairing, the rhythm. One sentence.
2. **`inspect` before you touch anything.** It returns every node with its kind, span, track
   properties and first words, plus slots and `needs_markup`. Node ids are not guessable: the lower
   third's card is `card`, the caption's words node is `caption`. Every operation that names a node or
   slot that does not exist is **refused and names it**.
3. **One `edit` batch.** Applied all-or-nothing and validated; a half-built comp never reaches the
   timeline. The error carries the failing operation's index.
4. **Look at three frames**: mid-entrance, settled, mid-exit. Each frame is 32–47 ms. Three, not thirty.
5. **Bake** at the size you cut at (`--width/--height`), never 4K "for safety".
6. **Place** the clip as `kind: "motion"` with `comp`, `hash`, `engine` — or put it in a storyboard and
   let `reel` do steps 5–6 for every scene at once.

---

## 2. The CLI

`concat-motion-agent` — one binary, JSON in, JSON out, non-zero exit on failure. Every verb prints
exactly one JSON document.

```sh
concat-motion-agent list
concat-motion-agent inspect comps/funcut.lower-third.glass.json
concat-motion-agent edit comps/lt.json ops.json -o comps/lt2.json
concat-motion-agent frame comps/lt2.json 1.5 -o still.png --width 960
concat-motion-agent bake comps/lt2.json cache/lt --width 1280 --height 720
concat-motion-agent reel storyboard.json -o reel.mp4            # --plan, --frame 12.4, --cache DIR
concat-motion-agent captions words.json -o storyboard.json --plate assets/plate.jpg --audio assets/score.wav
concat-motion-agent help
```

| Verb | Returns |
| --- | --- |
| `list` | `{ok, count, presets[{id,name,category,engine}]}` |
| `inspect <comp>` | `{ok, comp: CompSummary, hash, problems[]}` — nodes, spans, tracks, words, slots, `needs_markup` |
| `edit <comp> <ops.json\|-> [-o out]` | `{ok, written, hash, comp}` — the new summary |
| `frame <comp> <sec> [-o png] [--width N]` | `{ok, at, frame}` |
| `bake <comp> <dir> [--width --height --fps --opaque]` | `{ok, frames, width, height, fps, alpha, engine, engine_version, hash, dir, seconds, frames_per_second}` |
| `captions <words> [-o storyboard] [--words-per-line N] [--max-line S] [--preset ID] [--plate IMG] [--audio FILE]` | `{ok, words, lines, duration, written, text[], storyboard}` |
| `reel <storyboard> [-o mp4] [--cache DIR] [--frame SECONDS] [--plan]` | `{ok, scenes, duration, layers, assets, warnings, written, seconds, frames_per_second, report}` |

`ops.json` is `{"ops":[…]}` or a bare array; `-` reads stdin. In-repo:
`cargo run -p concat-motion --bin concat-motion-agent -- <verb>`.

`frame --width` renders a **window of the authored frame**, not a scaled copy: a 1920 comp checked at
`--width 960` shows the left half. Check at the authored width and downscale the PNG.

---

## 3. Operations

Kebab-case `op` tag, one JSON object each.

| Op | Fields | For |
| --- | --- | --- |
| `set-slot` | `slot` (label or node id), `value` | filling a template — name, headline, number |
| `set-text` | `node`, `text` | plain text nodes only (refused on `html` nodes) |
| `set-words` | `node`, `words: [[word, start, end?], …]` | **karaoke captions from a transcript** |
| `move` | `node`, `start` | when it happens |
| `retime` | `node`, `duration` (`0` = to the end) | how long it lasts |
| `set-track` | `node`, `property`, `keys: [[at, value, curve?], …]` | anything precise; `at` is a **fraction of the node's span** |
| `fade-in` | `node`, `seconds`, `curve` | the safe entrance (curve is required) |
| `slide-to` | `node`, `x`, `y`, `seconds` | arrival from an edge, settled on a spring |
| `pop` | `node`, `from`, `seconds` | chips, badges, numbers arriving |
| `add-group` | `id`, `x`, `y` | a parent to hang children on |
| `add-rect` | `id`, `width`, `height`, `fill`, `radius`, `x`, `y` | a plate, a bar, a divider |
| `add-text` | `id`, `text`, `size`, `weight`, `color`, `x`, `y` | a kicker, a label |
| `delete` | `node` (and its children) | removing what the edit does not need |
| `set-blend` | `node`, `blend` | `screen` for glow, `multiply` for shadow |
| `set-opacity` | `node`, `opacity` | base opacity, under any track |
| `set-duration` | `seconds` | the comp's own length |
| `set-background` | `color` (`#rrggbbaa`) | `#00000000` = transparent (the default for overlays) |

Animatable through `set-track`: `position`, `scale`, `rotation`, `opacity`, `blur`, `dash`,
`param:<key>`. Values are **numbers**; a `position` track takes `[x, y]`-shaped points and a `dash`
track a number. The ops are the *convenience* layer: anything they cannot express, the comp document
can (§4b).

### Filling a slot

`set-slot` finds the slot by its **label** or its node id and replaces *the words it stands in for* —
a whole text run, or a span across inline tags (so a headline written `One <em>tap</em> away.` fills
correctly and the emphasis inside the span goes with it). A sample that is nowhere in the markup is
**refused with both the sample and the node**, because filling some other word would be a silent, wrong
edit. A value is text, never markup: `<b>` in a value is escaped, so a slot cannot inject HTML.
Filling a slot with the words it already has is fine — it is a fill, not a no-op error.

### Curves

| Curve | JSON | Use |
| --- | --- | --- |
| linear | `{"kind":"linear"}` | wipes, constant rotation, and nothing else |
| bezier | `{"kind":"bezier","x1":0.16,"y1":1,"x2":0.3,"y2":1}` | the expensive settle — **the default choice** |
| steps | `{"kind":"steps","count":6,"position":"jump-end"}` | typewriters, counters, terminal lines |
| points | `{"kind":"points","stops":[[0,0],[0.7,1.1],[1,1]]}` | a hand-drawn ramp, an overshoot you tuned |
| spring | `{"kind":"spring","stiffness":260,"damping":15,"mass":1}` | one physical overshoot: pops, chips, marks |

A spring is solved analytically and rescaled to land **exactly** on the next key, so its parameters
shape the overshoot, not the duration — retiming never retunes the motion.

**Curves are the personality of the piece.** A film where everything eases on the same bezier feels
machine-made. Pick a temperament per piece: *physical* (springs, 240–320 stiffness, damping 13–16),
*editorial* (long bezier on position, fast bezier on opacity), *mechanical* (steps 6–12 for anything
numeric), *hand-drawn* (points ramps with a small overshoot cadence). Two families per film; three is
a mood, one is a template.

### Words

`[word, start]` with two elements means **hold until the next word**; a `null` third element on the
last word means **hold to the end of the comp**. So:

```json
{"op":"set-words","node":"caption","words":[["Make",0,0.45],["it",0.45,0.85],["move.",0.85,null]]}
```

---

## 4. The brief — five decisions before the first op

Write these down (in the reply, in a file, in your head — but decide):

1. **The idea.** One sentence the finished piece proves. "The system is fast enough to feel
   inevitable." "One word, twelve dialects." If the sentence is "here are the twelve presets", start
   again.
2. **The accent.** One colour, plus its tints. It appears in *every* scene and covers *no more than
   about 10%* of the frame. Everything else is ink, paper, and the footage's own colour.
3. **The type pairing.** One display face for statements (weight 700–900, tight tracking) and one
   workhorse for labels (weight 300–500, open tracking). Two faces, never three. Big type gets
   `-0.02em`; small caps get `+0.18em` to `+0.3em`.
4. **The rhythm map.** Write the scene order as durations before you write any JSON, and check the
   shape: `3.8 2.6 | 2.6 3.6 3.8 3.4 | 2.4 3.6 3.6 2.8 | 2.4 4.6 4.0 3.4 | 2.2 3.2 2.6`. Long, short,
   long — never four scenes of the same length. Movement is punctuated by stillness: one scene in
   five should be nearly static.
5. **The exit.** Every element that arrives has an exit or a cut that knows about it. A comp that pops
   out instantly looks broken.

### 4b. When the presets are not enough — author the document

A comp document is JSON with an `id`, frame size, `duration`, `background`, `assets`, `curves`,
`nodes` and `slots`. Every node has `id`, `start`, `duration`, `transform`, `opacity`, `blend`,
`tracks`, optional `mask` and `effects`, and one `kind`:

| `kind` | Fields | What it gives you |
| --- | --- | --- |
| `group` | `children` | a transform to hang nodes on |
| `rect` | `width`,`height`,`fill`,`radius` | plates, rules, bars |
| `ellipse` | `width`,`height`,`fill` | discs, glows (with a feathered mask) |
| `path` | `commands`,`fill`,`stroke`,`dash` | vector marks — **native engine only** |
| `text` | the text node | words, gradient fills, outlines, shadows, per-word timing |
| `html` | `markup`,`css`,`width`,`height` | anything CSS can draw; the design-grade surface |
| `image` / `lottie` | `asset` | stills and vector animation |
| `composition` | `comp` | a comp placed inside a comp |

A node's `mask` is `{"shape":"rect|ellipse","radius":…,"feather":…,"invert":…}` and `effects` are
`[{"kind":"blur","radius":…},{"kind":"shadow","x":…,"y":…,"blur":…,"color":…}]`. Blends are `normal`,
`multiply`, `screen`, `overlay`, `darken`, `lighten`, `difference`, `soft-light`, `add`.

The correct split of labour, measured on Geneva 1.6.0:

- **Markup is where gradients live.** A shape's `fill` is *one colour*; a gradient on it is a hard
  `E103` in the engine, so `concat-motion` averages it to a solid and warns. Gradients, radial light,
  gradient-clipped type, `backdrop-filter` glass, keyframes — all markup.
- **Shapes are where masks and soft light live.** A feathered elliptical mask over a solid ellipse is
  a glow; a feathered rect mask over a solid rect is a light leak.
- **`path` is native-only.** Geneva reports it and drops it. Do not build a look on it unless the comp
  is going to the native renderer.
- **Keyframes on the outermost element of a clip are refused (`E442`)**: the clip can only animate
  transform and opacity. Put the animated rule one element deeper.
- `conic-gradient` does not draw; a `radial-gradient` with an explicit size ("62% 74% at …") drew
  nothing, `circle at 50% 60%` works. A `box-shadow` on the outermost element of a markup clip draws
  the *clip's rectangle*, not the rounded shape — put the shadow on an inner element. The `shadow`
  **effect** on a shape is native-only: Geneva warns and skips it, so a shadow you can see is always
  a CSS shadow inside markup.

Hand-authored comps are the fastest way to a look nothing else in the product can copy. `cypher.opener`
(a masked glow, a gradient-clipped word pulling focus on a `blur` track, a CSS hairline, a `steps()`
kicker) is ~120 lines of JSON and is in the repository as a template for exactly this.

---

## 5. The library — twelve comps

| Id | Category | Slot | What it is |
| --- | --- | --- | --- |
| `funcut.lower-third.glass` | Lower thirds | Name | avatar, name, role, LIVE badge, growing bar |
| `funcut.caption.kinetic` | Captions | Transcript line | word-timed caption with a highlight |
| `funcut.stats.triptych` | Stats | Numbers | three stat cards, bars growing |
| `funcut.chart.bars` | Charts | Chart title | bar chart building on |
| `funcut.ticker.count` | Stats | Number | one big counting number + drawn rule |
| `funcut.sting.wordmark` | Stings | Wordmark | gradient wordmark, drawn underline |
| `funcut.quote.pull` | Quotes | Quote | pull quote with attribution |
| `funcut.chapter.chip` | Chapters | Chapter | number, label, underline |
| `funcut.cta.subscribe` | Calls to action | Headline | pulsing subscribe pill |
| `funcut.code.terminal` | Code | Window title | terminal window, lines on `steps()` |
| `funcut.compare.split` | Before / after | After label | split frame, wipe, knob |
| `funcut.backdrop.mesh` | Backdrops | — | mesh gradient + grid + vignette (opaque) |

**How to use a preset without looking like you did:**

- change what it says (real copy, written for the scene, never lorem);
- change how it arrives — `pop` 0.94 → 1, `slide-to` from 66% of the frame, `fade-in` on a points
  ramp instead of the shipped bezier;
- cut it down (`delete` a badge, a divider, an avatar) so it carries one thing;
- retime it (`retime`, `move`) so it lands on the beat, not on the comp's own length;
- pair it — a preset over a hand-built glow reads as a system, not a template.

Measured bake speed (2 CPU cores, no GPU): chapter chip 39 fps, caption 29, lower third 28, subscribe
21, counter 21, wordmark 21, chart 16, terminal 15, quote 13, split 12, stat cards 11, mesh backdrop 4.
**Backdrops are the slow case — bake once, reuse.**

---

## 6. Sequences: many comps, one render

A reel, an explainer or a title package is not one comp — it is several, in order, over plates, with a
dip between them and music under. That is `sequence`:

```rust
let mut reel = sequence::Sequence::new(1920, 1080, 30.0);
reel.asset("plate", Asset { src: "assets/plate.jpg".into(), kind: None });
reel.push(sequence::Scene::new(presets::build("funcut.chapter.chip").unwrap()).over("plate", 0.05).holding(3.0));
reel.bed(sequence::AudioBed::new("score").at(-2.0));
let plan = sequence::plan(&geneva, &reel)?;
plan.render(&geneva, cache, "reel.mp4")?;
```

- a scene with no `at` follows the one before it, so a reel is written in order;
- `holding(n)` **retimes** the comp rather than cutting it off, so its motion stays inside its span;
- layer and asset ids are per scene, so the same template can appear twice;
- the curtain dips at every cut and is opaque at both ends;
- `plan.frame(...)` checks one moment for pence before `plan.render(...)` pays for all of them.

**From the agent door**, the same thing is one verb and one document:

```sh
concat-motion-agent reel storyboard.json -o reel.mp4        # or --frame 12.4, or --plan
```

```json
{ "width": 1920, "height": 1080, "fps": 30,
  "assets": { "plate": "assets/plate.jpg", "score": "assets/score.wav", "Inter": "assets/Inter.ttf" },
  "scenes": [
    { "preset": "funcut.chapter.chip", "hold": 3.0, "plate": "plate", "zoom": 0.04,
      "ops": [ { "op": "set-slot", "slot": "Chapter", "value": "01 / INTRO" } ] },
    { "comp": "comps/mine.json", "at": 3.0, "plate": "plate", "zoom": 0.05, "ops": [] }
  ],
  "audio": [ { "asset": "score", "gain_db": -2.0, "fade_in": 0.4, "fade_out": 1.2 } ],
  "curtain": { "color": "#05060aff", "open": 0.6, "close": 0.8, "dip": 0.2, "depth": 1.0 } }
```

Ship every font the comps name as an asset (`"Inter": "assets/Inter.ttf"`), or the render says
`W405` and another machine draws different type. A plate with `zoom` 0.03–0.06 drifts enough to read as
footage and not as a still; past 0.1 it reads as a zoom, which is a different sentence.

---

## 7. Composition rules

- 5% title-safe inset on every edge; stack left edges on one x.
- One accent colour per video; one appearance at a time.
- Weight contrast: 900 headline beside a 400 label, never two 600s.
- Typographic scale: pick three sizes and stay on them (e.g. 210 / 84 / 26). A fourth size is a
  mistake; a size that is 1.6× another reads as an accident.
- Tighten big type (`-0.02em`), open small caps (`0.2em`).
- A card over footage should be glass (`backdrop-filter: blur()`), not a sticker.
- Timing: 0.35–0.6 s small element, 0.7–1.1 s card, never >1.2 s for something that repeats.
- **Exits matter.** Fade 0.3–0.5 s; a comp that pops out instantly looks broken.
- Never cover faces, subtitles or hands. Busy frame → move the comp.
- An overlay comp must not paint a background; `#00000000` is the contract.
- Contrast is structural, not decorative: put the type where the plate is darkest, or darken it
  locally with a gradient scrim in markup.
- Bake at cut size; nothing is "4K for safety".

---

## 8. Caption workflow (the highest-value move)

**Two commands do it:**

```sh
concat-motion-agent captions transcript.json -o storyboard.json \
    --plate assets/plate.jpg --audio assets/score.wav
concat-motion-agent reel storyboard.json -o captions.mp4
```

`captions` breaks the words into lines by sentence end, silence, word count (5) and length (3.6 s),
gives each line its own comp with **relative** word times, and writes the storyboard `reel` renders.
Look at `text[]` in its answer before rendering — if a line reads badly, change `--words-per-line`.

By hand, the same thing:

```
1. transcribe → words with start/end
2. inspect funcut.caption.kinetic → node "caption", slot "Transcript line"
3. edit one comp clip per 4–7 words, or per sentence
   {"op":"set-words","node":"caption","words":[[w,s,e], …]}
4. frame at the middle of the longest word — the highlight must be on it
5. bake; place bottom third, 10% margin, clear of faces
```

2.5–3.5 words/second reads; past 4 it does not. If the transcript is cue-level only, split each cue
evenly by character count — never invent timings that drift from the audio.

---

## 9. When something is refused

- The message names the node, the slot or the property. The fix is usually `inspect` again — the id you
  remember is not the id in the document.
- `needs_markup: true` + no Geneva sidecar → the comp bakes on `native` **without the markup nodes**;
  read the bake manifest's warnings instead of trusting the picture.
- A comp that renders empty is a background with `a: 0` (correct for an overlay), or a node whose
  `start` is later than the frame you looked at.
- Geneva refusals seen in the field: a gradient in a shape's `fill` (`E103`, now averaged to a solid
  with a warning), a keyframe on the clip's outermost element (`E442`), a `path` node (warned,
  dropped), a `shadow` effect on a shape (warned, skipped), `conic-gradient` (draws nothing), a keyed
  time with more than 9 decimals (`E103`, so times are written to milliseconds), an `inset`
  `box-shadow` (warned, skipped).
- Alpha is PNG-only: ProRes 4444 and VP9-alpha are the export-side targets, not yet wired.

---

## 10. Before you ship — the ten questions

1. Does the piece prove the one-sentence idea from §4, or does it demonstrate features?
2. Is there a hook in the first 1.5 s — a word, a shape, or a movement that promises something?
3. Are two consecutive scenes distinguishable at a glance (position, weight, colour, motion)?
4. Is the accent in every scene, and under ~10% of the frame?
5. How many type sizes? (Three or fewer.) How many weights? (Three or fewer.)
6. Where is the stillness? (At least one near-static beat.)
7. Does everything that arrives also leave — or is leaving the point?
8. Has every preset been *edited* — copy, timing, a deletion — rather than used as shipped?
9. Did you look at three frames per scene, and did you bake at cut size?
10. Would you cut this if it were someone else's footage? If not, cut it.

**A comp is done when you have looked at three frames, baked it at cut size, and can say why each
thing in the frame is where it is.**
