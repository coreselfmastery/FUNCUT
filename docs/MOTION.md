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
The Linux `.deb`, `.rpm` and AppImage ship the pinned v1.6.0 binary next to the executable, so a
package install needs **no separate geneva install**; `install-linux.sh` still fetches it for source
and tarball installs. `motion_presets` reports whether the engine was found on this machine.
Version `1.6.0` is what this was written against - the Linux installer fetches exactly that one, pinned
by checksum, into `~/.local/bin`. Another **major** version is reported as a risk by the probe and
**refused at bake with the reason**, never silently used. Every bake records the version it used.

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
edit gets a new hash and misses the cache by itself. And the bake honours its own
key: `Geneva::bake` checks the target's `manifest.json` first, and a bake whose
comp hash, engine, engine version, size, rate, alpha and frame count all match —
with the frames still on disk — answers from the cache without re-rendering
(`cached: true` in the `motion_bake` reply). Bakes are also clamped to **4096**
on each side: the tool always promised "never above 4K for safety", and a
4-6 GB laptop has no business rendering 8K PNG frames.

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
`examples/comps/` holds that output for all twelve, plus the two hand-authored `cypher` comps and the
`showreel-v3` storyboard.

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
| mesh backdrop | 180 | 3.9 |

A single frame is **32–47 ms**, which is what makes a scrub responsive. Backdrops are the expensive
case (full-frame radial gradients); bake them once, at the size you need, and reuse.

**Alpha is PNG only.** Geneva documents its transparent output for the PNG image sequence; ProRes 4444
and VP9 came back opaque in testing, so a comp bakes to PNG. Use `motion_overlay` to composite those
RGBA frames over the actual shot; see "Where alpha actually lands" below.

---

## Sequences: many comps, one render

A single comp bakes to its own little frame sequence. A **sequence** is what a reel, an explainer or a
title package actually is: several comps, each with its own entrance, butted together over plates with
a dip between them and a bed of music under the lot.

```rust
use concat_motion::authoring::Op;
use concat_motion::comp::{Asset, Blend};
use concat_motion::{geneva::Geneva, presets, sequence};

let mut reel = sequence::Sequence::new(1920, 1080, 30.0);
reel.source_dir = ".".into();
reel.asset("plate", Asset { src: "assets/plate.jpg".into(), kind: None });

reel.push(
    sequence::Scene::new(presets::build("funcut.lower-third.glass").expect("a preset"))
        .over("plate", 0.05)                       // a still underneath, drifting 5%
        .layer(                                      // and one more, moving on its own clock
            sequence::SceneLayer::new("plate")
                .panning(-0.03, 0.0)
                .fading(0.5, 0.8))
        .layer(                                      // over the comp: a screen-blended light leak
            sequence::SceneLayer::new("leak")
                .blending(Blend::Screen)
                .at_offset(-0.25, -0.2)
                .scaled(0.8)
                .above())
        .holding(3.6)                              // shorter than the comp: it runs shorter
        .with_ops(vec![Op::SetSlot { slot: "Name".into(), value: "Grace Hopper".into() }]),
);
reel.bed(sequence::AudioBed::new("score").at(-2.0));

let geneva = Geneva::discover(None).ok_or("no geneva binary")?;
let plan = sequence::plan(&geneva, &reel)?;
let cache = std::path::Path::new("cache/reel");
plan.frame(&geneva, cache, 12.4, std::path::Path::new("check.jpg"))?;    // one still
plan.render(&geneva, cache, std::path::Path::new("reel.mp4"))?;          // the whole thing
```

A scene's `layers` are the multi-layer scene: plates under the comp in the order they are added,
`above()` overlays over it, each a [`sequence::SceneLayer`] with its own drift (`zoom`, `pan`),
position (`offset`, `scale`, `fit`), mixing (`blend`, `opacity`, `fade_in`/`fade_out`) and timing
(`start`, `hold`). `over(asset, zoom)` is and remains one plain plate; a scene may stack as many
layers as it has pictures for.

