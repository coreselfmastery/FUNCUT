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
| `funcut.lower-third.glass` | Lower thirds | an editorial lower third — an accent bar that wipes down, a kicker, a name, a role (no avatar block, no LIVE badge) — slot: Name |
| `funcut.caption.kinetic` | Captions | word-timed caption row with a highlight — slot: Transcript line |
| `funcut.stats.triptych` | Stats | three stat cards, bars growing |
| `funcut.chart.bars` | Charts | a bar chart that builds on |
| `funcut.ticker.count` | Stats | one big number, label, drawn rule |
| `funcut.sting.wordmark` | Stings | solid-ink wordmark with one accent full stop and a rule that draws under it — slots: Wordmark, Subline |
| `funcut.quote.pull` | Quotes | pull quote with attribution |
| `funcut.chapter.chip` | Chapters | number, label, underline |
| `funcut.cta.subscribe` | Calls to action | a typographic ask — one accent word, an outlined button that settles once |
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

### The guard: limits, a breaker, and diagnostics that parse

Every call to the sidecar is bounded and counted, so a sidecar that hangs or fails is a message, not
a session:

- **Per-call limits.** A render runs under `CallLimits::render` (300 s by default); every quick call
  - plan, validate, one frame, the version - under `CallLimits::plan` (60 s). A call that outlives
  its limit is **killed** and refused with `MotionError::TimedOut`, which names the call and the
  limit. The pipes are drained while the child runs, so a chatty sidecar cannot deadlock on a full
  buffer either. `--geneva-timeout SECONDS` (the `agent reel` verb) and `genevaTimeoutSecs` (the
  `motion_reel` tool) cap every call at the number given - a render at it, a quick call at the
  lesser of it and the quick default.
- **A circuit breaker.** Three failures inside sixty seconds - a refusal, a crash, a timeout, from
  any door - open the breaker for a minute: further calls are refused immediately with
  `MotionError::CircuitOpen` instead of being spent on a sidecar that is not answering, and a
  success closes it again. `EnginePreference::choose` folds the breaker into the engine choice: a
  failing sidecar is a missing one for every comp the native engine can draw (with the note that
  says so, because a fallback nobody can see is a fallback nobody can trust), and a markup comp
  keeps Geneva - the native engine cannot draw it, and the honest answer is the fast refusal.
- **Diagnostics that parse.** `--format json` is asked for on every invocation, so a refusal
  arrives as [`geneva::Diagnostic`] - code, severity, the JSON-pointer `path` into the timeline,
  message, value, help - and `MotionError::Engine` carries the code, the message, the location and
  the most specific help available: the engine's own suggestion, else what `geneva::fix_for`
  knows about that code in FUNCUT's terms (E302 is clip overlap, W405 is a font the machine
  lacks - the table is the sidecar's own `geneva guide errors`, said in storyboard words), else
  `geneva explain <code>`, asked of the sidecar itself.

```rust,ignore
use concat_motion::geneva::{Breaker, CallLimits, Geneva};
use std::time::Duration;

// a render that must not take all day
let geneva = Geneva::discover(None).expect("the sidecar")
    .with_limits(CallLimits::capped_at(Duration::from_secs(120)));
match geneva.breaker() {
    Breaker::Open { failures } => { /* draw natively, or wait a minute */ }
    Breaker::Closed => { /* call as usual; a hang costs its limit, not the session */ }
}
```

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
  strictly increase. **Unless the cut is choreographed** - see below, the curtain stands down at a
  boundary that carries a [`transition`], because a whip pan and a colour wash at the same frame is
  two transitions fighting.
- **The output is one timeline**: a scene's own @keyframes, its plates and overlays, the curtain and
  the audio beds, rendered in a single pass because they are one document.

`Plan::write` copies every asset into the render directory, so the folder renders on a machine that has
nothing else; `Plan::render` writes it and calls the engine.

### Scene-to-scene transitions: the cut, choreographed

The router knows the beat; the curtain dips through a colour. Neither of those is a *transition* -
a dip is the admission that nothing was choreographed. A **`transition`** on a scene card says what
the moments either side of the boundary do, written on the scene that **enters by it**:

```json
{ "scenes": [
  { "preset": "funcut.sting.wordmark", "hold": 4.0 },
  { "preset": "funcut.chart.bars", "hold": 6.0,
    "transition": { "kind": "whip-pan", "direction": "left", "seconds": 0.24, "blur": 12 } } ] }
```

