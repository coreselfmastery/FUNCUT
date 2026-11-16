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

## Historical findings and current limitations

The original G1–G5 findings below are closed by PR #78 and the subsequent documentation/native-engine reconciliation; they are retained as history, not current blockers. PR #79 supplies the native `CompEngine`. The current product limitations are tracked in `KNOWN_ISSUES.md`.

| # | Finding | Current status | Evidence |
| --- | --- | --- | --- |
| G1 | E303 keyframe collision after millisecond rounding | **Fixed** | PR #78 regression test; `geneva.rs` keyframe normalization |
| G2 | Missing named particle presets | **Fixed** | PR #78 particle preset vocabulary and patch semantics |
| G3 | Camera limited to from/to | **Fixed** | PR #78 multi-waypoint camera paths and per-segment curves |
| G4 | Export output not verified after write | **Fixed** | PR #78 read-back probe and `verifying` stage |
| G5 | Stale motion/native-engine documentation | **Corrected** | PR #78 docs pass and PR #81 reconciliation |
| G6 | Native `CompEngine` implementation | **Implemented** | PR #79; `concat-motion/src/native/{mod.rs,scene.rs}` |
| G7 | Dead code: `BeatGrid`, `Value::Size`, and unread `BakeRequest.crf` | **Audit item; verify before removal** | `pulse.rs`, `track.rs`, `engine.rs` |

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
| J | Preserve decoded native still-image cache across bake frames | **merged via PR #80** | `dce1a750` |
| K | Explain and test mixed-markup safe refusal at selection time | **merged via PR #85** | `cf82462c` |
| L | Fail-closed Geneva installer checks + reproducible 1080p30 benchmark | **merged via PR #86** | `cc48ce81` |

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

## Units - the 2026-11-16 deep review

Branch `fix/deep-review-2026-11-16`, based on `81439cc3` (the PR #147
content, retargeted to main through PR #149 after the stacked-base
mishap recorded in AGENT_STATE.md). Seven parallel read-only reviews
(host; motion+geneva; media/render/export; mcp/director/server/api;
project/core/text/effects/vision/speech; the GUI crate; the director
harness Python + CI/packaging), each finding verified against the code
before a fix was written. No P0 crash on the Rust side; the two P0s
were in the director harness, where a dead anti-fabrication gate and a
self-feeding watcher shipped no editor bug but lied about evidence.