What `plan` guarantees:

- **Every scene is validated and its operations applied first.** A shot that does not make sense stops
  the plan and names the scene and the operation, rather than reaching a render.
- **A scene's `hold` retimes the comp, it does not cut it off** — the comp's own length shrinks, so a
  track keyed as a fraction of a span is still inside its span.
- **Layer ids are prefixed per scene**, so two scenes using the same template never collide, and asset
  ids are copied per scene so one scene can hold a different plate of the same name. A scene's own
  layers are numbered on their side of the comp: `s0.plate`, `s0.plate2`, `s0.overlay`,
  `s0.overlay2`.
- **A cover-fitted layer that pans or sits off-centre is scaled up by its own travel**, so the
  frame's edge never shows behind it; a `scale` under one is a deliberate shape and keeps its edges.
- **One curtain** over everything, opaque at the ends and dipping at each cut, with times forced to
  strictly increase.
- **The output is one timeline**: a scene's own @keyframes, its plates and overlays, the curtain and
  the audio beds, rendered in a single pass because they are one document.

`Plan::write` copies every asset into the render directory, so the folder renders on a machine that has
nothing else; `Plan::render` writes it and calls the engine.

`examples/reel.rs` is a complete one: **thirteen scenes, twelve presets, 45.5 s**, with plates, a
curtain and a synthesised score.

```sh
cargo run -p concat-motion --example reel -- --plan > reel.json
cargo run -p concat-motion --example reel -- --frame 12.4 -o check.jpg
cargo run -p concat-motion --example reel -- --assets . --cache cache/reel -o reel.mp4
```

Measured here: **1365 frames of 1080p30 in 111 s (12.2 fps, 7.0 MB at crf 18)** on two cores.

`examples/layers.rs` is the multi-layer one: **two scenes, eight timeline layers** - a
background plate drifting, a midground sliding the other way, a wordmark over both, a
screen-blended light leak and a grain tile over the lot, and a foreground card that
arrives late and leaves before the cut. Its pictures are not in the repository:
`scripts/motion/layers-assets.py` writes them, deterministically, from nothing but the
standard library - two gridded plates (a dark one and a warm one, gridded so what shows
under an overlay is identifiable in a diff), a radial light leak, a grain tile from a
fixed LCG seed, and the three faces the comps name, copied from `assets/fonts/`. The
same script on two machines writes byte-identical assets, so the example's cache key
survives a re-run:

```sh
python3 scripts/motion/layers-assets.py ./check
cargo run -p concat-motion --example layers -- --frame 2.0 -o check/s1.png --assets ./check
cargo run -p concat-motion --example layers -- --frame 7.0 -o check/s2.png --assets ./check
cargo run -p concat-motion --example layers -- -o check/layers.mp4 --assets ./check --cache check/cache
```

**Look at the frames before believing the composite.** Measurement is not inspection:
`scripts/motion/inspect-frames.py` does both for the two claims a stack of layers has
to hold. The card claim, at `--frame 7.0` against `--frame 5.5` (the same scene just
before the card arrives): the card's centre region differs (measured: mean luma
difference 58.5, peak 102), the border ring does not (0.0 - the scene was untouched
around the card), and four samples inside the frame but outside the card still read
the scene's own tones, so the card is a layer **on** the scene, not a replacement of
it. The blend claim, at `--frame 2.0` against the same frame rendered with the leak
layer dropped from the staged timeline: the leak's core region differs (mean 144.8),
a far corner does not (0.0), the rim is lighter with the leak than without it (mean
luma 164 against 72) while the plate's grid contrast survives underneath it (46
against 49) - screened over the ground, lightening it, not covering it. A vision
model reading the two frames back says the same thing in words: the grid lines and
the wordmark stay visible through the glow, the card sits over a comp whose own text
is still readable around it, and the frame reads as one composed scene rather than a
collage. Re-rendering the reel is byte-identical, and the engine's own report ends at
zero errors and zero warnings (one note: which x264 built the H.264).

