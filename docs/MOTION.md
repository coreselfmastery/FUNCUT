# Motion comps

Motion comps are the design layer of FUNCUT: the thing that makes a lower third, a kinetic caption,
a stat card or a logo sting *move*, without leaving the project. A comp is a small scene graph stored
as JSON in the project, baked to an RGBA frame sequence, and composited like any other picture — so it
goes through the project's own linear-light blending, effects, transitions and encoder.

The crate is `concat-motion` (`src/crates/concat-motion`). Nothing in it links FFmpeg, so it builds and
tests on its own.

---

## Three engines, one document

| Engine | Draws | Needs |
| --- | --- | --- |
| `geneva` — **the default** | everything, markup included: flexbox, gradients, `backdrop-filter`, `box-shadow`, `background-clip: text`, CSS `@keyframes` | the pinned Geneva sidecar (one 17 MB binary, **no browser**) |
| `native` | shapes, paths, text, images, nested comps | nothing (the shipped compositor) |
| `lottie` | Lottie / dotLottie files | reserved; the player lands next |

`EnginePreference::default()` is **Geneva**, and a motion clip records `engine: "geneva"` unless it says
otherwise. Why: a comp written by a designer, a template or an agent is markup, and markup is the only
way to get design-grade type, cards and gradients. A comp with no markup still bakes on Geneva, so one
engine means one look. A machine without the sidecar falls back to `native` for the comps it can draw,
rather than failing to open the project.

Finding the sidecar: `CONCAT_GENEVA_BIN` → the setting → next to the executable → `~/.local/bin` → `PATH`.
Version `1.6.0` is what this was written against; another **major** version is reported as a risk, never
silently used. Every bake records the version in its manifest.

---

## The document

```json
{
  "format": 1,
  "id": "funcut.lower-third.glass",
  "name": "Glass lower third",
  "width": 1920, "height": 1080, "fps": 30, "duration": 6.0,
  "background": {"r": 0, "g": 0, "b": 0, "a": 0},
  "assets": {},
  "nodes": [ /* paint order: earlier is under later */ ],
  "slots": [ { "node": "card", "field": "text", "sample": "Ada Lovelace", "label": "Name" } ]
}
```

A node is `{ id, parent?, start, duration, transform, opacity, blend, mask?, effects[], tracks{}, …kind }`.
`duration: 0` runs to the end of the comp. Kinds:

| Kind | What it is |
| --- | --- |
| `group` | a transform and a parent for children — animate one, move the group |
| `rect` / `ellipse` | a filled shape with a corner radius |
| `path` | `MoveTo/LineTo/CubicTo/Close`, fill, stroke, optional `(dash, gap)` for draw-on |
| `text` | a style and words, **plus `words: [(text, start, end)]` and a `highlight` style** — karaoke is data |
| `html` | markup + stylesheet: the design-grade card, chip row or kinetic title |
| `image` | a still, fitted `contain/cover/fill/none` |
| `lottie` | a Lottie / dotLottie asset |
| `composition` | another comp, placed: define a sting once, use it nine times |

**Tracks** are keyed at *fractions of the node's span*, so an entrance survives retiming, trimming and
nesting. Property names: `position`, `scale`, `rotation`, `opacity`, `blur`, `dash`, and `param:<key>`
for an effect parameter. Values are `number | point | size | color`; mismatched kinds step at halfway,
as CSS does.

**Curves**: `linear`, `bezier{x1,y1,x2,y2}` (CSS `cubic-bezier`), `steps{count,position}`,
`points` (CSS `linear()` ramps), and `spring{stiffness,damping,mass}` — solved analytically and
rescaled to land **exactly** on the next key, so the parameters shape overshoot, not duration, and
retiming a comp does not retune its motion. Presets ship `SPRING` (one overshoot: pops, chips) and
`EMPHASIS` (fast out, long settle).

A comp is **validated** before it is baked, and the diagnostics point into the document
(`/nodes/3/tracks/opacity`). `Comp::hash()` is the content key: the same comp gets the same bake, an
edit gets a new hash and misses the cache by itself.

---

## The library (`presets::ALL`, twelve comps)

| Id | Category | What it is |
| --- | --- | --- |
| `funcut.lower-third.glass` | Lower thirds | avatar, name, role, LIVE badge, growing bar — slot: Name |
| `funcut.caption.kinetic` | Captions | word-timed caption row with a highlight — slot: Transcript line |
| `funcut.stats.triptych` | Stats | three stat cards, bars growing |
| `funcut.chart.bars` | Charts | a bar chart that builds on |
| `funcut.ticker.count` | Stats | one big number, label, drawn rule |
| `funcut.sting.wordmark` | Stings | gradient wordmark + drawn rule |
| `funcut.quote.pull` | Quotes | pull quote with attribution |
| `funcut.chapter.chip` | Chapters | number, label, underline |
| `funcut.cta.subscribe` | Calls to action | pulsing subscribe button |
| `funcut.code.terminal` | Code | terminal window, lines landing on `steps()` |
| `funcut.compare.split` | Before / after | split frame with a wipe and a knob |
| `funcut.backdrop.mesh` | Backdrops | mesh gradient + grid light + vignette (opaque) |