| Cut | What both sides do |
| --- | --- |
| `whip-pan` (`direction`, `seconds` 0.24, `blur` 12px) | the outgoing plates and comp sweep out, accelerating; the incoming sweep in from the opposite side, settling - and both sides smear through the cut, racking to `blur` pixels at the boundary and resolving off it: the motion blur that sells the whip. `blur: 0` is the dry whip - the sweep without the smear, which is a legitimate read |
| `zoom-through` (`seconds` 0.35) | the outgoing grows to 135%, the incoming arrives still at 135% and settles - one continuous push |
| `inverse-zoom` (`seconds` 0.35) | the pull-back reading: the outgoing shrinks to 72%, the incoming starts small and grows - a breath. On a cover-fitted plate the shrink shows the frame's edges for its duration; that is the read of a pull-back |
| `rack-focus` (`seconds` 0.3, `radius` 16px) | the outgoing blurs out, the incoming resolves in - "same world, new subject" |
| `velocity-match` (`seconds` 0.4) | the incoming plate enters carrying the outgoing's own drift velocity - direction *and* speed, read from the plate - and settles; the outgoing keeps drifting exactly as it was. The cut hides itself |
| `stagger-exit` (`seconds` 0.22, `step` 0.06) | the outgoing comp's elements leave one by one - the front of the list first, the last gone exactly at the cut - not on the same frame |