```sh
python3 scripts/motion/inspect-frames.py ./check   # renders one A/B frame itself
```

---

## Captions: a transcript is the input, not the design

`captions.rs` is the shortest path from a recogniser's word list to finished kinetic captions, and it is
the one thing an external motion engine cannot do for FUNCUT — FUNCUT holds the transcript.

```rust
use concat_motion::captions::{self, Captions, Word};

let words = vec![Word::new("Every", 0.20, 0.55), Word::new("cut", 0.55, 0.85)];
let lines = captions::lay_out(&words, &Captions::default());          // the line breaks, decided
let reel = captions::sequence(&words, &Captions::default(), 1920, 1080, 30.0);  // ready to render
```

The line breaker is the whole value:

| Rule | Why |
| --- | --- |
| a line ends at a **sentence end** (`.` `!` `?` `…` `:` `;`) | breaking mid-clause reads wrong |
| …or at `words_per_line` (default **5**) | six is the most a phone reads at once |
| …or at `max_line` seconds (default **3.6**) | longer than that is not a caption |
| …or at a silence of `gap` seconds (default **0.6**) | a pause is a natural break |
| a line starts `lead` (0.08 s) early and holds `tail` (0.28 s) after | the words sit *inside* the line |
| no line is shorter than `min_line` (0.9 s) | a clipped word is unreadable |
| two lines never overlap | a caption is never on screen twice |
| a word longer than a line keeps its own line | cutting a word is worse than holding it |

Each line becomes **one comp of the caption preset**, placed where it is spoken, with its word times
made **relative to the line** — so the same comp keeps its motion wherever it lands.

The sequence ships with the standard curtain (opaque ends, a dip at every
line change). Over a voiceover those dips read as flashes, so
`captions_animate` takes **`curtain: false`** for hard cuts between lines —
usually the right choice when the reel is burned over narration, while the
dip stays the default for a standalone caption reel.

---

## Authoring by hand: what the presets cannot say

Twelve presets are a vocabulary; a film is a sentence. Every preset in `presets.rs` is written as a
document literal, and so is anything a designer or an agent writes: the JSON *is* the authoring
surface, masks, effects, blends, gradients, dash and all five curve families included.

`src/crates/concat-motion/examples/comps/cypher.opener.json` and `cypher.spectrum.json` are the two
worked examples, and between them they exercise the whole model:

| In `cypher.opener` | In `cypher.spectrum` |
| --- | --- |
| an ellipse with a **feathered elliptical mask** and the unclamped `add` blend — a glow, not a disc | a `radial-gradient` light leak in markup, `difference`-blended over the plate |
| gradient-clipped type pulling focus on a `blur` **track** | a violet disc, `add`, masked and turning |
| a spring on `scale`, a bezier on `opacity`, `steps(8)` on the kicker, a `points` ramp on nothing else | a rounded mask on a slab, a `blur` track on type, `screen` blend |
| a CSS hairline grown by `@keyframes` inside markup | a markup card with `backdrop-filter`, a gradient-clipped heading and a growing bar |

Measured bake: `cypher.spectrum` is 162 frames of 1080p in 23 s on two cores (7 fps); the opener is
~10 s for 132 frames. Both are slotted into `examples/reel.rs`'s shape by a storyboard: seventeen
scenes, twelve presets plus the two hand-authored comps, 54.6 s, 1638 frames at 1080p30 rendered in
121 s on two cores (13.6 fps end to end).

### What Geneva 1.6.0 does and does not draw

Found by rendering, not by reading the docs — each one is now either handled in the mapper or written
in `.claude/skills/motion/SKILL.md` so an author does not walk into it:

