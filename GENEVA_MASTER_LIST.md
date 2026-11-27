# Geneva master list — current verified state

> Counts and the canonical feature inventory live in
> [docs/FEATURES.md](docs/FEATURES.md). This page carries Geneva's own
> dated detail.

**Last reconciled:** 28 November 2026  
**Repository:** `coreselfmastery/FUNCUT`  
**Canonical implementation:** `src/crates/concat-motion`  
**Sidecar contract:** Geneva 1.6.0, pinned by the installer and CI dependency lock  
**Purpose:** Geneva's own current status page; the canonical whole-repo inventory is [docs/FEATURES.md](docs/FEATURES.md). The 1 November four-pillar audit remains historical evidence; its original “Findings register” and “What remains” sections are superseded by its 3 November postscript and this page.

## Executive status

Geneva is a substantial, integrated motion-rendering path, not a placeholder. The four pillars—Creative, Performance, Reliability and Agent—are implemented in the repository. The ten findings in the original 1 November register are recorded as closed in the 3 November postscript. The current Geneva workflow also exercises the real sidecar, rendered output, scene-quality floors and golden-frame comparisons.

**Do not interpret this as proof that every FUNCUT release requirement is complete.** This page is scoped to Geneva. It does not certify the separate full AI Director workflow, target-laptop performance, or the editor's general release readiness.

**Historical exact-main validation on `bd1de9d073e5ba198bffe1c837dcee806db0f777` (24 November 2026).** This commit merges PR #209. The exact-main CI run [45394932229](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932229) passed all three jobs: Format/locales/models, real-fixture analysis and Engine tests/lints. The Build app run [45394932347](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932347) passed Linux x86_64 and aarch64 builds, packaging and clean-distribution smoke tests. The Geneva run [45394932160](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932160) passed both the multi-layer rendered/inspected example and the pinned dependency-dance job. The release workflow run [45395003098](https://github.com/coreselfmastery/FUNCUT/actions/runs/45395003098) ran on this same commit for tag `v0.2.8-funcut.15`; tag validation, all three CI jobs, both Geneva jobs, both Linux package jobs, Nix, RustSec and Publish release all completed successfully. GitHub reports 18 check-runs on this SHA, all successful. This is CI and release-pipeline evidence; it is not a substitute for real target-hardware or real-project acceptance.

The branch API reports `main` protected and currently exposes these required status-check contexts with `enforcement_level=everyone`: `Format, locales and models`, `The analysis senses against real fixtures`, and `Engine tests and lints`. The detailed branch-protection endpoint was not readable through the connected GitHub access, so approval/conversation-resolution rules are not independently certified here.

## 1. Creative pillar — implemented

- [x] Motion vocabulary: named easing curves, spring curves and stepped timing.
- [x] Typography and kinetic text, including word/character staggering and karaoke timing.
- [x] Transition vocabulary: whip pan, zoom-through, inverse zoom, rack focus, velocity match and staggered exits.
- [x] Audio-driven motion with beat timing and authored-track-aware values.
- [x] Seeded procedural motion (Drift, Fluid and bounded Particles); deterministic seeds, no unseeded randomness in these render modules.
- [x] Nested compositions, scoped assets and repeated placements.
- [x] Principles lint and preset self-audit.
- [x] Storyboard/sequence planning with warnings for clamped or skipped choreography.

**Evidence:** `curve.rs`, `principles.rs`, `typography.rs`, `transition.rs`, `pulse.rs`, `noise.rs`, `sequence.rs`, `comp.rs`; detailed module-level evidence is in `audit/2026-11-01/geneva-four-pillars.md`.

## 2. Performance pillar — implemented

- [x] Content-sensitive cache identity includes comp data and referenced asset bytes.
- [x] Asset hashing streams in bounded chunks rather than loading large media files into memory.
- [x] Nested-scene/reel cache identity includes the scene inputs and nested asset content.
- [x] Cache publication uses unique temporary paths and atomic rename; manifest is the completion sentinel.
- [x] Cache budget and LRU eviction are bounded/configurable; stale interrupted work is swept.
- [x] Ranged/farm rendering uses independently staged parts and joins without re-encoding where the render can be split.
- [x] A render that cannot be safely split is refused with a diagnostic rather than silently producing a wrong result.

**Important operational truth:** a Geneva cache hit still requires the live sidecar to answer its version query. The cache avoids rendering; it is not a sidecar-free offline renderer. This is intentional and documented in the source.

**Evidence:** `geneva.rs`, `cache.rs`, `hash.rs`, `reel.rs`, `sequence.rs`, and `docs/PERFORMANCE.md`.

## 3. Reliability pillar — implemented, with known boundaries

- [x] Input/schema validation before rendering; missing assets are refused with actionable diagnostics.
- [x] Sidecar calls have bounded timeouts and a circuit breaker.
- [x] Stable diagnostic codes and fix guidance are returned to callers.
- [x] Deterministic seeded motion and repeat-render checks.
- [x] Atomic cache publication and interrupted-cache cleanup.
- [x] Scene-quality regression: ten reference scenes rendered and held to recorded quality floors.
- [x] Golden-frame regression: committed board rendered at fixed instants and compared with DSSIM; a repeated frame is checked for identical bytes.
- [x] CI asserts rendered example reports, frame dimensions/content and output existence instead of treating process exit alone as proof.