| Unit | What | Status |
| --- | --- | --- |
| H1 | `sounds::fetch_sound` and `magick::fetch` take a per-id turn (the bgm `turns` idiom), so the pane's worker and the API's call never share one `.partial`; regression test for the turn map | fixed + test |
| H2 | The Lo-Fi census bounds-checks the zip local header before slicing (a short 206 body no longer aborts the process under the shipping `panic = "abort"` profile) | fixed |
| H3 | `own_releases` pages on GitHub's own entry count, not the filtered list (the updater's pagination was latently short-paged) | fixed |
| M1 | Group children composite once (a child of a placed group is drawn by its group's layer, not again on its own row); a matte-source child of a group draws nothing inside it; two regression tests | fixed + tests |
| M2 | The resampled morph opens one outline and draws `LineTo`s (it emitted all-`MoveTo`s, so a morph between differing topologies vanished); assertions added to the existing test | fixed + test |
| M3 | SoftLight's dark arm uses the W3C factor `(1-Cb)` (it borrowed the light arm's `D(Cb)`); `soft_light` test pins the table | fixed + test |
| M4 | Chromatic aberration normalises by the farthest corner from the centre, not the bottom-right one | fixed |
| M5 | Bakes publish every frame through `part_path`/`publish` and unlink the stale manifest before rewriting frames (a cache hit can no longer count a half-written frame) | fixed |
| M6 | A positionless child of a positioned group rides the ancestor transform to the ancestor's origin, matching the Geneva timeline's folded placement; cross-engine regression test | fixed + test |
| E1 | `with_backdrops`' `opaque` word reads the flattener's `preserve_alpha` mark beside the stills and cutouts (a full-frame RGBA comp kept its chosen background instead of compositing clear-over-black); `alpha_clips` carried through `BuiltTimeline`; regression test through `preview_sources_of` | fixed + test |
| E2 | `Upscaler::run` waits on the device within `gpu::DEVICE_WAIT`, the bounded-wait doctrine the crate already enforces everywhere else | fixed |
| P1 | `track_subject`'s hint states the seconds-to-`0..=1` conversion for `keyframe_set` (the old hint silently collapsed every key past 1 s onto the clip's end) | fixed |
| P2 | `motion_probe`/`motion_inspect` catalogue descriptions match what the tools return (docs/MCP.md's own wording) | fixed |
| P3 | The durable idempotency ledger adopts its rows oldest-first (the file is newest-first; adopting in file order inverted the eviction ring after a restart); restart-ordering regression test | fixed + test |
| P4 | The over-long-line discard consumes bounded chunks exactly through the newline and answers before discarding (an unbounded `read_until` buffered whatever a client streamed and held the reply hostage) | fixed |
| R1 | `FillSlot` leaves detached sounds alone (a video fill doubled the picture, an audio fill deleted it); detach-then-fill regression test | fixed + test |
| R2 | `Package::resolve` holds every value to its parameter's own range and recovers non-finite to the default (the clamp `replacing` already kept for carried-over knobs), covering constants, keyed rides, audio chains and transitions | fixed + test |
| R3 | `decode_wav` refuses a fmt chunk whose bit depth it cannot scale (0 or >32 bits panicked debug builds and produced garbage scaling in release) | fixed |
| G1 | The lane waveform reads peaks in source seconds (`* clip.speed`), and the wave cache keys on speed | fixed |
| G2 | Mute writes the `muted` flag, not a volume constant a keyed clip ignores | fixed |
| G3 | `apply_within` flushes the pending inspector commit before taking the echo (a paste inside the 120 ms window dropped a released knob's edit) | fixed |
| G4 | The monitor's quality map clears on project open (every project's founding timeline is "TL1") | fixed |
| D1 | The forged-citation gate reads the reply's real `signal` key (reading `name` answered nothing, so the gate never fired) and refuses unanswered citations including the nothing-measured case; mocks re-pinned to the real shape | fixed + self-test |
| D2 | The watcher re-reads the bin after each run (it directed its own imports - the repaired take, the bed, the SFX - a second time) and records unexpected failures instead of dying | fixed |
| D3 | The kinetic-captions fallback unwraps `track_add`'s `createdId` (the reply dict always failed the branch) | fixed |
| D4 | The transcript is re-read onto the sped clip's timeline once, after the transcribe record, so captions, the planner's gaps, the duck and VERIFY-03 all work in the export's timebase; self-test assertions re-pinned | fixed + self-test |
| D5 | `slot_order` is rebuilt from the plan (a resumed run mixing reused and fresh slots crossed the transition/keyframe/removal index onto the wrong clip) | fixed |
| D6 | The bed's duck returns to unity at the last word's end, not at `at: 1.0` of a 180 s bed (the tail stayed mostly ducked); self-test pins `at < 1.0` | fixed + self-test |
| D7 | A graphic revision without a lane of its own reuses the old graphic's lane, read from the timeline before removal | fixed |
| D8 | A B-roll segment without numeric start/end is refused by name before the plan gate, not by a KeyError out of the watcher | fixed |
| I1 | `install_model` places tts bundles by the archive's own stem (`sherpa-onnx-pocket-...`, the engine's `POCKET_ID`; the row-id path meant Pocket TTS never landed and re-downloaded every rerun) and counts any `.onnx` | fixed |
| I2 | Chatterbox graphs land under `onnx/` where the engine's FILES table reads them | fixed |
| I3 | `fetch` sends the token to the fork's own release and raw URLs (models, manifests, indexes and fonts; a private fork refused them anonymously) | fixed |
| I4 | `scripts/ci/fetch.sh` aborts a stalled body (`--speed-limit/--speed-time`, curl exit 28, the retry's own case) | fixed |
| I5 | The nix link check picks the binary that exists and fails on ldd's own error (a missing binary passed vacuously) | fixed |
| I6 | `scripts/funcut-env.sh` no longer `set -euo pipefail`s the sourcing shell | fixed |
| I7 | The models workflow records digests even when a later fetch in the batch failed | fixed |