| Ask | Result | What `concat-motion` does |
| --- | --- | --- |
| a gradient in a **shape's** `fill` | `E103`, fatal to the whole document | averages the stops to one colour and **warns**, naming the node |
| a gradient in markup, or a text node's `fill` | drawn exactly | passes it through |
| a `shadow` **effect** on a clip | not drawn (native-only) | warns, names the node |
| a CSS `box-shadow` inside markup | drawn, on an inner element | — |
| `box-shadow: inset` | `W450`, skipped | — |
| a `box-shadow` on the clip's outermost element | drawn as the **clip's** rectangle | put it one element deeper |
| a keyframe on the clip's outermost element | `E442`, refused | `sequence` animates only transform and opacity there |
| a `path` node | warned, dropped (native-only) | warns, names the node |
| `conic-gradient` | draws nothing | use a linear or a radial gradient |
| `radial-gradient(62% 74% at …)` | draws nothing | `radial-gradient(circle at 50% 60%, …)` works |
| a key time with >9 decimals | `E103` | times are written to **milliseconds** |

The last row was a real bug: a track key at a fraction of a node's span divides into seventeen digits,
and one such key made the render refuse the whole timeline. `seconds()` now writes three decimals,
which is finer than any frame the renderer draws, and a test holds it there.

---

## The agent door: `agent`, one binary, JSON in and out

Every verb prints exactly one JSON document and exits non-zero on failure.

The binary ships as `concat-motion-agent` in the Linux .deb and .rpm (on the
path, beside `concat`) and inside the AppImage (`--appimage-extract` reaches
it); an MCP agent reaches the same engine through the `motion_*` tools and
never needs it. From a source checkout it is:

```sh
cargo run -p concat-motion --bin concat-motion-agent -- list
cargo run -p concat-motion --bin concat-motion-agent -- inspect comps/funcut.lower-third.glass.json
cargo run -p concat-motion --bin concat-motion-agent -- edit comps/lt.json ops.json -o comps/lt2.json
cargo run -p concat-motion --bin concat-motion-agent -- frame comps/lt2.json 1.5 -o still.png --width 960
cargo run -p concat-motion --bin concat-motion-agent -- bake comps/lt2.json cache/lt --width 1280 --height 720
cargo run -p concat-motion --bin concat-motion-agent -- bake comps/lt2.json cache/lt --encode piece.mp4 --crf 18
cargo run -p concat-motion --bin concat-motion-agent -- reel storyboard.json -o reel.mp4
cargo run -p concat-motion --bin concat-motion-agent -- captions words.json -o storyboard.json \
    --plate assets/plate.jpg --audio assets/score.wav
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
  { "op": "set-track-points", "node": "card",
    "keys": [[0, 240, 540, null], [0.7, 960, 540, { "kind": "spring", "stiffness": 170, "damping": 26, "mass": 1 }]] },
  { "op": "add-text", "id": "kicker", "text": "CHAPTER 02", "size": 28, "weight": 700,
    "color": "#22d3c5", "x": 240, "y": 140 }
] }
```

Two commands get from a transcript to a finished caption video: `captions` breaks the words into lines and
writes the storyboard, `reel` renders it. The transcript is `[["Make", 0.2, 0.55], …]`, a `{"words": […]}` wrapper,
or objects with `text`/`start`/`end`.

`reel` is the whole-sequence verb: a storyboard of scenes (a preset id or a comp file each, with
operations, layers and a hold), shared assets, audio beds and a curtain, in one JSON document.
It is the library's own contract (`concat_motion::storyboard`) - the same document the MCP
`motion_reel` tool takes, so a reel that renders one way renders the other way too.

`bake --encode out.mp4` renders the staged timeline once more into one video file the timeline
plays like any other footage - `.webm` asks for VP9, `.mov` for ProRes 4444, anything else H.264.
The sidecar's video codecs carry no alpha (only the PNG sequence keeps it), so the reply carries a
`notes` line saying so on every transparent comp: `--encode` is for full-frame pieces, and an
overlay's truth stays in the bake.

