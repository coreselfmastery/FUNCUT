# IMPLEMENTATION_LEDGER.md — what is done, what is next

Legend: each unit is a coherent implementation with tests, committed and
pushed separately. Status: done / in-progress / planned / dropped(why).

## Verified repository state at baseline `ab7c0704`

Working and CI-proven (7/7 checks green at baseline):

- Editor: multi-track editing, multi-timeline model + tab UI, undo/redo
  (depth 200, batch = one step), proxies on by default at ≥1080p (H.264
  veryfast CRF23 default, MJPEG opt-in), atomic exports with cancellation,
  codec capability probing, 109 MCP tools with full schema tests, director
  skill on the segment-based plan contract.
- Motion: comp document (10 node kinds, 5 curve families, tracks, slots,
  sub-comps 8 deep), Geneva sidecar binding (limits, breaker, cache with
  asset-bytes keys), sequences/reels with 6 choreographed transitions,
  pulse (audio-reactive bake), seeded noise/wobble, grade/looks/styles,
  critic loop with applicable fixes, 5-family lint, kinetic typography
  builder, captions pipeline, 10 chart kinds, particles (deterministic,
  baked), 2.5D camera (from/to + depths), 18 presets, farm + progress.
- Honest limitation, documented in-repo: Geneva 1.6.0 cannot draw paths,
  variable-font axes, or in-keyframe weights; the code measures and guards
  each refusal.

## Known real gaps (audited this session, code-verified)

| # | Gap | Evidence |
| --- | --- | --- |
| G1 | Latent E303: `keyframes()` rounds key times to ms but never dedups after rounding → two keys <0.5 ms apart refuse the whole bake | geneva.rs `keyframes()`, pulse.rs merge does dedup, camera.rs dedups pre-rounding only |
| G2 | No particle presets (snow/rain/embers/...) — the simulator is finished, the vocabulary is not named | particles.rs |
| G3 | Camera is from/to only; no multi-waypoint paths | camera.rs `CameraMove` |
| G4 | Export never verifies its own output file after the atomic rename | concat-export/src/lib.rs:1535 |
| G5 | Docs drift: MOTION.md "still to come" lists the PNG-sequence export that flatten.rs already has (d07597c8); lib.rs:19 says the native engine is "already shipped" (no impl exists); geneva-adapter lib.rs:34-37 claims Lottie "plays as an asset" (no player exists); FEATURES.md:315 omits director Phase 1c | files cited |
| G6 | Native `CompEngine` is implemented on main (PR #79); remaining unsupported render kinds include Lottie and shader packages, which must continue to warn rather than silently disappear | `concat-motion/src/native/{mod.rs,scene.rs}`; PR #79 |
| G7 | Dead code: `BeatGrid` never constructed, `Value::Size` produced/consumed nowhere, `BakeRequest.crf` unread by the only impl | pulse.rs, track.rs, engine.rs |

## Units

| Unit | What | Status | Commit (as merged) |
| --- | --- | --- | --- |
| 1 | PR #76 fmt fix, CI green, **merged** (`6dafbeee`, 7/7 checks) | done | `54e13922` |
| 2 | PR #73: verified fully re-landed via PR #74; no merge needed | done (no code) | — |
| 3 | State files (this set) | done, on main | `94ebfdde` (7/7) |
| A | Fix G1: E303 key dedup after ms rounding + regression test | **merged via PR #78** | `b4af211a` |
| C | Fix G2: particle presets (snow, rain, embers, sparks, dust, atmosphere, confetti, fireflies, energy, streaks, data) on `add-particles`, patch semantics | **merged via PR #78** | `649df220` |
| D | Fix G3: multi-waypoint camera paths, per-axis stops, per-segment curves | **merged via PR #78** | `82483657` |
| F | Fix G4: post-export output verification probe (`verifying` stage) | **merged via PR #78** | `4c431485` |
| G | Fix G5: docs truth pass (four corrected claims) | **merged via PR #78** | `829fcac3` |
| — | Ledger section 47 + state files + handoff | **merged via PR #78** | `1ce22147`, `68dbf225`, `4b9f8f81` |
| B | `motion_edit` MCP tool | dropped — `motion_bake` already persists ops-edited comps; 242k token cap has 119 tokens headroom; convenience not worth the guarded budget | — |
| I | Fix G6: native `CompEngine` impl | **merged via PR #79** | `de804c75` |

**PR #79 merged at `de804c75`** after all five checks passed on its
head: native `CompEngine` (shapes, paths, text, stills, nested comps, masks,
mattes and supported effects). **PR #78 merged at `3adb448c`** after all five checks passed on its
head `4b9f8f81`: Format, locales and models; The analysis senses against
real fixtures; The dependency dance, pinned and green; Engine tests and
lints; The multi-layer example, rendered and inspected (the Geneva
sidecar lane — the motion units rendered through the real engine).

Dropped (with reasons):

- `export_cancel`/`project_close` in the director allowlist — deliberate,
  tested safety surface ("a harness must never call"), not a gap.
- VAAPI encode/decode wiring in the app — real gap on a Linux-only fork,
  but cannot be honestly validated without the hardware; recorded here
  rather than shipped untested.