Both sides are written into the timeline the engine already plays: keyframes on the layers'
transforms and blur effects, and tracks appended to the comp's own nodes - a transition *rides on
top of* the authored animation, never rewriting it (a node whose own track runs past where the
cut's keys would land keeps what it wrote; a node that is never placed keeps its place). The
`transition` on a **first** scene is the way the reel opens; an **`exit`** on a scene is its own way
out - the fallback when the next scene names none, and the only way to choreograph the last scene's
leaving (a staggered exit at the reel's end is the credits rolling out).

**The two sides share the window.** A scene that enters by an overlapping cut starts *before the
scene before it ends* - by the cut's own `seconds`, the plan lays them that way - so the outgoing
is still on screen, sweeping out, while the incoming is already arriving: one frame holds both,
which is what makes the whip read as one camera move and the rack as one focus pull instead of an
exit, a splice and an entrance. Without the overlap every choreographed cut degrades to a plain
cut, however beautiful its two halves. The `stagger-exit` is the deliberate exception: its
elements are gone *exactly at* the cut, so the incoming's first frame is the reveal and the two
never share a window. A cut longer than the scene before it can spare is clamped to the overlap
that fits, and the plan says so.

The scene-aware reel cache carries the cuts into each cached segment, and the shared window makes
that a rule of the layout: the scenes an overlapping boundary ties together are **one render
unit** - a composite no concat can express - so the segmented cache renders them as one *run*,
cached by one key (an edit to either scene re-renders the run, never the reel). A boundary
without an overlapping cut is where one run ends and the next begins, so the per-scene
cache granularity survives everywhere else. The segmented cache and the one-shot render agree
frame for frame, and a changed choreography is a changed scene, so the cache key carries the cuts
too. The `whip` golden frame is this made visible: both plates on screen inside the window.

### The pulse: audio-driven keyframes, baked into the document

The engine computes every frame from its timestamp, so a keyframe is just a value at a time - and
the values at times are exactly what a beat grid, an SFX transient and an emotion read already
are. `pulse.rs` is the arithmetic between them:

```json
{ "preset": "funcut.chart.bars", "hold": 5.0,
  "pulse": { "beats": [0.98, 1.94, 2.91], "driven": "scale",
             "amount": 0.05, "release": 0.32 } }
```

| Piece | What it does |
| --- | --- |
| `beats` | the measured grid, in seconds from the scene's own start - `suggest_cuts` reads it from the soundtrack, the recogniser times the words; nothing here trusts a guess |
| `driven` | `scale` (the chart grows on the downbeat), `opacity` (the caption pulses on the transient - a full-opacity node dips, because brightening nowhere is not a pulse), or `position` (the card takes the beat) |
| `amount` / `release` | the bump's size and its settle - the tail that makes a hit read as a hit rather than a flicker |
| `curve` | how the bump decays: the library's `spring-pop` is the drum read, written as a name from the easing library (`bounce`, `typewriter`) or a full curve object |
| `nodes` | the ids the pulse drives; empty is every node the comp carries |

The keys **merge** with the nodes' own tracks - an authored fade, a choreographed transition and a
heartbeat all ride the same property without one rewriting the other, sorted by time, the beat
winning its own moment. Beats outside a node's own window do not reach it; a node that starts at
2 s does not pulse at 0.5 s.

The scene card can also name its **`act`** - one of the seven the emotion read uses (`hook`,
`setup`, `tension`, `turn`, `reveal`, `proof`, `cta`) - and the act chooses the pulse's hand
when nobody else did: a pulse that names no curve of its own gets the act's, wired in by the
planner itself rather than left for the caller to translate. A pulse that names a curve keeps
it; the author's hand beats the read's.

```json
{ "preset": "funcut.chart.bars", "hold": 5.0, "act": "hook",
  "pulse": { "beats": [0.98, 1.94, 2.91], "driven": "scale",
             "amount": 0.05, "release": 0.32 } }
```

Two more pieces of the same idea: **`curve_for_act`** maps the emotion read's act to its easing
curve - calm for the hook (`ease-out`), wound for the tension (`ease-in`), punchy for the payoff
(`spring-pop`) - and **`sound_starts_at`** places an SFX by its measured `syncAt` so the *peak*
lands on the entrance, not the file's first sample.

### Organic motion: noise with a seed, not a mood

A pan is a sentence a layer says once; a **wobble** is a voice - motion that never repeats, never
resolves, and never reads as keyframed, because it was not. It is `noise.rs`'s seeded simplex
field, sampled into the position keys when the plan is laid, folded onto the drift the layer
already carries. The determinism contract holds exactly: the **seed is the whole of the
randomness**, so the same layer wobbles identically on every machine, in every render, forever
(see `docs/DETERMINISM.md`) - and two layers on one seed read two channels of one field, never
wobbling in step.

```json
{ "asset": "back", "zoom": 0.04,
  "wobble": { "seed": 2026, "amount": 0.008, "rate": 8 } }
```

| Piece | What it does | The read |
| --- | --- | --- |
| `seed` | the noise field - the one randomness the contract allows, because it is a function of the document | `0.008` is a plate breathing; `0.02` is a hand-held read |
| `amount` | the reach, in fractions of the frame each side of the drift, like `pan` | a cover-fitted layer grows to cover its own wobble, the way it grows for a pan |
| `rate` | keys a second the field is sampled at - and how fast it is read (one cycle per ten keys): one knob, one feel | held to 1..=30 whatever the document says |

A cut's own windows are never chopped into samples: the sweep keeps its two keys and its curve,
the wander folded underneath, so a whip pan over a wobbling plate still eases exactly the way a
plain layer's does. Inside a comp, the `set-noise-track` operation is the same idea on a node -
`{"op": "set-noise-track", "node": "glow", "property": "scale", "seed": 7, "amount": 0.05,
"rate": 10}` bakes the field into the node's own track (position in pixels, placed nodes only;
scale, rotation and opacity around the node's resting value, opacity held inside 0 to 1).

### The grade: colour grading inside the comp

A LUT is a lookup the footage passes through, and a comp cannot carry one - but it can carry the
CSS that approximates it. `grade.rs` bakes a scene's grade into the comp as its **first paint**:
a full-bleed box whose `backdrop-filter` (`saturate()`, `hue-rotate()`, `contrast()`,
`brightness()`) does the tonal work and the hue turn, and whose translucent tint, blended
`soft-light`, does the cast. What the engine has under that box - the scene's plate, the layers
beneath - is what gets graded; the comp's own words stay as inked, which is the whole point of
grading inside the comp rather than over the render.

```json
{ "preset": "funcut.sting.wordmark", "hold": 5.0, "plate": "back",
  "grade": { "temperature": "cold" } }
```

| Piece | What it does |
| --- | --- |
| `temperature` | the seven the emotion read names per line - `hot`, `sickly`, `cold`, `warm`, `balanced`, `cool`, `split` - each a tuned row of tonal moves, a hue turn and a cast; the act a line sits in is the grade its scene wears |
| `amount` | 0 to 1, how much of the cast is put on; the tonal moves are not scaled by it |
| `saturate` / `contrast` / `brightness` | explicit overrides of the temperature's moves - a balanced grade with `{"saturate": 1.2}` is a neutral tonal move by name |
| `hue` | `hue-rotate()` in degrees, -360 to 360 - the move that turns the whole palette rather than laying a colour over it, the LUT approximation's third leg beside `saturate()` and `contrast()`; `sickly` carries +25° of it, `split` -5° |
| `tint` | the brand's own colour, `#rrggbb` or `#rrggbbaa`, worn instead of the stock cast - the palette is respected by wearing it |

`split` is the classic two-temperature grade - teal in the shadows, orange in the highs -
approximated as a top-to-bottom gradient of the two casts. Measured against the engine on a
neutral grey plate, the warm grade at full amount moves the frame **+40 red and -10 blue** and
leaves the comp's own text untouched; the CI example wears `cold` on its hook and `warm` on its
payoff, and `scripts/motion/inspect-frames.py` asserts both shifts channel by channel, and the
ink, on every run.

A scene whose grade has nothing under its comp - no plate, no lower layer - is told so in the
plan's warnings: the cast would land on the curtain alone, and a grade nobody can see is a grade
the author should hear about.

### The looks: grain, glass and the chromatic fringe

The effect catalogue owns the pixel shaders, and they run on footage through the renderer's own
compositor; a comp is a document, and the engine that bakes it is a CSS raster with no shader
stage. `looks.rs` writes the CSS that approximates the three textures every template reaches
for, as nodes the engine draws in the comp's own paint order:

```json
{ "preset": "funcut.lower-third.glass", "hold": 4.0, "plate": "back",
  "look": { "kind": "grain", "amount": 0.4, "size": 3, "seed": 7 } }
```

| Look | Where it sits | What it is |
| --- | --- | --- |
| `grain` | over everything, ink included | a seeded field of motes (480 at full strength, capped where the engine's layout stays quick), half light half dark, shimmering on the node's own `position` track - twelve steps-curved keys within a few pixels of the authored place |
| `glass` | first paint, under the comp's ink | a full-bleed `backdrop-filter: blur() saturate()` frost over the plate, and a faint white wash; the words stay sharp, the world behind them goes glassy |
| `chromatic` | over everything | the edge fringe: a warm fall-off on the left, a cool one on the right, one gradient per edge - the honest approximation a browserless raster can draw; a true channel split needs per-channel masks the engine has none of |

Measured against the engine (1.6.0) on a neutral grey plate: the wash and both fringes draw;
gradients without stop positions carry the spatial fall-off (so one gradient per edge, and one
div per mote - multiple backgrounds on one element drew nothing when measured); and **markup
animations are static per bake** - zero pixels moved between two frames under a linear-timed
keyframe set - so the grain's shimmer rides the node's own timeline track, which the engine
*does* animate per frame. The timeline's animation is the timeline's; the stylesheet's is a
still. The seed is the whole of the grain's randomness - the same seed is the same grain on
every machine and every re-render, and the mote count and the shimmer's offsets come from
`noise.rs`, never from a clock.

A scene whose glass has nothing under its comp is told so in the plan's warnings, the way a
grade is: frosted curtain is fog nobody can see through. And an identity look - a grain at
zero, a frost with no blur at unity saturation, a fringe at zero - bakes nothing at all.

### The style: one layout, three personalities

A layout template says where things sit; a style says how the same layout feels. `style.rs` is
the restyle: it swaps the weights, the tracking, the leading, the timing tokens and the tempo of
the comp's own stylesheet in place, and the easing curves of the comp's own keyframes, and leaves
the geometry, the structure and the words exactly as the template wrote them. The colour comes
back as a grade - the personality's own when the scene names none - so a scene's `grade` still
wins the colour.

```json
{ "preset": "funcut.stats.triptych", "hold": 5.0, "plate": "back", "style": "punchy" }
```

| | calm | punchy | editorial |
| --- | --- | --- | --- |
| weight | 500 | 900 | 600 |
| tracking | 0.02em, open | -0.04em, tight | 0.08em, wide |
| leading | 1.6 | 1.08 | 1.45 |
| the hand | the long settle | an overshoot past the landing | even in, even out |
| tempo | 1.4x, slower | 0.72x, hurried | 1.0x, as written |
| colour | cool at 0.6 | hot | a crisp neutral, contrast 1.06 |

Nothing a style does is a guess: a typewriter's `steps()`, a ticker's `linear` count and a
spring keep their meaning - only the eased segments take the personality - and the choreographed
cuts and the pulse apply after the style, so the velocity match and the heartbeat win their own
moments. The example (`cargo run -p concat-motion --example style`) renders the same stat-card
layout as all three over the same plate, and `scripts/motion/inspect-styles.py` measures that
they are three visibly different scenes - pairwise, in the rising moment and the settled one -
that the colour runs the way the personalities say, and that punchy lands first.

### The critic's fixes, applied

The contact-sheet critic loop runs, and it scores. What it did not produce was a revision an
editor could apply without translating it by hand - "the chart enters 400ms too early" was a
sentence, not an operation. `craft_critique` now returns a **`fixes`** array - each fix a
`{where, problem, change, op}` with the number and unit in its change, and the op from the scene
builder's own vocabulary - and `critic.rs` is the other half:

```rust,ignore
use concat_motion::critic::{self, Fix};

let fixes: Vec<Fix> = reply["fixes"].as_array()...
    .filter_map(|value| Fix::parse(value).ok())
    .collect();
let report = critic::apply(&mut comp, &fixes);   // every fix landed or is skipped with its reason
```

| op | What it patches | The change it parses |
| --- | --- | --- |
| `retime` | the node's start | `0.4s later`, `400ms earlier` - the words carry the sign |
| `shift` | the node's place, and its whole position journey | `x by -120px`, `left 480px`, `down 40px` |
| `resize` | the node's scale | `to 0.9`, `90%`, `1.2x` |
| `restyle` | the node's tracks' easing | `ease with spring-pop` - a name from the easing library, the longest match winning |
| `replace` | nothing automatically | a replacement needs new content the fix does not carry: the one op that is always a skip, said as the escalation it is |

The `where` matches node ids loosely and case-insensitively - the critic writes prose, not ids -
with the longest id winning when two match, and a comp of one node named by anything at all. A
fix that cannot be parsed is skipped **with the reason said**, never guessed at: a wrong patch
applied confidently is worse than a suggestion left on the table. The heuristic engine emits
measured fixes of its own (black lead-ins, bars, static cuts, repeats), the strong VLM's prompt
asks for exactly this shape, and every pass - scores, revisions *and* fixes - is journaled to
`craft-history.json`, so the loop from critique to revision is data the whole way through.

And the loop's convergence has a number: **`critic::score`** holds a comp to the contact
sheet's own floors - 10 to start, a contract problem costing 2, a clipped element 4 (the defect
the sheet's reader names first and loudest), an element the reel never shows 2 - so "applying
the fixes converges the score above 7" is a thing a test holds and a loop can steer by without
eyes: the deliberately broken scene (a chart clipped off the right edge) scores 6, and the same
scene under its two fixes scores a clean 10.

```rust,ignore
use concat_motion::critic;

let before = critic::score(&comp);          // 6.0 - the chart is clipped
let report = critic::apply(&mut comp, &fixes);
let after = critic::score(&comp);           // 10.0 - nothing left to name
```

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

### The lint: five families, every finding a stable code

`lint.rs` is the discipline, widened: the motion principles (still reached
through `discipline::audit`, unchanged, as the sentences the plan's warnings
always carried) plus four more families — structure, timing, assets and
determinism — every finding carrying a stable code in the lint's own `L###`
namespace. The sidecar owns `E###`/`W###`; the document's own lint owns `L`,
so the two can never collide or be read as one another's.

| Family | Codes | The rules |
| --- | --- | --- |
| Structure | `L100`–`L103` | missing comp id, duplicate node ids, empty tracks, a comp that contains itself |
| Timing | `L200`–`L203` | keys outside their span, negative starts, scenes overlapping with no transition to blend them, a cut longer than the scene before it |
| Assets | `L300`–`L302` | a file that is not there, a kind the cache cannot file, a codec the engine's own table does not name |
| Motion | `L400`–`L406` | the four principles, plus the crowd: more than five nodes entering inside one instant |
| Determinism | `L500`–`L503` | the fetch, the wall clock, the unseeded random, an asset that points off-machine |

Advice, not refusal — the same contract the principles always had. Several
rules shadow a `validate()` refusal on purpose: a comp that is about to be
refused also deserves to be told, in the words an agent can act on, before
the refusal lands. Three doors, because the families read different things:

```rust
use concat_motion::lint;

let findings = lint::audit(&comp, false);                          // the document alone
let against_disk = lint::audit_assets(&comp, source_dir.as_path()); // the asset family
let against_the_reel = lint::audit_sequence(&sequence);            // the reel-level timing family
```

Every finding carries its code, a JSON pointer into the document, the
sentence, and the fix: `[L401] /nodes/3/tracks/opacity: node "card" enters
opacity at 0% on a straight line; …`. The codes are stable — `L400` means
the same thing in every future version, a new rule takes a new code, and
`lint::RULES` is the registry a report counts from.

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

## Kinetic typography: the builder the engine's facts were waiting for

Geneva plays `@keyframes` exactly as written and times an `animation-delay` to
the millisecond. What it cannot do is animate a **weight**: `font-variation-settings`
is not drawn at all (W450 as a base style, E442 inside keyframes — a variable
font's axis is never read) and `font-weight` inside keyframes is E442 too;
its keyframes also take `px` and `%` lengths only, not `em`. All three facts
were measured against the pinned 1.6.0, and the third was found the hard way:
the first cut of this builder wrote `translate: 0 0.3em` and the engine
refused the bake. `typography.rs` is the builder that turns those facts into
a node — the arithmetic a model gets wrong and a human will not do twice:

```rust
use concat_motion::comp::TextNode;
use concat_motion::typography::{self, Entrance, Kinetic};

// A headline: three words, eighty milliseconds apart, weights ramping 100→900.
let kinetic = Kinetic::words().delay(0.08).entrance(Entrance::BlurIn)
    .flexing_weight(100, 900);
let style = TextNode { font: "Archivo, Inter".into(), size: 148.0,
    color: Rgba::hex("#f4f1ea"), ..TextNode::default() };
let headline = typography::headline("headline", "Make it move", &style, &kinetic);

// A caption: the words a recogniser timed, each lit when it is spoken.
let caption = typography::karaoke(&words, 2.4, "caption", &style, &kinetic);
```

| Piece | What it builds |
| --- | --- |
| `Stagger` — `Word` \| `Char` | one `<span>` per unit, each with its own inline `animation` and `animation-delay: i × delay` — no `calc()`, no custom properties, no cascade order to trust |
| `Entrance` — `blur-in` \| `slide-up` \| `scale-in` | the three named entrance presets, as `@keyframes` with the individual `translate`/`scale` **inside** the rule (the static-transform guard reads the output), the rises in **pixels** computed from the style's own font size — the engine's keyframes take `px` and `%` only, and an `em` there is E442 |
| `WeightFlex` — `flexing_weight(100, 900)` | a **static `font-weight` per unit**, the first at `from`, the last at `to`, interpolated between — the engine snaps a static weight to the family's own instances and draws it (measured: 100 against 900 is 1.74× the ink through this very builder), which is the one weight it draws; the animation this was once written as (`font-variation-settings` in `@keyframes`) is refused by the engine, and `css::refused_weights` refuses it at `validate` now, naming the fix |
| `karaoke(&words, …)` | the word-synced caption: each word one animation that appears it, holds it **lit** in the highlight colour for most of its own length, and settles it back — the Whisper word times wired straight into the CSS; the highlight is a colour trip, the engine being unable to animate a weight |
| `Kinetic::curved(curve)` | any of the five curve families, as the CSS timing function — `Curve::css()` writes `steps()`, `linear()` and `cubic-bezier()` natively, and approximates a spring by the overshoot its own damping ratio dictates |

In `captions_animate`, **`kinetic: true`** (or an object spelling out the
stagger, delay, entrance and weight) rebuilds each caption line as this
markup — same preset, same style, same placement as the native text-node
path, the words now arriving as a designed moment. The whole step is one
flag: the line breaking, the re-anchoring to the frame and the guard all
stay the code they already were.

`examples/typography.rs` is the done-when as a runnable comp: a
weight-ramped headline, three words staggered 80 ms apart, and a
karaoke caption from a transcript — validated, mapped to a timeline,
baked by the pinned engine (90 frames of 1080p), and written as a
document.

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
| **`transform` as a base style** (and `translate`/`rotate`/`scale`/`perspective`) | **silently skipped** — the layer is reported by name and drawn at its default position | `Comp::validate` **refuses the comp**, naming the rule and the property |
| **`font-variation-settings`** (base style) | **W450, not drawn** — a variable font's axis is never read; the text renders at the family's default instance | `Comp::validate` refuses it (`css::refused_weights`): write a static `font-weight`, which the engine snaps to the family's own instances |
| **`font-variation-settings` or `font-weight` in `@keyframes`** | **E442, refused at the bake** — the engine cannot animate a weight at all | same guard, same fix: a static `font-weight` per element — a ramp across staggered units is what `typography::flexing_weight` builds |
| **an `em` length in `@keyframes`** (`translate: 0 0.3em`) | **E442** — keyframes take `px` and `%` only | `typography` computes the rise in pixels from the style's own font size |

The `key time` row was a real bug: a track key at a fraction of a node's span divides into seventeen digits,
and one such key made the render refuse the whole timeline. `seconds()` now writes three decimals,
which is finer than any frame the renderer draws, and a test holds it there.

The `transform` row is the trap that never reports itself, so it gets its own guard. Geneva animates
`transform` — and the individual `translate`, `rotate`, `scale` and `perspective` properties — only
inside `@keyframes`; written as a base style the declaration is skipped without a diagnostic, and
every
motion feature built on top of it is wrong while the comp looks perfectly valid. `css::static_transforms`
reads every stylesheet a markup node carries — its `css` field and any `<style>` block in its markup,
comments stripped, strings kept whole — and `Comp::validate` refuses the comp with the rule, the
property and a JSON pointer, **before** anything is staged or rendered:

```text
.layer { transform: translateX(120px); }              ← refused: "the rule for '.layer'
                                                          sets 'transform' outside a @keyframes block"
@keyframes slide { from { translate: -100% 0 } to { translate: 0 0 } }
.layer { animation: slide 0.5s ease-out; }            ← the right way, validates clean
```

A layer that must *sit* somewhere rather than *move* there is placed with the node's own
`transform` (position, scale, rotation), which the timeline carries first-class — that path never
had the trap. The audit that added the guard (`grep -rn "transform:" src/crates/concat-motion/src/`
beside every `@keyframes`) found the library's own stylesheets already clean; the guard is what
keeps them that way, for a designer, a template or an agent editing the CSS next.

The three weight rows are the mirror trap, and they were found the hard way: the first
cut of `typography.rs` wrote `font-variation-settings` inside `@keyframes` on the strength
of a claim nobody had rendered, and the comp — valid, mapped, green in every test that
never woke the engine — was refused only at the bake, with E442. `css::refused_weights`
is the guard that came out of it, reading the same stylesheets the same way and refusing
both the animation the engine will not play and the base style it will not draw, with the
static `font-weight` that works as the fix. The `em` row was found the same hour, by the
same method: render it, then write down what the engine said.

---

## The agent door: `agent`, one binary, JSON in and out

Every verb prints exactly one JSON document and exits non-zero on failure.

The binary ships as `concat-motion-agent` in the Linux .deb and .rpm (on the
path, beside `concat`) and inside the AppImage (`--appimage-extract` reaches
it); an MCP agent reaches the same engine through the `motion_*` tools (and
`geneva_explain`, for any diagnostic code a refusal carries) and never needs
it. From a source checkout it is:

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
cargo run -p concat-motion --example typography -- /tmp/moment.json   # the kinetic-typography comp
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

**The contract is checked before the bake.** `Storyboard::validate` reads the document the way an
engine never gets to: every asset a layer, a plate or a bed names must be carried by `assets`;
every `blend` one of the nine, every `fit` one of the four; every `start`, `hold`, `at` and fade
non-negative; every zoom, pan, offset, scale, gain and opacity finite (and the two fractions in
range); a comp a name under the folder, not a route out of it. Every problem is found and returned
together - one refusal that lists them all, each with its scene and its layer (`scene 2, the layer
"leak": the blend "glow" is not one of ...`) - and `sequence()` calls it first, so the CLI's `reel`
verb and the MCP's `motion_reel` tool refuse the same invalid storyboard with the same list, before
a preset is resolved or an engine is woken.

Every layer moves on its own clock, and that is the whole point: a background drifting four
percent while a foreground card slides two is parallax, and parallax is what makes a stack of
stills read as a scene rather than a collage. And every layer *eases* on its own curve, which is
the other half of the point: the header settles in on `"emphasis"` while the body bounces in on
`"bounce"`, in one storyboard:

```json
{ "layers": [
  { "asset": "header", "fadeIn": 0.6, "fadeOut": 0.6, "fadeCurve": "emphasis" },
  { "asset": "body",   "fadeIn": 0.6, "fadeOut": 0.6, "fadeCurve": "bounce" } ] }
```

The easing library is ten names, every one of them a curve the engine already plays exactly as
written - `linear`, `ease`, `ease-in`, `ease-out`, `ease-in-out`, `emphasis` (the settle curve the
presets share), `spring`, `spring-pop`, **`typewriter`** (`steps(8, jump-end)` - the reveal in
clacks) and **`bounce`** (`linear(0, 1.4, 0.9, 1)` - a whole bounce in one timing function, no
JavaScript). A full curve object parses too (`{"kind": "steps", "count": 12, "position":
"jump-none"}`), and an unknown name is refused with the library spelled out rather than silently
ignored: a typo forgiven is a layer animating on the wrong curve.

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
| `fade_curve` | the easing the fades travel - a name from the easing library, or a full curve object (`fadeCurve` also parses) | `"typewriter"` for a mechanical reveal, `"bounce"` for a bounce, `"spring-pop"` for a settle; the curve rides the key that starts each fade's segment, so the hold between them stays straight |
| `start` / `hold` / `delay` | its own timing, from the scene's start (`delay` is the choreography's name for `start`) | a foreground that arrives at `1.2` lands over a scene already running; a layer that outlives its scene is cut to the scene's end |
| `parallax` | how fast it drifts, as a multiple of its own `zoom` and `pan` - or of the classic plate's 4% growth / 3% slide, when it carries none | three layers at `0.5`, `1.0` and `2.0` are three depths of one scene; the rate is applied where the timeline is emitted, so the layer's own numbers stay what they said |
| `wobble` | organic drift on a seeded noise field, folded onto the layer's own drift and baked into the position keys - `seed`, `amount` (fractions of the frame), `rate` (keys a second) | `{ "seed": 7, "amount": 0.01, "rate": 8 }` is a plate that breathes; the seed is the whole of the randomness, so the same storyboard wobbles the same way on every machine |
| `above` | paint over the comp rather than under it | grain, vignettes, light leaks, silhouettes - still under the curtain and under any scene that overlaps this one |

The layers a scene paints, bottom to top: its plates in order, the comp, its overlays in order,
the curtain. A layer naming an asset the storyboard does not carry stops the plan and names the
scene - a missing picture is a mistake to hear about, not one to render around.

The recipes worth reaching for first:

- **Parallax depth** - `back` at `zoom: 0.04`, `mid` at `pan: [-0.03, 0]` with a slow fade in
  and out, the comp over both. The differing drift is the depth. `parallax` says the same thing
  as a rate - `back` at `0.5`, `mid` at `1.0`, a foreground at `2.0` - and a layer with no drift
  of its own is given the classic plate's before the rate is applied, so three bare layers at
  three rates are a scene already.
- **A light leak** - a soft radial picture, `blend: "screen"` (the builder's `blend()` is the
  same as `blending()`), `scale: 0.8`, `offset` into a corner, `above: true`, fading in and out
  over half the scene.
- **Film grain** - a grain tile at `opacity: 0.08`, `above: true`, running the whole scene.
- **A vignette** - a dark radial picture, `blend: "multiply"`, `above: true`.
- **A foreground card** - `fit: "contain"` or a `scale` under one, `delay: 1.2`, `fade_in: 0.3`:
  it arrives over a scene already running and leaves before the cut. A stack that arrives one
  after another - `delay` 0.0, 0.4, 1.2 - is choreography; a stack that arrives at once is a
  collage.

**Card or scene is a route, not a vibe.** `concat_motion::route::route(duration_secs,
beat_type)` is the agent's deterministic default: under 5 s is a **card**; a **hook** or a
**CTA** stays a card until 8 s (both are read at a glance); 5-15 s is a **scene**; over 15 s is
a **nested** composition - an act, scenes within - except a hook and a CTA, which stay one
scene however long the slot is. The beat types are the emotion read's acts (hook, setup,
tension, turn, reveal, proof, cta), the answer carries the rule that fired as its reason, and
the director's override rides along with the reason it was taken (`Decision::overridden`) -
because an override nobody can see the why of is a guess with confidence. The full matrix is
tested; see `concat-motion/src/route.rs`.

Operations: `set-slot`, `set-text`, `set-words`, `set-markup`, `move`, `retime`, `set-track`, `set-track-points`,
`set-noise-track`, `fade-in`, `slide-to`, `pop`, `add-group`, `add-rect`, `add-text`, `delete`, `set-blend`,
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
8. **The same bytes twice**: a comp that fetches, imports or asks the clock is refused — see
   `docs/DETERMINISM.md` — and the golden frames hold the render to it in CI.

Rules 2 and 3 are not left to the mood, either: the lint (`lint.rs`, reached as
`discipline::audit` for the motion family) audits every planned scene and
rides its findings in the plan's warnings — an entrance that creeps in on `ease-in` or a straight
line, an exit that hangs in the doorway on `ease-out`, a bounce on a scene that never asked to be
playful. Advice, not refusal: the `punchy` style is the ask, and it is honoured.
The four other families — structure, timing, assets and determinism — carry
stable `L###` codes beside them, for the agent that wants to key on one.