```json
{ "width": 1920, "height": 1080, "fps": 30,
  "assets": { "back": "assets/back.jpg", "mid": "assets/mid.png", "leak": "assets/leak.png",
              "score": "assets/score.wav" },
  "scenes": [
    { "preset": "funcut.chapter.chip", "hold": 3.0,
      "layers": [
        { "asset": "back", "zoom": 0.04 },
        { "asset": "mid", "pan": [-0.03, 0.0], "fade_in": 0.5, "fade_out": 0.8 },
        { "asset": "leak", "blend": "screen", "scale": 0.8, "offset": [-0.25, -0.2],
          "fade_in": 0.4, "fade_out": 0.4, "above": true }
      ],
      "ops": [ { "op": "set-slot", "slot": "Chapter", "value": "01 / INTRO" } ] },
    { "comp": "comps/mine.json", "at": 3.0, "plate": "back", "zoom": 0.03 }
  ],
  "audio": [ { "asset": "score", "gain_db": -2.0 } ],
  "curtain": { "color": "#05060aff", "open": 0.6, "close": 0.8, "dip": 0.2, "depth": 1.0 } }
```

### Multi-layer scenes

A scene's `layers` stack **bottom first**: plates under the comp, and - with `"above": true` -
grain, vignettes, light leaks and foreground cards over it. The short form (`"plate": "back",
"zoom": 0.04`) is still one layer under the comp and nothing else, exactly what it always was.

Every layer moves on its own clock, and that is the whole point: a background drifting four
percent while a foreground card slides two is parallax, and parallax is what makes a stack of
stills read as a scene rather than a collage.

| Field | What it does | The read |
| --- | --- | --- |
| `asset` | an asset id from the storyboard's `assets` | required |
| `zoom` | growth over the span, as a scale delta | `0.04` is life without a zoom |
| `pan` | drift over the span, `[x, y]` fractions of the frame | `-0.03` slides left three percent; a cover layer grows by its own travel so the edge never shows |
| `offset` | static offset from the centre, `[x, y]` fractions | `-0.25, -0.2` sits up and left, like a light leak |
| `scale` | base scale multiplier | under `1` shows the frame's edges on purpose - a card, not a plate |
| `fit` | `cover` (default), `contain`, `fill`, `none` | `cover` is the plate; anything else is a shape |
| `blend` | `normal` (default), `multiply`, `screen`, `overlay`, `darken`, `lighten`, `difference`, `soft-light`, `add` | `screen`/`add` turn a soft radial picture into a light leak; `multiply` shades one into a vignette |
| `opacity` | steady opacity, 0 to 1 | `0.08` grain over everything |
| `fade_in` / `fade_out` | fade up and down, seconds (`fadeIn`/`fadeOut` also parse) | held to half the span so the two never cross |
| `start` / `hold` | its own timing, from the scene's start | a foreground that arrives at `1.2` lands over a scene already running; a layer that outlives its scene is cut to the scene's end |
| `above` | paint over the comp rather than under it | grain, vignettes, light leaks, silhouettes - still under the curtain and under any scene that overlaps this one |

The layers a scene paints, bottom to top: its plates in order, the comp, its overlays in order,
the curtain. A layer naming an asset the storyboard does not carry stops the plan and names the
scene - a missing picture is a mistake to hear about, not one to render around.

The recipes worth reaching for first:

- **Parallax depth** - `back` at `zoom: 0.04`, `mid` at `pan: [-0.03, 0]` with a slow fade in
  and out, the comp over both. The differing drift is the depth.
- **A light leak** - a soft radial picture, `blend: "screen"`, `scale: 0.8`, `offset` into a
  corner, `above: true`, fading in and out over half the scene.
- **Film grain** - a grain tile at `opacity: 0.08`, `above: true`, running the whole scene.
- **A vignette** - a dark radial picture, `blend: "multiply"`, `above: true`.
- **A foreground card** - `fit: "contain"` or a `scale` under one, `start: 1.2`, `fade_in: 0.3`:
  it arrives over a scene already running and leaves before the cut.

