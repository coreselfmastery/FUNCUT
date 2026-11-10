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
| G6 | No native `CompEngine` impl — `NodeKind::Path`, `Effect::Shadow`, `Effect::Shader` are modeled but unrenderable anywhere; `EngineKind::Native` fallback plumbing exists with nothing behind it | engine.rs:99-142, MOTION.md:1142 |
| G7 | Dead code: `BeatGrid` never constructed, `Value::Size` produced/consumed nowhere, `BakeRequest.crf` unread by the only impl | pulse.rs, track.rs, engine.rs |

## Units

| Unit | What | Status | Commit |
| --- | --- | --- | --- |
| 1 | PR #76 fmt fix (merge the four-bug PR) | pushed `54e13922`, CI 4/5 green at last look, engine running | — |
| 2 | PR #73 resolution: verified fully re-landed via PR #74; no merge | done (no code) | — |
| 3 | State files (this set) | done | (this commit) |
| A | Fix G1: E303 key dedup after ms rounding + regression test | planned | — |
| C | Fix G2: particle presets (snow, rain, embers, sparks, dust, confetti, fireflies, energy, atmosphere) on `add-particles` | planned | — |
| D | Fix G3: multi-waypoint camera moves | planned | — |
| F | Fix G4: post-export output verification probe | planned | — |
| G | Fix G5+G7: docs truth pass + dead-code cleanup where safe | planned | — |
| I | Fix G6: native `CompEngine` impl (tiny_skia CPU raster: shapes, paths incl. dash draw-on, text via shaped runs, images, nested comps, masks, blends) | stretch — attempt after A/C/D/F/G | — |

Dropped (with reasons):

- `export_cancel`/`project_close` in the director allowlist — deliberate,
  tested safety surface ("a harness must never call"), not a gap.
- VAAPI encode/decode wiring in the app — real gap on a Linux-only fork,
  but cannot be honestly validated without the hardware; recorded here
  rather than shipped untested.
