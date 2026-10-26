---
name: motion
description: Author, inspect and bake motion graphics comps inside FUNCUT (the Concat video editor) — lower thirds, kinetic captions, stat cards, charts, logo stings, subscribe cards, backdrops, before/after splits. Use this whenever a video needs designed type or animated graphics, whenever you are asked to animate text, make captions word-by-word, build a title or stat reveal, or retime/restyle an existing comp. Covers the comp JSON document, the twelve shipped presets, the full operation vocabulary (set-slot, set-words, set-track, spring/bezier/steps curves), the `concat-motion-agent` CLI (list, inspect, edit, frame, bake, reel, captions), and the verification loop: inspect, edit in one validated batch, look at three frames, bake at cut size. Also covers turning a transcript into word-timed captions and rendering several comps as one reel.
---

# Motion comps

A comp is a small scene graph — shapes, text, markup, nested comps — stored as JSON, baked to an
**RGBA PNG sequence**, and composited like any other picture in the project. It is the design layer:
what turns a cut into an authored video.

The default engine is **Geneva**, a browserless HTML/CSS renderer (one binary, no Chrome), so
flexbox, gradients, `backdrop-filter`, `box-shadow`, `background-clip: text` and `@keyframes` all draw.
The `native` engine draws shapes/text/comps with no sidecar. A comp that carries `html` nodes needs
Geneva (`inspect` reports `needs_markup`).

The full reference is `docs/MOTION.md` in the repository; this file is the working manual.

---

## 1. The loop — never skip a step

```
list / inspect  →  one edit batch  →  frame at 2–3 moments  →  bake  →  place on the timeline
```

1. **`inspect` before you touch anything.** It returns every node with its kind, span, track
   properties and first words, plus the slots and `needs_markup`. Node ids are not guessable: the
   lower third's card is `card`, the caption's words node is `caption`. Every operation that names a
   node or slot that does not exist is **refused and names it**.
2. **One `edit` batch.** It is applied all-or-nothing and validated; a half-built comp never reaches
   the timeline. The error carries the failing operation's index.