Operations: `set-slot`, `set-text`, `set-words`, `set-markup`, `move`, `retime`, `set-track`, `set-track-points`,
`fade-in`, `slide-to`, `pop`, `add-group`, `add-rect`, `add-text`, `delete`, `set-blend`,
`set-opacity`, `set-duration`, `set-background`.

`set-markup` hands a markup node whole new HTML - the op a data-driven comp needs, where the words
alone cannot say it. The chart preset, for instance, ships its bar labels as slots for a ranking, and
real bar heights arrive as one rewrite that keeps the preset's stylesheet and rise animation:

```json
{ "op": "set-markup", "node": "sheet",
  "markup": "<div class=\"sheet\"><div class=\"head\">…</div><div class=\"plot\"><div class=\"col\"><div class=\"bar\" style=\"height: 26%\"></div><div class=\"lab\">YOU $180K</div></div>…</div></div>" }
```

---

## In the project

`concat-project` carries a motion clip:

```json
{ "kind": "motion", "motion": { "comp": "funcut.lower-third.glass", "hash": "…", "engine": "geneva" } }
```

The comp file lives in the project's `comps/` folder, named by its hash, so two clips placing the same
comp share one bake. `MotionClip::bakes_at(w, h)` is the cache-resolution test.

**Where alpha actually lands.** The sidecar's H.264/VP9/ProRes encoders write opaque video, and
FFmpeg's WebM muxer does not reliably emit VP9 `AlphaMode` (verified: `yuva420p` input round-tripped as
`yuv420p` with every alpha byte at 255). Claiming an "alpha video" would be a lie. The MCP tool
`motion_overlay` is the honest path: it takes the baked RGBA PNG sequence (`frames` or `hash` +
`project`) and composites it over the **real footage** (`plate`) at `at` seconds with ffmpeg's
`overlay` filter, honouring the comp's transparency at the one step where it matters. The result is an
ordinary H.264 clip that `media_import` + `clip_add` place on the timeline above (or instead of) the
plate. Without a plate it composites over a solid colour, which is what `--encode` already did but
with the alpha handled correctly. The RGBA sequence itself is never modified, so the same bake can be
re-composited over a different shot.

Try:

```
motion_bake  {project, preset, width, height}      -> dir, hash
motion_overlay {project, hash, plate: "<b-roll.mp4>", at: 1.2, fps: 30}
media_import / clip_add  -> the composited clip
```

**`ClipKind::Motion` in an export.** `concat-export/src/flatten.rs` now resolves a motion clip to
the **encoded** bake in the project's `motion/` folder (matched by the comp's content hash, `.mp4`
preferred), and rejoins it as an ordinary video clip with the clip's keys, blend, effects and
transition carried through. If only the RGBA PNG sequence exists there is no file the exporter can
decode, so it logs a warning naming the clip and tells you to run `motion_bake {encode:true}` or
`motion_overlay` rather than silently dropping the comp from the render. Unit-tested in
`flatten.rs`. Note that no edit command currently creates `ClipKind::Motion` clips, so in normal
use comps still reach the timeline as media clips; this closes the gap for hand-authored documents
and the motion agent's output.

**Still to come** (nothing here is faked): a native PNG-sequence decoder inside the exporter (which
would let a transparent bake export without an intermediate encode), the `motion.*` API methods, the
Lottie engine, and the native engine's `CompEngine` impl.

---

## Rules of thumb for a good comp

1. **One focal moment per scene.** Everything else supports it.
2. **Ease with `EMPHASIS` or a spring, never linear** — linear is what makes motion look cheap.
3. **Stagger by 60–120 ms**, not all at once.
4. **Type is the design**: weight contrast, tight tracking, one accent colour.
5. **Alpha is the contract**: a comp meant to sit over footage must not paint a background.
6. **Bake at the size you cut at**, not at 4K "for safety" — the compositor samples it.
7. **Nothing is silent**: an unsupported feature is reported by node id, never drawn as something else.