Each is an ordinary `Comp` with slots, so it is a template rather than a picture. `presets::build(id)`
returns one by id, `presets::ALL` is the catalogue, and `dump_presets` writes all twelve out as JSON -
`examples/comps/` holds that output for six of them.

---

## Baking, previewing, cost

`CompEngine::bake` writes `frames/0001.png…` (RGBA, true alpha) plus `manifest.json`
(hash, engine, engine version, size, fps, alpha). `CompEngine::frame` draws one frame for the stage.

Measured in this sandbox — **2 CPU cores, no GPU, bundled OpenH264** (a desktop is several times faster):

| Preset | Frames | Frames/s |
| --- | --- | --- |
| chapter chip | 150 | 39 |
| kinetic caption | 120 | 29 |
| lower third | 180 | 28 |
| subscribe card | 135 | 21 |
| counting number | 120 | 21 |
| wordmark sting | 105 | 21 |
| chart bars | 138 | 16 |
| terminal reveal | 126 | 15 |
| pull quote | 180 | 13 |
| before/after split | 135 | 12 |
| stat cards | 150 | 11 |
| mesh backdrop | 180 | 4 |

A single frame is **32–47 ms**, which is what makes a scrub responsive. Backdrops are the expensive
case (full-frame radial gradients); bake them once, at the size you need, and reuse.

**Alpha is PNG only.** Geneva documents its transparent output for the PNG image sequence; ProRes 4444
and VP9 came back opaque in testing, so a comp bakes to PNG and the encoder turns it into whatever the
project wants (ProRes 4444 and VP9-with-alpha are the export-side work).

---

## The agent door: `agent`, one binary, JSON in and out

Every verb prints exactly one JSON document and exits non-zero on failure.

```sh
cargo run -p concat-motion --bin concat-motion-agent -- list
cargo run -p concat-motion --bin concat-motion-agent -- inspect comps/funcut.lower-third.glass.json
cargo run -p concat-motion --bin concat-motion-agent -- edit comps/lt.json ops.json -o comps/lt2.json
cargo run -p concat-motion --bin concat-motion-agent -- frame comps/lt2.json 1.5 -o still.png --width 960
cargo run -p concat-motion --bin concat-motion-agent -- bake comps/lt2.json cache/lt --width 1280 --height 720
```

`inspect` returns the comp summary a model needs: nodes with their kinds, spans, tracks and first
words; the slots; and `needs_markup`, which says whether a markup engine is required.

`edit` takes a batch of operations, applies them **all or nothing**, validates, and writes the new
document. An operation that fails names its index, so a retry is one step, not a re-plan.

```json
{ "ops": [
  { "op": "set-slot", "slot": "Name", "value": "Grace Hopper" },
  { "op": "set-words", "node": "caption",
    "words": [["Make", 0, 0.45], ["it", 0.45, 0.85], ["move.", 0.85, null]] },
  { "op": "move", "node": "card", "start": 3.5 },
  { "op": "fade-in", "node": "card", "seconds": 0.8,
    "curve": { "kind": "bezier", "x1": 0.16, "y1": 1, "x2": 0.3, "y2": 1 } },
  { "op": "slide-to", "node": "card", "x": 960, "y": 540, "seconds": 0.7 },
  { "op": "pop", "node": "card", "from": 0.9, "seconds": 0.5 },
  { "op": "set-track", "node": "card", "property": "scale",
    "keys": [[0, 0.9, { "kind": "spring", "stiffness": 260, "damping": 15, "mass": 1 }],
             [0.6, 1.0, null]] },
  { "op": "add-text", "id": "kicker", "text": "CHAPTER 02", "size": 28, "weight": 700,
    "color": "#22d3c5", "x": 240, "y": 140 }
] }
```

Operations: `set-slot`, `set-text`, `set-words`, `move`, `retime`, `set-track`, `fade-in`, `slide-to`,
`pop`, `add-group`, `add-rect`, `add-text`, `delete`, `set-blend`, `set-opacity`, `set-duration`,
`set-background`.

---

## In the project

`concat-project` carries a motion clip:

```json
{ "kind": "motion", "motion": { "comp": "funcut.lower-third.glass", "hash": "…", "engine": "geneva" } }
```

The comp file lives in the project's `comps/` folder, named by its hash, so two clips placing the same
comp share one bake. `MotionClip::bakes_at(w, h)` is the cache-resolution test.

**Still to come** (nothing here is faked): resolving a motion clip to its baked sequence in
`concat-export`, the `motion.*` MCP tools (an API method each), the Lottie engine, the native engine's
`CompEngine` impl, and the alpha/intermediate export targets.

---

## Rules of thumb for a good comp

1. **One focal moment per scene.** Everything else supports it.
2. **Ease with `EMPHASIS` or a spring, never linear** — linear is what makes motion look cheap.
3. **Stagger by 60–120 ms**, not all at once.
4. **Type is the design**: weight contrast, tight tracking, one accent colour.
5. **Alpha is the contract**: a comp meant to sit over footage must not paint a background.
6. **Bake at the size you cut at**, not at 4K "for safety" — the compositor samples it.
7. **Nothing is silent**: an unsupported feature is reported by node id, never drawn as something else.