**Known boundaries, not silently “fixed”:**
- Shape gradients are reduced to a representative colour by the Geneva bridge. PR #84 is merged to `main` (merge commit `c45ac873`). Non-markup comps now route to native; mixed markup comps with native-only features are refused rather than silently degraded. True Geneva support for those mixed cases remains a separate feature.
- `absolute_position` folds static ancestor translations into child positions, including children with no own position. The follow-on `translated_position_track` path now applies the same accumulated offset to every animated child position keyframe (12 November 2026). Animated parent transforms and inherited scale/rotation still route to native because those cases require hierarchy-aware composition.
- Geneva's PNG-sequence output carries alpha. The `alpha` request field is retained in cache/manifest identity, but does not change Geneva's RGBA pixels.
- A missing sidecar can fall back to the native compositor only for content that native supports. Native fallback is not equivalent to Geneva markup/CSS rendering.
- A golden update is a reviewed change: do not refresh reference images merely to silence a failing visual regression.

## 4. Agent pillar — implemented

- [x] MCP-facing motion operations expose probing/planning/preview/bake/reel capabilities through the motion stack.
- [x] Critique output can carry validated, bounded patch operations.
- [x] The agent CLI has a `fix` path that applies valid patch operations, uses prose fallback where appropriate, skips unapplicable fixes with a reason, and reports remaining problems.
- [x] The sense/critic bridge validates patch shape and does not invent numeric patch values from unmeasured visual guesses.
- [x] Tool descriptions document the supported critique/edit/fix workflow.

**Interface distinction:** the apply-fix implementation is the agent CLI's `fix` verb, paired with the existing edit path. A separate MCP tool named `motion_fix` was deliberately not added because it would duplicate that existing door. Do not list the absence of a separately named MCP action as an unimplemented engine feature.

## 5. Original ten findings — disposition

The historical audit's 3 November postscript records all ten as closed. The source tree was rechecked on 11 November for the key implementation paths.

| # | Original finding | Current disposition |
|---:|---|---|
| 1 | Cache hit requires live sidecar | **Closed as an intentional contract**; comment documents version lookup before cache use. |
| 2 | Two definitions of bake key | **Closed**; obsolete `Comp::cache_key` removed; manifest hash + assets comparison is authoritative. |
| 3 | Ranged bakes shared a staging path | **Closed**; per-writer unique `cache::part_path`. |
| 4 | Explicit scene `at` bypassed overlap warning | **Closed**; plan warns when authored placement defeats a requested cross; preserves the author's placement. |
| 5 | Sequence duration disagreed with choreography | **Closed**; layout and plan use the same overlap arithmetic. |
| 6 | Single-frame render leaked scratch directory | **Closed**; frame is copied to output/deterministic temp destination and scratch is cleaned. |
| 7 | Geneva alpha flag looked like a rendering promise | **Closed as documentation/contract clarification**; Geneva always emits RGBA PNG frames. |
| 8 | Retired-directory naming could collide/mangle paths | **Closed**; publish path uses unique temporary part + rename; no retired directory scheme. |
| 9 | Interrupted bake directories escaped eviction | **Closed**; stale no-manifest work and stale part files are swept; recent in-flight work is protected. |
| 10 | Sidecar discovery ignored executable permission | **Closed**; discovery selects runnable candidates and has a regression test. |

The detailed rationale and regression-test names are retained in the historical audit's “Postscript — the register, closed: 3 November 2026”. If a future code change reopens one of these behaviours, reopen the row and add a regression test; do not erase the history.

## 6. CI and acceptance evidence

The canonical workflow is `.github/workflows/geneva.yml`. It has two jobs:

1. **The multi-layer example, rendered and inspected:** installs the pinned sidecar, runs `cargo test -p concat-motion`, plans and renders the multi-layer reel, asserts the report, inspects frames/styles, runs the ten-scene quality regression and runs the golden-frame DSSIM suite.
2. **The dependency dance, pinned and green:** provisions pinned native dependencies, tests `concat-motion`, `concat-core` and `concat-mcp`, then renders the example under the full dependency environment.

**Observed evidence at this reconciliation:**
- PR #81's five required checks all completed successfully on head `1035fb68`; PR #81 merged as `66cc89d2`.
- PR #83's five required checks all completed successfully on head `9fc16e79`; PR #83 merged as `e3114ca8` after its branch was reconciled with the then-current `main`.
- PR #84 merged as `c45ac873`. Its main-commit Geneva rendered-example and dependency-dance jobs both passed in run `42979489279`. Both Linux x86_64 and aarch64 build jobs passed. The general Rust engine/clippy job remained in progress at the last poll; verify it before calling the whole merged commit fully green.
- PR #85 passed all five CI checks on head `d8877703` and merged as `cf82462c`; its selection-time refusal explanation and regression test are now on `main`.
- PR #80 passed all five PR CI checks and merged as `dce1a750`; its native still-image bake-cache fix is on `main`.
- PR #86 passed all five PR CI checks, including the hosted 1080p30 benchmark smoke and workspace clippy, then merged as `cc48ce81`. The installer refuses an unverified sidecar when no SHA-256 tool is available; hosted benchmark correctness is not antiX laptop performance evidence.
- PR #88 merged to `main` as `56bde7c85e9fedc817092e2fdad59a3c19aeb09f`: per-timeline zoom, horizontal/vertical scroll, tool, snap, lane sizing, playhead and clip selection are cached/restored, with switching/removal regression tests. The exact merge commit is validated separately below: main CI passed 3/3 jobs, Geneva passed 2/2 jobs, and Build app passed Linux x86_64 and aarch64.
- PR #80 passed all five PR CI checks and merged as `dce1a750`; native still-image pixels are cached across bake frames, with a regression test proving the cache avoids rereading the source.
- PR #81 and PR #83 are historical merged prerequisites. The current main baseline is the later PR #88 merge `56bde7c85e9fedc817092e2fdad59a3c19aeb09f`; fresh checks on that exact merge commit remain a release-gate item.

