# The determinism contract

> For a given composition - the document, the **bytes of every asset it
> points at**, the frame size and rate, and the engine build - FUNCUT's
> motion renders are **bit-identical across runs, across processes and
> across machines**. Same inputs, same bytes, every time. This page is
> the guarantee, what enforces it, and what it is for.

Determinism is not a nicety here; it is the foundation of four things
that are nothing without it:

- **The caches** (`cache.rs`, `reel.rs`, the bake manifest) key on
  content. A key that could serve two different renders is a cache that
  lies; a render that changes between two identical invocations is a
  cache that can never hit.
- **The golden frames** (`scripts/motion/golden.py`) compare pixels. They
  are meaningless against an engine whose output drifts.
- **The farm** (`farm.rs`) joins parts rendered by separate processes
  into one file. A part that renders differently from its neighbour is a
  seam.
- **AI-driven editing** only converges when a fix, applied again,
  produces the same pixels it produced the first time. A render with
  mood is an agent's dead end.

## What the promise covers

| Input | Where it comes from |
| --- | --- |
| the comp document | the JSON itself, hashed canonically |
| the **bytes** of every referenced asset | streamed and hashed into the bake key (`manifest.json`'s `assets`) and every reel key - a re-saved picture is a new key, whatever its name says |
| width, height, fps, alpha | the bake request / the storyboard |
| the engine and its version | the pinned Geneva sidecar, checked against a digest by the installer and `scripts/ci/fetch.sh`; the manifest records the version it used |

Given those, `Geneva::bake`, `render_reel` and `farm::render` produce the
same PNG frames and the same joined encodes. The suite proves the halves:

- **Across runs, one machine**: `scripts/motion/golden.py` renders one
  frame twice and asserts the two PNGs are the same **bytes** - the
  shortest possible statement of the promise, checked on every CI run.
- **Across machines**: the same pinned binary, the same staged assets
  (the render directory is self-contained by `Plan::write`), the same
  committed fonts. The golden frames hold at DSSIM 0 against renders on
  this machine; a machine with the repo and the pinned toolchain renders
  the same.

## What the contract forbids

The document cannot ask for anything that answers differently twice.
`Comp::validate` **refuses** it before anything is staged or rendered,
with the token that broke the promise and a JSON pointer to fix:

| Forbidden | Why |
| --- | --- |
| `url(http://…)`, `url(https://…)`, `url(//…)`, remote `src=`/`href=` | a mid-render fetch: the network's answer is not the document's |
| `@import` of anything | a stylesheet read at render time that was never staged |
| `Math.random(` | unseeded randomness: no two runs agree |
| `Date.now(`, `new Date(` | the wall clock: not a function of the document |

`data:` URLs are inline bytes - welcome. Relative paths are the staged
assets' own names - welcome. And the randomness the contract *does*
allow is exactly the seeded kind: the noise fields (`noise.rs`,
`Wobble`, `set-noise-track`) take a **seed** the document carries, so a
wobble is as deterministic as a keyframe - the seed is the whole of it.
An absent seed is a refused parameter, never a silently-picked one. The
particle fields (`add-particles`) are the same promise one level up: the
seed drives the simulation, the simulation is baked into keyframes at
authoring time, and a frame asked for out of order is answered from the
document's own numbers. The camera (`set-camera`) adds no randomness at
all - it is arithmetic folded into the tracks the layers already carry.

On the engine side, the sidecar makes the same promise structurally:
no JavaScript, no network, asset paths confined to the render directory,
and the whole timeline staged before the first frame is drawn. FUNCUT
holds its side of that by refusing majors other than the one it was
written against, and by recording the version every bake used.

## What enforces it

| Guarantee | Enforced by |
| --- | --- |
| the document refuses non-determinism | `css::network_refs` + `Comp::validate` (tests in `css.rs`, `comp.rs`) |
| the cache key covers the files | `geneva::bake_assets_key` (streamed), `reel::scene_key`'s asset hashes - "changed logo, new bake" is a test |
| the cache never serves a half-written entry | atomic publish + `.done` sentinels (`cache.rs`); the manifest is written last |
| renders are byte-stable | `golden.py`'s twice-rendered frame, `DSSIM` goldens in CI (`.github/workflows/geneva.yml`) |
| noise is seeded | `noise.rs` is a pure function of its seed; tests hold two machines' worth of the promise by construction |

## What it means for you

- **Editing an existing comp**: keep the seeds. Two comps with the same
  seed wobble in step - sometimes exactly the point, usually not: give
  each layer its own.
- **Intending a visual change**: `golden.py --update`, and say so in the
  commit. The goldens are the record of what the engine draws; they move
  when the drawing moves, and only then.
- **Found a non-determinism**: that is a bug with a page to cite. The
  contract above is the spec; the refusal you hit is the enforcement -
  and if something non-deterministic slipped past the guards, the golden
  frames will find it on the next run.