3. **Look at three frames**: mid-entrance, settled, mid-exit. Each frame is 32–47 ms. Three, not thirty.
4. **Bake** at the size you cut at (`--width/--height`), not 4K.
5. **Place** the clip as `kind: "motion"` with `comp`, `hash`, `engine`.

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
concat-motion-agent reel storyboard.json -o reel.mp4
concat-motion-agent captions words.json -o storyboard.json --plate assets/plate.jpg --audio assets/score.wav
concat-motion-agent help
```

| Verb | Returns |
| --- | --- |
| `list` | `{ok, count, presets[{id,name,category,engine}]}` |
| `inspect <comp>` | `{ok, comp: CompSummary, hash, problems[]}` — nodes, spans, tracks, words, slots, `needs_markup` |
| `edit <comp> <ops.json\|- > [-o out]` | `{ok, written, hash, comp}` — the new summary |
| `frame <comp> <sec> [-o png] [--width N]` | `{ok, at, frame}` |
| `bake <comp> <dir> [--width --height --fps --opaque]` | `{ok, frames, width, height, fps, alpha, engine, engine_version, hash, dir, seconds, frames_per_second}` |
| `captions <words> [-o storyboard] [--words-per-line N] [--max-line S] [--preset ID] [--plate IMG] [--audio FILE]` | `{ok, words, lines, duration, written, text[], storyboard}` |
| `reel <storyboard> [-o mp4] [--cache DIR] [--frame SECONDS] [--plan]` | `{ok, scenes, duration, layers, assets, warnings, written, seconds, frames_per_second, report}` |

`ops.json` is `{"ops":[…]}` or a bare array; `-` reads stdin. In-repo:
`cargo run -p concat-motion --bin concat-motion-agent -- <verb>`.

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

Animatable properties: `position`, `scale`, `rotation`, `opacity`, `blur`, `dash`, `param:<key>`.
`set-track` values are numbers; a `dash` or `position` track needs the same numeric shape the node uses.

### Filling a slot

`set-slot` finds the slot by its **label** or its node id and replaces *the words it stands in for* -
a whole text run, or a span across inline tags (so a headline written `One <em>tap</em> away.` fills
correctly and the emphasis inside the span goes with it). A sample that is nowhere in the markup is
**refused with both the sample and the node**, because filling some other word would be a silent, wrong
edit. A value is text, never markup: `<b>` in a value is escaped, so a slot cannot inject HTML.

Filling a slot with the words it already has is fine - it is a fill, not a no-op error.

### Curves

| Curve | JSON | Use |
| --- | --- | --- |
| linear | `{"kind":"linear"}` | wipes and nothing else |
| bezier | `{"kind":"bezier","x1":0.16,"y1":1,"x2":0.3,"y2":1}` | the expensive settle — **the default choice** |
| steps | `{"kind":"steps","count":6,"position":"jump-end"}` | typewriters, counters, terminal lines |
| points | `{"kind":"points","stops":[[0,0],[0.7,1.1],[1,1]]}` | a hand-drawn ramp |
| spring | `{"kind":"spring","stiffness":260,"damping":15,"mass":1}` | one overshoot: pops, chips, logo marks |

A spring is solved analytically and rescaled to land **exactly** on the next key, so its parameters
shape the overshoot, not the duration — retiming never retunes the motion.

### Words

`[word, start]` with two elements means **hold until the next word**; a `null` third element on the
last word means **hold to the end of the comp**. So:

```json
{"op":"set-words","node":"caption","words":[["Make",0,0.45],["it",0.45,0.85],["move.",0.85,null]]}
```

---

## 4. The library — twelve comps

| Id | Category | Slot | What it is |
| --- | --- | --- | --- |
| `funcut.lower-third.glass` | Lower thirds | Name | avatar, name, role, LIVE badge, growing bar |
| `funcut.caption.kinetic` | Captions | Transcript line | word-timed caption with a highlight |
| `funcut.stats.triptych` | Stats | — | three stat cards, bars growing |
| `funcut.chart.bars` | Charts | Chart title | bar chart building on |
| `funcut.ticker.count` | Stats | Number | one big counting number + drawn rule |
| `funcut.sting.wordmark` | Stings | — | gradient wordmark, drawn underline |
| `funcut.quote.pull` | Quotes | — | pull quote with attribution |
| `funcut.chapter.chip` | Chapters | — | number, label, underline |
| `funcut.cta.subscribe` | Calls to action | Headline | pulsing subscribe pill |
| `funcut.code.terminal` | Code | Window title | terminal window, lines on `steps()` |
| `funcut.compare.split` | Before / after | After label | split frame, wipe, knob |
| `funcut.backdrop.mesh` | Backdrops | — | mesh gradient + grid + vignette (opaque) |

**Order of preference:** a preset that is already close → a preset plus operations → primitives
(`add-group`/`add-rect`/`add-text`) only when nothing is close. Hand-built shapes look hand-built.

Measured bake speed (2 CPU cores, no GPU): chapter chip 39 fps, caption 29, lower third 28, subscribe
21, counter 21, wordmark 21, chart 16, terminal 15, quote 13, split 12, stat cards 11, mesh backdrop 4.
**Backdrops are the slow case — bake once, reuse.**

---

## 5. Sequences: many comps, one render

A reel, an explainer or a title package is not one comp - it is several, in order, over plates, with a
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
    { "comp": "comps/mine.json", "at": 3.0 }
  ],
  "audio": [ { "asset": "score", "gain_db": -2.0, "fade_in": 0.4, "fade_out": 1.2 } ],
  "curtain": { "color": "#05060aff", "open": 0.6, "close": 0.8, "dip": 0.2, "depth": 1.0 } }
```

Ship every font the comps name as an asset (`"Inter": "assets/Inter.ttf"`), or the render says
`W405` and another machine draws different type.

The measured example is `examples/reel.rs`: thirteen scenes, twelve presets, 45.5 s, **1365 frames of
1080p30 in 111 s on two cores**. Copy its shape rather than starting from an empty timeline.

## 6. Composition rules

- 5% title-safe inset on every edge; stack left edges on one x.
- One accent colour per video; one appearance at a time.
- Weight contrast: 900 headline beside a 400 label, never two 600s.
- Tighten big type (`-0.02em`), open small caps (`0.2em`).
- A card over footage should be glass (`backdrop-filter: blur()`), not a sticker.
- Timing: 0.35–0.6 s small element, 0.7–1.1 s card, never >1.2 s for something that repeats.
- **Exits matter.** Fade 0.3–0.5 s; a comp that pops out instantly looks broken.
- Never cover faces, subtitles or hands. Busy frame → move the comp.
- An overlay comp must not paint a background; `#00000000` is the contract.
- Bake at cut size; nothing is "4K for safety".

---

## 7. Caption workflow (the highest-value move)

**Two commands do it now:**

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

## 8. When something is refused

- The message names the node, the slot or the property. The fix is usually `inspect` again — the id
  you remember is not the id in the document.
- `needs_markup: true` + no Geneva sidecar → the comp bakes on `native`
  **without the markup nodes**; read the bake manifest's warnings instead of trusting the picture.
- A comp that renders empty is a background with `a: 0` (correct for an overlay), or a node whose
  `start` is later than the frame you looked at.
- Alpha is PNG-only: ProRes 4444 and VP9-alpha are the export-side targets, not yet wired.
- Nothing is ever drawn as something else: an unsupported feature is reported by node id.

**Before claiming a comp is done, you have looked at three frames and baked it at cut size.**
