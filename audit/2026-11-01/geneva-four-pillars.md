# Geneva, audited against the four pillars: 1 November 2026

The in-depth review of the motion engine against the roadmap it was
given - **Creative, Performance, Reliability, Agent** - at `2d344f8a`
plus this session's uncommitted work (7 files, +572/−31). This is the
Geneva-scope companion to the [October audits](../AUDIT-2026-10-20.md):
it covers `concat-motion` (25 970 lines of Rust across 26 modules, 290
`#[test]` functions), its sidecar protocol, the sense scripts behind the
creative critic, and the CI lane that renders it.

**Method.** Every module was read; `hash.rs`, `critic.rs` and the agent
binary line by line. The three exploratory passes (engine, sequence and
reel; the six creative modules) cite file:line throughout this report.
This session had no Rust toolchain, so the Rust changes below are
verified by reading against the APIs they call and by pattern-matching
the code they follow - the compile and the 290 tests are the CI lane's
to confirm. The Python changes are verified by execution: `py_compile`
plus the behavioural checks in this report.

## In one paragraph

The four-pillar roadmap is implemented, not sketched: the typography
suite and the seeded organic layers landed with their own tests, the
cache answers to asset bytes and streams its hashes, the farm renders
in parts joined without a re-encode, the principles lint holds the
library's own presets to its taste rules, and the MCP tools see the
timeline the engine will render. What this session found was a critic
loop one half short - the patch field existed in the interrupted work
but its prose fallback had quietly changed meaning, and nothing could
apply a fix from the shell - a scene cache that served first-generation
markers as zero-length segments, and a visual regression suite that had
a golden scene but no diff. All three are closed here, with the
evidence below and the open findings registered at the end.

## Pillar I - Creative

**Typography** (`typography.rs`, 849 lines): word and character
staggering with word boundaries kept as unanimated spacers that still
count in the rhythm (`:269-334`); entrances `blurIn`/`slideUp`/
`scaleIn` with keyframes in pixels because the engine refuses em in
keyframes (E442, `:404-421`); the `typewriter` preset is character
stagger under `Curve::TYPEWRITER` - `steps()` for mechanical reveals
(`:140-148`). Karaoke: each word's span carries
`animation: kk-word {spoken_len}s {curve} {word.start}s both` - the
recogniser's word times become CSS delays directly (`:222-246`,
`:433-450`), and the agent's `captions` verb feeds it from any
transcript shape a recogniser gives (triples, `{"words": [...]}` and
object words). One honest limitation, documented in the code: the
variable-weight ramp is emitted as two stacked copies cross-faded by
opacity, because font properties cannot be keyframed on this engine
(`:185-199`).

**Motion vocabulary** (`curve.rs`, 763 lines + `principles.rs`, 532):
the easing library exposes ten named curves including two springs and
`steps()` with jump positions; the discipline is a lint, not a style
guide - `audit()` fires `entrance-eases-in` on arrivals that start
dead, `exit-lingers` on departures that overshoot, and
`overshoot-everywhere` when more than a third of animated nodes pop
(`principles.rs:83-148`). The library holds itself to it:
`every_preset_audits_clean` (`:279-298`).

**Transitions** (`transition.rs`, 1054 lines): whip pan with motion
blur (24 px, directional), zoom-through (1.35x), inverse zoom (0.72x),
rack focus (16 px radius, blur keyframed through a `Blur` effect),
velocity match (the incoming plate carries the outgoing drift's
velocity, capped at half a frame) and stagger exit. The cut **crosses
in time**: a scene entering by a cut starts `cut.seconds()` before the
previous scene ends (`sequence.rs:1367-1384`), clamped with a warning
when the window outruns the hold (`:1373-1383`); an exit with no frame
to play over is skipped and said (`:1249-1260`). Authored animation is
never rewritten - keys that land at or before the authored track's last
key are dropped (`transition.rs:717-752`, tested `:982-1025`).