**12 November 2026 follow-up refresh (supersedes the older main snapshots above):** `main` is `cbcf821e465c265aef26071be3350fe239619aaf`. Geneva passed both jobs in run `43236482716`; Build app passed Linux x86_64 and aarch64 in run `43236482829`, including package and clean-distribution smoke tests; Nix passed `nix build` and binary-link validation in run `43236482693`. General CI run `43236482894`: Format/locales/models and real-fixture analysis passed; workspace Clippy passed, but test-binary compilation was still running at the last poll, with unit tests, documentation examples and gRPC checks not yet started. PR #101 merged as this main commit and corrected the Nix swap threshold and codegen-unit setting after the earlier Nix failure on `a29f44ad`. The current main commit's release workflow has not run: the release tracker subsequently records `v0.2.8-funcut.12` at `6f9ab45fb0916f44675f10769351d911e0e7f75b`; current `main` is 27 commits ahead, so that release does not certify the current main tree. The release asset listing could not be independently verified through the connected tools. Treat earlier SHA/check links as historical evidence only.



**12 November 2026 follow-on implementation:** after PR #99, `main` advanced through the proxy-scrub and proxy-codec improvements and the Geneva follow-on commit `7497922b93dbff68c9cd4fef04ee7f13b4364023`, which offsets animated child position keyframes by their static ancestor translations. This closes that specific flattened-timeline gap. Animated parent transforms and inherited scale/rotation remain unsupported. The older CI snapshot above is evidence for its recorded SHA only; it is not a validation claim for these later commits.

**17 November 2026 historical follow-up:** PR #166's head `154f8f802211177e625e140026735819ff2ac33e` completed the full CI and Geneva workflows; the exact-main evidence for that snapshot is retained above as historical validation. It is superseded by the 18 November current-main runs and must not be used to claim the current dependency setup is green.

**18–19 November 2026 follow-up:** PR #167 merged as `08c77063c361a8e91a0b83b0897ed8a211c8ecd4`, then PR #169 merged as `f13305fc1931e88d79bafd4112118fd0f9b8c19c`. PR #169's head passed all three CI jobs, both Geneva jobs, and both Linux package architectures (runs [44364031838](https://github.com/coreselfmastery/FUNCUT/actions/runs/44364031838), [44364031575](https://github.com/coreselfmastery/FUNCUT/actions/runs/44364031575), [44364031794](https://github.com/coreselfmastery/FUNCUT/actions/runs/44364031794)). The 1.28.0 HTTP 404 is resolved in the merged dependency pins. Exact-main push-triggered runs on merge SHA `f13305fc` were not retrievable through the connected workflow interface, so do not call the merge SHA independently validated.

**19–20 November 2026 follow-up:** PR #170 merged as `e54e767562c7c0c963436be95149feb4d6a74e0f`, carrying full-result-page stock selection, measured-duration refusal, per-take footage planning, explicit no-B-roll handling, and preview/revise before export. PR #172 (the AUTO EDIT Director handoff and voice-signal evidence) and PR #173 (the `sherpa-onnx` 2.x migration that clears the RustSec gate) have both since merged, along with #175 and #176. The old main's seven check-runs were six successes and one real-fixture analysis cancellation at `timeout-minutes: 15`; PR #176 raised that limit to 30 minutes, and the job has passed in under two and a half minutes on PR heads since.

## 7. Remaining work — Geneva scope

No item from the original ten-finding register is currently listed as open. The following are the remaining validation/roadmap boundaries, not claims that the closed findings are still broken:

- [x] Historical 12 November baseline `cbcf821e465c265aef26071be3350fe239619aaf`: Geneva, Nix and Linux x86_64/aarch64 packaging passed on that exact SHA; general CI's format/locales/models and real-fixture jobs passed, with engine tests still running at the last poll. This is historical evidence, not current-main validation.
- [x] Historical 23 November snapshot (`fa68c059`): superseded by the exact-main reconciliations in Sections 11–12. Its target-machine and real Director acceptance gaps remain open.
- [x] Historical scene-quality and golden-frame gates passed on the 12 November baseline in Geneva run `43236482716`; keep these mandatory on every relevant future change and review any intentional golden/quality-floor update.
- [x] PR #169's head passed both Geneva jobs, including dependency-dance, rendered examples, scene-quality floors and golden-frame DSSIM in [run 44364031575](https://github.com/coreselfmastery/FUNCUT/actions/runs/44364031575); exact-main push validation remains unverified.
- [ ] Run and record representative timing/memory measurements on the target low-memory Linux machine for realistic 1080p motion workloads. PR #86 is merged: the reproducible cold 1080p30 Geneva benchmark records machine, sidecar version, wall time, output size and sampled process-tree RSS. Hosted CI proves correctness, not target-device speed; a real antiX run remains required.
- [x] Mixed markup comps that also use native-only shape gradients, mattes, effects, paths or masks are refused by Geneva bake/frame instead of being silently degraded; PR #84 shipped the safe refusal and native fallback for non-markup comps. PR #85 added the selection-time explanation and regression test; it merged as `cf82462c`.
- [ ] Optional future feature: implement true Geneva rendering support for those mixed markup/native-only cases. This is a feature-parity enhancement, not a silent-degradation blocker while the refusal remains in place.
- [ ] Decide whether mixed markup needs full inherited transform parity; static ancestor translations and animated child position keyframes under those static translations are now folded into child positions (PR #92 and follow-on commit `7497922b`). Animated parent transforms and inherited scale/rotation still require native fallback or future Geneva composition work. The native fallback cannot be used for HTML/CSS comps.
- [x] PR #84 makes `needs_markup()` recursive so nested HTML sub-comps do not incorrectly fall back to native when the sidecar is missing; regression coverage is included.
- [ ] If offline cache use without the sidecar is desired, change the version/key contract deliberately and test compatibility; current behaviour intentionally requires the sidecar.
- [ ] Keep Geneva 1.6.0 pin, checksum, installer, package contents and supported platform binaries aligned whenever the sidecar is upgraded.
- [x] PR #86 merged as `cc48ce81`: the optional sidecar install fails closed when neither `sha256sum` nor `shasum` is available, and CI checks installer syntax and pin parity.

## 8. Separate FUNCUT work — not Geneva blockers

These must remain on the broader FUNCUT release tracker, not be misreported as Geneva defects:

- Native Lottie/dotLottie playback — PR #91 merged as `4a65a7b0`; bounded JSON/dotLottie playback and explicit unsupported-feature diagnostics are implemented. This is not full After Effects/Bodymovin parity.
- Timeline Motion clips — PR #97 merged as `ea589c73`; the Motion page and validated AddMotionClip command are on main. The PR reports metadata preservation, undo/redo, save/reopen regression coverage, preview and final-export bake paths. Real-project acceptance should still be included in the broader AI Director/export validation.
- Per-timeline view-state isolation — PR #88 merged as `56bde7c8`.
- Export queue and explicit timeline picker — PR #96 merged as `30a91b06`.
- Linux VAAPI encode integration — PR #95 merged as `48cdcd89`; real Intel/AMD hardware validation remains outstanding.
- 1080p proxy performance measurements on the actual antiX laptop, including H.264 versus MJPEG tradeoffs.
- Full AI Director brief-to-final-export acceptance.

## 9. Definition of Geneva-ready

Geneva is ready for a release candidate only when the following evidence is current on the same main commit:

- [x] Historical 12 November baseline `cbcf821e465c265aef26071be3350fe239619aaf`: Geneva example/dependency-dance, Nix and Linux x86_64/aarch64 package smoke checks passed on that exact SHA. General CI format/locales/models and real-fixture jobs passed; engine tests were still in progress at that historical poll.
- [x] Historical 23 November snapshot (`fa68c059`): superseded by the exact-main reconciliations in Sections 11–12. Its target-machine and real Director acceptance gaps remain open.
- [x] Historical Nix/RustSec snapshot on `c8505e02` is superseded by the 24 November release-run evidence in Section 11. The latest audit recorded 11 allowed unmaintained/yanked warnings; the current-main exact-SHA coverage is updated in Section 12.
- [x] Historical scene-quality and golden-frame checks passed on the 12 November baseline in Geneva run [43236482716](https://github.com/coreselfmastery/FUNCUT/actions/runs/43236482716).
- [x] PR #169's head scene-quality and golden-frame reports are retained in Geneva [run 44364031575](https://github.com/coreselfmastery/FUNCUT/actions/runs/44364031575); these are PR-head reports, not exact-main push evidence.
- [x] Mixed markup comps that contain native-only features are explicitly refused with a useful diagnostic; PR #85's selection-time explanation and regression test merged as `cf82462c`.
- [x] The pinned sidecar installed and was discovered in the Linux x86_64 and aarch64 package builds on the historical 12 November baseline (run [43236482829](https://github.com/coreselfmastery/FUNCUT/actions/runs/43236482829)); these artifacts do not validate the current main SHA.
- [ ] Target-machine timing/memory evidence is attached, or release notes clearly state that only hosted correctness—not target-device performance—has been validated.
- [x] The `v0.2.8-funcut.14` tag is pushed at `7ef266f` (2026-11-23) and its release workflow passed the required CI lanes before the package jobs; `.13` remains the last published release until those jobs finish. `main` now carries the native CPU 3D scene layer (`61057bb`), so `.15` is the complete latest tree. Target-hardware acceptance and a real Director project run remain open, and the release notes say so.

## 10. Exact-main reconciliation — 23 November 2026

The original ten Geneva audit findings remain closed; the items above are validation and roadmap boundaries, not silently reopened defects.

| Gate | Current evidence | Status |
| --- | --- | --- |
| Creative / rendered examples | PR #200 head Geneva run [45036672897](https://github.com/coreselfmastery/FUNCUT/actions/runs/45036672897) and exact-main Geneva run [45047192665](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192665); the exact-main multi-layer example and dependency-dance jobs both passed. | CI-verified on main |
| Format, locales and models | Exact-main CI run [45047192734](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192734), job passed. | CI-verified on main |
| Real-fixture analysis | Exact-main CI run [45047192734](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192734), job passed, including the Auto Director safety self-test and analysis fixtures. | CI-verified on main |
| Engine tests and lints | Exact-main CI run [45047192734](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192734): 2,246 workspace tests passed, 13 skipped and one slow; gRPC tests 30/30 passed; workspace and gRPC Clippy, documentation examples and generated-plan schema checks passed. | CI-verified on main |
| Linux x86_64 / ARM64 packages | Exact-main Build app run [45047192666](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192666), both architecture jobs passed. | CI-verified on main |
| Nix package | Latest successful Nix run [45007325513](https://github.com/coreselfmastery/FUNCUT/actions/runs/45007325513) used `c8505e021f72824bfe5f6a08dda141522b2db690`, not the current merge SHA. PR #202 runs Nix on updated head `53213b2dcec42ab323ef6f87ea14fab4aac36c71` in [run 45053366736](https://github.com/coreselfmastery/FUNCUT/actions/runs/45053366736); it is still in progress. | Not verified on current main yet |
| RustSec | Latest successful audit [45007325573](https://github.com/coreselfmastery/FUNCUT/actions/runs/45007325573) used `c8505e021f72824bfe5f6a08dda141522b2db690` and had 11 allowed maintenance/yank warnings. Fresh audit [45052190027](https://github.com/coreselfmastery/FUNCUT/actions/runs/45052190027) on the prior PR #202 head failed on `RUSTSEC-2026-0592` (`url 2.5.8`, patched in `2.5.9`). Updated head `53213b2dcec42ab323ef6f87ea14fab4aac36c71` passed RustSec in [run 45053366735](https://github.com/coreselfmastery/FUNCUT/actions/runs/45053366735); 11 allowed warnings remain (nine unmaintained crates, two yanked versions). The fix is not yet merged or validated on main. | PR-head audit passes; main still blocked pending merge and exact-main run |
| Target hardware | No real antiX/low-memory-machine Geneva timing or memory run is attached. | Not verified |
| 1080p long-GOP proxy scrubbing | Source implements desktop proxies on by default, scrub proxies on by default, MJPEG as default codec, a 250 ms scrub linger and an original-frame refresh after a paused scrub. ReaderPool unit tests cover proxy selection/fallback; no measured interactive 1080p long-GOP benchmark on the target machine is recorded. | Implemented and unit-tested; product performance not verified |
| Full AI Director acceptance | PR #200 adds the creative contract and independent acceptance; its PR-head CI passed all seven checks. The real-fixture safety self-test is not a real brief-to-export project on the target machine. | Implemented and CI-tested; E4 product acceptance not verified |

**Historical merge-control finding (23 November):** that API snapshot reported `enforcement_level=off` and no required contexts. The later 24 November branch response, retained in the executive status above and Section 12, reports `main` protected with three required contexts and enforcement level `everyone`. The detailed protection endpoint returns 403 through this connection, so approval, admin, conversation-resolution and force-push/delete rules are not independently confirmed here.

**Historical release finding (before `.15`):** `.13` was then the latest verified published tag. Section 11 records the later `.15` publication; target-machine and real-project acceptance remain open.
**Status rule:** never mark the whole FUNCUT product “ready” from this Geneva checklist alone. This page certifies only the Geneva motion-rendering subsystem and its stated boundaries.


## 11. Exact-main reconciliation — 24 November 2026

This is the 24 November snapshot. Section 12 is the newer exact-main reconciliation; earlier SHAs and run links remain historical evidence only.

| Gate | Exact evidence on `bd1de9d073e5ba198bffe1c837dcee806db0f777` | Status |
| --- | --- | --- |
| Geneva sidecar, motion tests and dependency environment | Geneva run [45394932160](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932160), both jobs passed. | CI-verified on main |
| Rendered multi-layer example, scene-quality floors and golden-frame DSSIM | The “multi-layer example, rendered and inspected” job in [45394932160](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932160) passed all steps. | CI-verified on main |
| Format, locales, models and signal self-tests | CI run [45394932229](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932229), job passed. | CI-verified on main |
| Real-fixture analysis and Director safety self-test | CI run [45394932229](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932229), job passed. This is fixture/self-test coverage, not a real Director production. | CI-verified on main |
| Workspace tests, Clippy, docs and gRPC | Engine job in [45394932229](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932229) completed successfully, including workspace tests, workspace/gRPC Clippy, doctests, plan-schema check and gRPC tests. | CI-verified on main |
| Linux x86_64 and aarch64 packages | Build app run [45394932347](https://github.com/coreselfmastery/FUNCUT/actions/runs/45394932347), both architecture jobs passed package and clean-distribution smoke steps. | CI-verified on main |
| Nix package and binary links | Release run [45395003098](https://github.com/coreselfmastery/FUNCUT/actions/runs/45395003098), Nix job passed on the same SHA. | CI-verified on main |
| RustSec dependency audit | Release run [45395003098](https://github.com/coreselfmastery/FUNCUT/actions/runs/45395003098), audit step passed on the same SHA. | CI-verified on main |
| Release assembly | Release run [45395003098](https://github.com/coreselfmastery/FUNCUT/actions/runs/45395003098), manifest, checksums, desktop-entry check, notes and publish steps passed. The published `v0.2.8-funcut.15` release lists ten assets. | CI-verified / published |
| Required main checks | Branch API exposes the three required contexts listed above with enforcement level `everyone`. | API-verified |
| Target-machine timing and memory | No real antiX/laptop Geneva timing or memory capture is attached. Hosted render benchmarks do not establish target-device speed. | Not verified |
| 1080p long-GOP proxy scrubbing | Proxy defaults and the 250 ms pause/original-frame restoration path are implemented and unit-tested; no measured interactive target-laptop before/after result is attached. | Implemented and unit-tested; product performance not verified |
| Full AI Director production | Real-media brief/script through footage, motion, sound, editable timeline and final export still needs an end-to-end acceptance run. | Not verified |
| Motion-clip editing acceptance | Metadata edit, save/reopen, in-page preview and final-export round-trip acceptance remain open in `KNOWN_ISSUES.md`. | Not verified |

The Geneva implementation boundaries remain unchanged: mixed markup/native-only cases are refused rather than silently degraded; animated parent transforms and inherited scale/rotation are not fully represented by the Geneva bridge; the sidecar is still required for its version/cache contract. These are documented limitations, not silently completed features. Keep the native fallback/refusal behaviour and its tests intact.

## 12. Exact-main reconciliation — 25 November 2026

**Current main:** `9a6d7874d1dda0af0557296b6732b87a1bd8a25b` (merge of PR #220, head `f08aa4e29fa9a4ba973e0d4a2d30029d8156c0cf`). This section supersedes current-main and release-status claims in Sections 7, 9, 10 and 11. The statuses below are the observed state at this reconciliation; in-progress jobs are not green.

| Gate | Exact evidence on `9a6d7874` | Status |
| --- | --- | --- |
| Format, locales and model inventory | CI run [45665365086](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365086), job `Format, locales and models` passed. Log: locale check passed with 4,251 untranslated English-equivalent lines warned (not failed); `scripts/models.py --check` reports 51 models / 5.06 GB; the visual-sense self-test and release-gate checks passed. | CI-verified on main |
| Real-fixture analysis and Director safety | CI run [45665365086](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365086), real-fixture job passed. The `auto_director.py --self-test` rendered synthetic projects and exercised refusal, retry/revision and speech-verification scenarios; it is not a real user project with configured external providers. | CI-verified; E4 acceptance not verified |
| Engine tests and lints | First attempt [job 139385976608](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365086/job/139385976608) ended when the hosted runner shut down (exit 143) during test-binary build; no Rust compiler or test failure was reported. Retry [job 139410444366](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365086/job/139410444366) is active at workspace Clippy; test build, workspace tests, doctests, plan-schema and gRPC checks remain pending. | Retry in progress; not green |
| Geneva sidecar, motion tests and dependency environment | Geneva run [45665365144](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365144), both jobs passed. | CI-verified on main |
| Rendered example, scene-quality and golden frames | The Geneva multi-layer job in [45665365144](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365144) rendered and inspected the example; all golden frames held at DSSIM 0.00000. Its cold hosted 1080p30 benchmark reported 38.546 s render time, 760.3 MiB sampled process-tree RSS and a 4,337,591-byte output on GitHub's Ubuntu runner. This is a renderer benchmark, not an interactive antiX scrubbing benchmark. | CI-verified on hosted runner |
| Linux aarch64 package | Build app run [45665365184](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365184), ARM64 job passed build, package creation and clean-distribution `concat --version` / `concat --smoke` checks. | CI-verified on main |
| Linux x86_64 package | Build app job [139385977733](https://github.com/coreselfmastery/FUNCUT/actions/runs/45665365184/job/139385977733) completed build, Geneva sidecar, staging, smoke test, x86_64 instruction-set scan, Arch/Linux package creation and clean-distribution smoke. | CI-verified on main |
| Nix package | No Nix run was triggered for this main push because `nix.yml` is path-filtered and the merge did not touch its watched inputs. The latest exact-main release evidence is the 24 November `.15` release run [45395003098](https://github.com/coreselfmastery/FUNCUT/actions/runs/45395003098) on the preceding main SHA, not `9a6d7874`. | Not re-run on current SHA |
| RustSec dependency audit | No audit run was triggered for this main push because `security-audit.yml` is path-filtered. The 24 November `.15` release run [45395003098](https://github.com/coreselfmastery/FUNCUT/actions/runs/45395003098) passed on the preceding main SHA. Its prior standalone audit reported 11 allowed unmaintained/yanked warnings; it did not suppress vulnerability findings. | Not re-run on current SHA; warnings remain triaged |
| App release | Latest published app release is [`v0.2.8-funcut.15`](https://github.com/coreselfmastery/FUNCUT/releases/tag/v0.2.8-funcut.15), published 23 November with ten assets. The Release workflow is tag-triggered; no app release has been built/published for `9a6d7874`. | Published release is older than current main |
| Model mirror | Manual `Mirror models` run [45644419485](https://github.com/coreselfmastery/FUNCUT/actions/runs/45644419485) passed on branch head `284fb0f0`; `models-v1` contains 51 uploaded assets, all with SHA-256 digests (5,061,118,164 bytes total). The current main format job independently checks the 51-row manifest. | CI-verified mirror; current manifest checked |
| Required main checks | The branch API reports `main` protected and exposes the three required contexts: Format/locales/models, real-fixture analysis and Engine tests/lints, with enforcement level `everyone`. The detailed protection endpoint returned 403 to this connection, so approval/conversation-resolution settings were not independently re-read. | Partially API-verified |
| Geneva boundaries | Mixed markup/native-only cases remain explicitly refused rather than silently degraded; animated parent transforms and inherited scale/rotation are not fully represented by the bridge; the live sidecar version query remains part of the cache contract. | Implemented guardrails; feature boundaries remain |
| Target laptop / 1080p long-GOP | Proxy defaults, MJPEG proxy codec, pause linger and original-frame restoration are implemented and unit-tested. No measured interactive long-GOP before/after run on the actual antiX laptop is attached. | Product acceptance not verified |
| Full AI Director production | The Director skill now includes `sense_visual`, `sense_sound` and `sense_sound_model` planning/measurement workflows, with 117 catalogue tools / 109 served and signal-consumer guards. CI synthetic safety tests pass, but no real brief/script + voiceover + providers + footage + motion + audio + editable timeline + inspected export acceptance is recorded. | Integrated/source-tested; E4 not verified |
| Motion-clip round trip | AddMotionClip and the Motion page are implemented; real-project metadata edit, save/reopen, preview and final-export acceptance remain open. | Not verified |
| Hardware acceleration | NVDEC/VAAPI/NVENC selection and fallback code exist; actual Intel/AMD/NVIDIA hardware behavior and performance remain unverified. | Not verified |
| Model provenance | `models-v1` mirror assets are digest-pinned; 26 of the 30 visual-shelf entries remain operator-sourced and are pinned on first download rather than having an independently verified upstream digest. | Partially verified; provenance follow-up open |

**Geneva scope conclusion:** the Geneva CI/render path is green on this exact main SHA. That does not close target-machine performance, complete AI Director production, motion-clip round-trip or hardware acceptance. Do not mark FUNCUT release-ready until those E4 gates have evidence.

## 13. Exact-main reconciliation — 28 November 2026

This is the current review of `main` as of 28 November. It supersedes the
current-main status in earlier sections; their older SHAs and links remain
historical evidence. The main commit itself is dated 26 November.

**Current main:** `da544297c4a1f33f8e29defa0301ad0ee9f011e5` (merge of PR #243).
Exact-main push workflows: CI [45895570099](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570099),
Geneva [45895570075](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570075),
Build app [45895570052](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570052).
All seven check-runs completed successfully on this SHA.

| Gate | Exact evidence on `da544297` | Status |
| --- | --- | --- |
| Format, locales and model inventory | CI run [45895570099](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570099): `cargo fmt --check` passed; locale check passed with 4,256 English-identical lines warned (not failed); `scripts/models.py --check` reported 51 models / 5.06 GB on `models-v1`. | CI-verified on main |
| Signal matrix and sense self-tests | The same CI job passed the matrix and sense self-tests. It reports 76 sense readings and 67 requested by AUTO EDIT (88%); nine are not requested by the spine. **Important:** current main invokes `scripts/check-signal-coverage.py` without `--require-button`, so the job enforces catalogue/matrix drift but does not fail on those nine unrequested actions. PR #246 proposes the stricter `--require-button` ledger; that PR is not merged as of this reconciliation. | CI-verified for matrix/self-tests; full button-consumption gate not enforced on main |
| Real-fixture analysis | CI job [140168493501](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570099/job/140168493501) passed. The synthetic Director acceptance self-test recorded 14/14 verification checks, 17 plan actions executed and two skipped, two revision passes and three renders. It deliberately exercised a low-confidence heuristic critic and recorded warnings. This is synthetic fixture/safety evidence, not a real user's brief-to-export production. | CI-verified fixtures; real-project acceptance not verified |
| Engine tests and lints | CI job [140168493574](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570099/job/140168493574) completed 2,368 workspace tests passed, 13 skipped, one slow; workspace/gRPC Clippy, docs/schema checks passed; gRPC tests 30/30 passed. | CI-verified on main |
| Geneva motion stack and dependency environment | Geneva run [45895570075](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570075), both jobs passed: 640 motion-stack tests and 641 in the pinned dependency environment, each with five ignored tests. | CI-verified on main |
| Geneva rendered example and golden frames | The multi-layer rendered/inspected job in [45895570075](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570075) produced a 4,337,591-byte output; all golden frames held at worst DSSIM 0.00000. The cold hosted 1080p30 render measured 28.833 seconds and 759.7 MiB sampled process-tree RSS. This is hosted rendering evidence, not interactive target-laptop scrubbing. | CI-verified on hosted runner |
| Linux x86_64 and aarch64 packages | Build app run [45895570052](https://github.com/coreselfmastery/FUNCUT/actions/runs/45895570052): both architecture jobs passed build, package and clean-distribution smoke checks. | CI-verified on main |
| Nix package | No Nix run was triggered for this main SHA because `nix.yml` is path-filtered and the merge did not change its watched inputs. PR #246 changes the Nix workflow and its Nix run is separate PR-head evidence, not current-main evidence. | Not re-run on current main |
| RustSec | No dependency-audit run was triggered for this main SHA because `security-audit.yml` is path-filtered. PR #246's audit is PR-head evidence only. The last published-release audit is on an older SHA. | Not re-run on current main |
| Model mirror | The last manual mirror run [45644419485](https://github.com/coreselfmastery/FUNCUT/actions/runs/45644419485) passed on branch head `284fb0f0`, publishing 51 digest-pinned assets to `models-v1`; current main independently checks the 51-row manifest. | Mirror CI-verified on its recorded branch; manifest CI-verified on main |
| App release | Latest published release remains [`v0.2.8-funcut.15`](https://github.com/coreselfmastery/FUNCUT/releases/tag/v0.2.8-funcut.15), on the older SHA `bd1de9d073e5ba198bffe1c837dcee806db0f777`. No release/tag artifacts have been published for `da544297`. | Published release is older than current main |
| Required main checks | The branch API reports `main` protected with the three required contexts: Format/locales/models, real-fixture analysis and Engine tests/lints, enforcement level `everyone`. The detailed branch-protection endpoint remains unavailable to the connected integration (403), so approval/admin/conversation-resolution rules are not independently certified. | Partially API-verified |

**Separate open work as of this reconciliation:** PR #246 (`audit/all-signals-2026-11-27`) is the only open PR at the time of review. Its original head `67148b6b` has been updated during this run with a licensing-safe default for the new headless `auto_edit` tool; the resulting checks are not yet complete. Its Geneva/Nix/security and other PR-head results must not be substituted for exact-main evidence. The PR changes many files and has no formal review submission; keep it open until its complete checks, patch review and merge requirements are satisfied.

**Separate FUNCUT release requirements still open:**

- No real Forge AI Director production has yet demonstrated a real narration/script and media project through asset selection, motion, captions, music/SFX, editable timeline landing, independent acceptance and final export without manual recovery.
- No interactive 1080p long-GOP scrubbing benchmark on the target antiX laptop is attached. Proxy defaults, MJPEG proxy selection and paused original-frame restoration are implemented and unit-tested, but actual proxy generation, seek latency, pause restoration, memory and sustained playback remain unverified. Do not claim a speedup without repeatable measurements.
- Motion-clip command/page implementation exists; real-project metadata editing, save/reopen, in-page preview and final-export round-trip acceptance remain open.
- Per-clip stock licensing/provenance, applicable person/property releases and provider-terms checks remain incomplete for monetized publishing.
- VAAPI/NVDEC behavior and fallback still need physical Intel/AMD/NVIDIA hardware validation.
- Native Lottie/dotLottie support is bounded playback with explicit unsupported-feature diagnostics, not full After Effects/Bodymovin parity.

The original ten Geneva findings remain closed. The documented Geneva boundaries in Section 3 remain in force: mixed markup/native-only cases are refused rather than silently degraded, animated parent transforms and inherited scale/rotation are not fully represented by the Geneva bridge, and the pinned sidecar remains required for its version/cache contract. This section does not certify overall FUNCUT release readiness.

### Same-day postscript — PR #246 merged

Later on 28 November, PR #246 (`audit/all-signals-2026-11-27`) merged as
`13295fa3` ("Merge pull request #246 from coreselfmastery/audit/all-signals-2026-11-27").
The `da544297` evidence above remains the last completed exact-main push
record; the merge commit's own push workflows were still running when this
was written. The merged PR-head checks on `ce495ed6` (whose tree is identical
to the merge, since `main` had not moved past the PR's base) all completed:
CI run [46097714534](https://github.com/coreselfmastery/FUNCUT/actions/runs/46097714534)
passed `Format, locales and models`, the real-fixture analysis and
`Engine tests and lints`; Geneva run [46097714538](https://github.com/coreselfmastery/FUNCUT/actions/runs/46097714538),
Build app run [46097714557](https://github.com/coreselfmastery/FUNCUT/actions/runs/46097714557)
and the dependency security audit run [46097714550](https://github.com/coreselfmastery/FUNCUT/actions/runs/46097714550)
all passed. The branch's Nix run stayed queued and is not a required check.
An independent machine verification on the same tree (format, check, clippy,
263 crate tests, coverage, matrix, self-test and plan-schema) also passed
before the merge.

Two consequences for the rows above:

- The signal ledger is now enforced on `main`: `scripts/check-signal-coverage.py`
  runs with `--require-button`, and the merged main reports 76 sense readings,
  67 asked by the AUTO EDIT spine, 9 reasoned not applicable in the ledger's
  `NOT_APPLICABLE` table, covered 76/76, missing 0. The "full
  button-consumption gate not enforced on main" status in the table above
  applied to `da544297` and is superseded by the merge.
- The CI workflow now detects documentation-only pull requests and skips its
  two heavy lanes for them, so a documentation change no longer pays the
  engine lane's price. This pull request is the first to ride that path.