**Audio-driven motion** (`pulse.rs`, 628 lines): a beat grid bakes
keyframes into the document - `Driven::Scale/Opacity/Position` - riding
the authored motion where it stands (`bump_number` samples the track's
own rest, `:255-271`); the curve per act is a table (`curve_for_act`,
`:136-146`) and an SFX's measured peak lands on the moment
(`sound_starts_at`, `:153-155`). A pulse whose beats all miss its scene
is said, not swallowed (`sequence.rs:1295-1309`).

**Procedural** (`noise.rs`, 868 lines): Gustavson simplex behind a
Fisher-Yates permutation shuffled by a SplitMix64 stream - seeded or
nothing (`:61-127`); no `rand`, no clock, no I/O anywhere in the
module. `Drift`, `Fluid` and `Particles` (clamped to 64 motes) all
carry `seed: u64`.

## Pillar II - Performance

**The two cache bugs the roadmap named "immediate priority" are fixed
and tested.** The bake key answers to the bytes under the comp:
`Comp::asset_fingerprint` hashes per-asset content through the
streaming `file_hash` (comp.rs:1070-1086), and `bake_cache_hit`
compares it as a manifest field (geneva.rs:1099-1139) - an old
manifest reads `assets: 0` and misses exactly once (test geneva.rs:
2478-2481). `file_hash` streams 1 MiB chunks and is proven to agree
with the whole-file hash across the chunk boundary (hash.rs:252-266) -
the 2 GB master clip costs one megabyte of RAM, not the machine. The
reel scene key carries the scene document *and* its grade, style,
curtain flags, geometry, engine versions and every asset's content
hash, recursively for nested scenes (reel.rs:184-286), with tests that
move the key when each of those moves (reel.rs:1081-1275).

**Lifecycle**: atomic publish - render into a private staging dir,
manifest.json written last as the sentinel, one rename into place, the
old bake moved aside and restored on failure (geneva.rs:1153-1466) -
and LRU eviction to a machine-sized budget (2 GiB, 1 GiB under 6 GB of
RAM, `CONCAT_MOTION_CACHE_GIB` to override) with the just-published
bake kept but still counted (geneva.rs:1308-1323, hash.rs:193-241).
The reel cache holds its three rooms - scenes, joins, beds - to the
same budget separately (reel.rs:579-597).

**The farm**: `geneva farm` cuts the render into N parts, each drawn by
its own `geneva worker` process, joined **without a re-encode**
(sequence.rs:629-661; PERFORMANCE.md:87-130). Workers can live on other
machines - the staged render dir is self-contained by design. A render
that cannot be split is refused with engine code E504, not a wrong
file.

**Fixed this session**: the reel cache served a first-generation marker
- the key, no duration - as a **zero-length** segment, contradicting
`marker_duration`'s own contract (reel.rs:353-361) and pinning every
later scene to the reel's beginning; it now re-renders to learn a
duration, with a regression test
(`a_marker_without_a_duration_is_re_rendered_not_served_at_zero`).

## Pillar III - Reliability

**Determinism is architectural, not aspirational**: the organic layers
are seeded simplex ("on every machine and every re-render - the
determinism contract", authoring.rs:230-233); a second render of an
unchanged reel is byte-identical because it is the same file
(reel.rs:1028-1050); engine and timeline versions are part of every
cache key, so a different sidecar misses rather than serves. There is
no clock or unseeded randomness in the render path of these modules -
the timing calls live in the launch and cache layers, where they
belong.

**Silence is the enemy**: clamped crosses, skipped exits, pulses whose
beats all miss, layers dropped for starting after the scene - each is a
warning in the report (sequence.rs:1227-1283, 1418-1433), and the
missing asset is a hard refusal before a frame is drawn (staging check
at `/assets/{id}`, geneva.rs:593-662; the sidecar's own
`validate --probe` opens every asset, geneva.rs:419-430). The sidecar
is bounded - 60 s plan / 300 s render call limits, a circuit breaker
that opens after three failures in a minute and falls back to the
native engine for the comps it can draw (geneva.rs:79-185) - and its
diagnostics come back as stable codes with a `fix_for` translation
table (geneva.rs:940-1085).

**Visual regression - closed this session**. The golden scene existed
(`examples/golden_scene.rs`: the one comp carrying a graded plate, a
kinetic headline, a karaoke caption, a particle field, an organic drift
and a fluid background, at 480x270/12 fps so it renders in a second)
but nothing compared its frames to a reference. `scripts/motion/
golden-diff.py` (pure standard library, the frame inspector's own PNG
reader) now holds a render against committed goldens with two numbers -
mean channel difference (the whisper, default limit 1.0 of 255) and
worst 16x16 block (the shout, default 8.0) - plus a manifest that names
the frame count, and `--update` as the only mode that writes. It is
behaviour-tested over nine cases (bless, identical pass, global whisper
pass, localised block fail with its place named, manifest and set
mismatches, resize, missing directory with instructions). The CI step
in `geneva.yml` wakes only when goldens are committed, because a
reference nobody has reviewed is a reference that can only fail; the
blessing procedure is documented in TESTING.md.

## Pillar IV - Agent

**Schema validation before render**, three layers: `comp.validate()`
refuses an Image/Lottie node naming an unknown asset at
`/nodes/{i}/asset` (comp.rs:963-970); the staging check refuses a
missing file at `/assets/{id}` with help that says paths are relative
to the comp's folder (geneva.rs:608-622); and the sidecar's own
validator opens every asset before the timeline runs, its refusals
carried back as `MotionError::Engine` with code, path and help -
"unknown asset 'logo.png' at layer 3" is the documented shape
(geneva.rs:1468-1477).

**MCP exposure**: `motion_probe` (a media file read by the renderer's
own reader before anything is built on it), `scene_timeline` (the
validated storyboard timeline as the engine reads it), `motion_frame`
(see a graphic before baking), `captions_animate`, `motion_reel`,
`motion_bake`, `motion_overlay` - thin wrappers over this crate, not a
second implementation. The system prompt names the loop: probe, plan,
see, render, watch, **critique**, fix, watch again.

**The critic loop - finished this session**. The interrupted work had
given `Fix` a `patch` field with parse-time validation of each op's
shape; this session found two defects in it and closed the loop:

1. *The prose fallback had changed meaning.* A retime's prose seconds
   are a **move** - "0.4s earlier" of a node at 2 s is 1.6 s, clamped
   at zero - but the new code had made them an absolute start with no
   clamp, so the same sentence on the same node read -0.4 s. The tests
   could not see it because the fixture node starts at 0, where both
   semantics agree. Fixed, and pinned by a test whose node starts at
   2 s (critic.rs, `the_prose_retime_stays_a_move_and_never_crosses_zero`).
2. *A refusal the test could not pass*: the unknown-curve refusal named
   neither "patch" nor "replacement", which the new test asserted. The
   message now says what it refused.

The loop's missing half - nothing could **apply** a fix - is the new
`agent fix <comp.json> <fixes.json|-> [-o out.json]` verb: the fixes
array a critique returns (or the `{"fixes": [...]}` wrapper), a fix
with a patch applied from its exact numbers, a fix without one read by
its prose, a fix that cannot be applied skipped with its reason, and
the edited comp's remaining problems in the same JSON document. On the
Python side, `_fix_list` now passes a `patch` through only when it is
the shape its op reads - mirrored from the Rust `check_patch`, curves
and all - and the strong-critic prompt carries the rule that keeps the
loop honest: a patch only when every number in it comes from the
measured facts, never a guess at the picture. The measured (heuristic)
fixes deliberately carry no patch, and say why in a comment: their
numbers are sampled estimates about the rendered reel, and their
`where` names frames, not nodes.

## This session's diff

| File | Change |
|---|---|
| `concat-motion/src/critic.rs` | finished the patch feature: prose-retime semantics restored and clamped; unknown-curve refusal message fixed; two new tests (prose-relative retime, patch applies-and-refuses) |
| `concat-motion/src/bin/concat-motion-agent.rs` | the `fix` verb - the apply half of the critic loop |
| `concat-motion/src/reel.rs` | v1 markers re-render instead of serving 0.0 s (+ regression test); dead statement removed |
| `concat-mcp/sense/sense.py` | `_valid_patch` + `FIX_CURVES` mirror; `_fix_list` passes valid patches; schema and strong prompt carry the patch; measured fixes documented as prose-only |
| `concat-mcp/src/tools.rs` | `craft_critique` description documents the patch and the apply door |
| `scripts/motion/golden-diff.py` | new: the golden-frame differ, stdlib only |
| `.github/workflows/geneva.yml` + `docs/TESTING.md` | the guarded golden step and its blessing procedure |

## Findings register

Open items found by the review, in the order they deserve attention.
None is a data-loss bug; all are sharp edges.

1. **A cache hit still requires a live sidecar** - `bake()` asks the
   sidecar for its version before consulting the cache, because the
   version is part of the key; the comment (geneva.rs:1232-1235) and
   one test's name overstate the guarantee. Intentional per engine.rs:
   18-21, but the comments should say so.
2. **Two definitions of "the bake key"** - `Comp::cache_key`
   (comp.rs:1099-1105) is used only by its own tests while
   `bake_cache_hit` compares `hash` and `assets` as separate manifest
   fields; the doc at comp.rs:1096-1098 claims otherwise. Reconcile or
   delete.
3. **Ranged bakes share a staging dir** - two parallel workers copying
   the same asset write the same `.part` path (geneva.rs:1240-1248,
   625-642); identical content is harmless, but a rename under the
   other's feet is a spurious failure. Uniquify per worker.
4. **An explicit `at` bypasses the overlap rule silently at plan
   level** - the crossing rule applies to default placement
   (sequence.rs:1354-1384); an author-specified `at` is placed as
   written with no warning until the cache door joins it flush
   (reel.rs:438-449). The plan should say what the cache says.
5. **`Sequence::duration()` over-reports under choreography** - it lays
   scenes flush while the plan crosses them (sequence.rs:430-449 vs
   1367-1391). Convenience-only, but a trap for a caller that trusts it.
6. **`frame()` leaks its scratch dir** when the caller passes no `-o`
   (the agent CLI cleans up only on that path; geneva.rs:1344-1354).
7. **`request.alpha` is metadata-only for Geneva** - compared and
   stored, never passed to the sidecar (geneva.rs:1118, 1283, 1450).
8. **`publish_bake`'s retired-dir naming** mangles extensions and can
   collide under PID reuse (geneva.rs:1154-1161).
9. **Eviction blind spots** - crash-orphaned `.baking-*`/`.retired-*`
   dirs carry no manifest, so `enforce_lru` neither counts nor evicts
   them; SIGKILL is the only writer.
10. **`discover` checks `is_file()`, not the exec bit** (geneva.rs:220)
    - a later spawn failure says so, less helpfully.

And the two documented approximations worth restating because they
surprise: gradient-on-shape averages to one colour (alpha averaged
unweighted), and `absolute_position` folds ancestor positions only - no
scale or rotation, depth capped at 32 (geneva.rs:1682-1748).

## What remains

- **Bless the goldens.** Render `golden_scene` on a machine with the
  sidecar, run `golden-diff.py --update`, read the frames, commit
  `src/crates/concat-motion/golden/`. The CI step is waiting, asleep
  by design.
- **Wire `motion_fix` into the MCP surface** when a toolchain is at
  hand: the `Action` enum, the dispatch, the summary and the e2e each
  want one arm. The verb it wraps already exists.
- **The findings register**, in order.
- **Run the lane.** This session's Rust is read-verified, not
  compiled; `cargo test -p concat-motion && cargo clippy` on a machine
  with the toolchain is the acceptance the reading cannot substitute
  for, and the commit that carries it should say so.

## Postscript - the disposition, 1 November, later the same day

Everything above was written before the diff landed; this records what
became of it, so the register reads true against the tree.

**The diff landed as PR #18** (`feat/geneva-critic-patch`, merged):
the critic's `patch` field with its validator and the two tests, the
prose retime kept a move and clamped, the agent's `fix` verb, the
reel's v1-marker re-render with its regression test, the sense.py
passthrough mirrored from `check_patch`, and the `craft_critique`
description that documents the patch and the apply door. One drift fix
on the way: `crate::track::Value`, the path main's critic answers to.

**The lane ran, and so did the tests it was waiting for.** The Geneva
lane went green on the branch's own tip (4895771b). The toolchain the
session went without arrived afterwards, pinned as the repo asks
(1.93.1), and the acceptance the reading could not substitute for is
now on record: `cargo test -p concat-motion` - 315 lib tests and the 6
of the agent binary, all passing in 2.5 s - and
`cargo clippy -p concat-motion --all-targets -- -D warnings` clean.

**"Bless the goldens" resolved by not carrying it.** `golden-diff.py`
was left out of the landing on purpose: main already ships
`scripts/motion/golden.py` - DSSIM against a committed board, five
fixed instants, frames rendered twice held to identical bytes - which
is the stronger suite, already wired into the lane. A second, weaker
differ beside it would be noise wearing the same name. The file stays
in this session's working tree as a curiosity, not a change.

**`motion_fix` as an MCP tool was superseded by the landed shape.**
The apply half of the loop is the agent's own `fix` verb - `agent fix
<comp.json> <fixes.json>`, a patch applied as written, a skip said
with its reason - and the `edit` verb beside it, both documented in
`craft_critique`'s description. A 92nd catalogue tool would have been
a door to the same room the agent already opens; the description
points at the doors that exist.

The findings register above stays open, in the order it deserves.

## Postscript - the register, closed: 3 November 2026

The register above stayed open for two days. This is what became of
each item, in its own order, and the tree now says so:

1. **A cache hit still requires a live sidecar** - the comment at the
   cache door now says it plainly: the version is part of the key, so
   even a hit has asked a live sidecar for it; the cache spares the
   render, not the sidecar. The intent was always engine.rs's own
   (a missing engine fails loudly rather than serving a bake it cannot
   name); the words now agree with it.
2. **Two definitions of "the bake key"** - resolved before this pass:
   `Comp::cache_key` no longer exists, and `bake_cache_hit`'s hash +
   assets comparison is the one definition, documented where it lives.
3. **Ranged bakes share a staging dir** - fixed: `stage_assets` fills
   `cache::part_path(&to)` (pid and counter, hidden, the same
   discipline every publish already had) instead of one shared
   `.part`, so two workers copying the same asset cannot truncate
   each other's copy or rename it out from under the other.
4. **An explicit `at` bypasses the overlap rule silently at plan
   level** - fixed: the plan now warns when a scene enters by a cut
   choreographed to cross but its authored `at` places it flush or
   later - the placement is kept (it is the author's) and the cut's
   sequential fate is said in the plan, not left for the join to
   reveal. Tested both ways: the flush placement warns, the author who
   crosses the cut themselves hears nothing.
5. **`Sequence::duration()` over-reports under choreography** -
   resolved before this pass: `layout()` subtracts the entering cut's
   window the same arithmetic the plan uses, so the convenience and
   the plan agree.
6. **`frame()` leaks its scratch dir** - fixed in the agent's verb:
   the frame is always copied out (to `-o`, or to a deterministic
   temp file named by the comp and the moment - the engine is
   deterministic, so a second call writes the same bytes over the
   same name) and the scratch always goes.
7. **`request.alpha` is metadata-only for Geneva** - the field's doc
   says what it moves and what it does not: RGBA engines honour it in
   the frames, Geneva's PNG sequence is always RGBA so the flag
   changes nothing it draws, and it stays a key's field and a
   manifest's. Said where the reader would otherwise mistake it for a
   promise.
8. **`publish_bake`'s retired-dir naming** - resolved by the
   ship-blockers rewrite before this pass: there is no retired
   directory anymore, only a `.part` renamed in, unique per writer.
9. **Eviction blind spots** - resolved by the same rewrite: the
   sweep counts a no-manifest directory younger than an hour as
   somebody's in-flight bake and an older one as a crash's junk, and
   a stale `.part` file in the root goes the same way.
10. **`discover` checks `is_file()`, not the exec bit** - fixed:
    `runnable()` reads the mode (Unix) and discovery's choosing half
    is `Geneva::pick`, a pure function the regression test drives
    without touching the environment discovery reads: a plain file is
    skipped, the executable beside it is found, and a missing path is
    nothing.

The two documented approximations stand as documented: gradient-on-
shape averages to one colour, and `absolute_position` folds ancestor
positions only.
