# Geneva master list — current verified state

> Counts and the canonical feature inventory live in
> [docs/FEATURES.md](docs/FEATURES.md). This page carries Geneva's own
> dated detail.

**Last reconciled:** 23 November 2026  
**Repository:** `coreselfmastery/FUNCUT`  
**Canonical implementation:** `src/crates/concat-motion`  
**Sidecar contract:** Geneva 1.6.0, pinned by the installer and CI dependency lock  
**Purpose:** Geneva's own current status page; the canonical whole-repo inventory is [docs/FEATURES.md](docs/FEATURES.md). The 1 November four-pillar audit remains historical evidence; its original “Findings register” and “What remains” sections are superseded by its 3 November postscript and this page.

## Executive status

Geneva is a substantial, integrated motion-rendering path, not a placeholder. The four pillars—Creative, Performance, Reliability and Agent—are implemented in the repository. The ten findings in the original 1 November register are recorded as closed in the 3 November postscript. The current Geneva workflow also exercises the real sidecar, rendered output, scene-quality floors and golden-frame comparisons.

**Do not interpret this as proof that every FUNCUT release requirement is complete.** This page is scoped to Geneva. It does not certify the separate full AI Director workflow, target-laptop performance, or the editor's general release readiness. The latest observed `main` commit is `fa68c059945e0eef9151d0fd2010134f41620c05`, which merged PR #200 on 22 November 2026. PR #200's head `84f553d09a8a7885f43f8395a4643123bf2a3ff0` passed all seven PR-head check-runs (CI 3/3, Geneva 2/2 and Build app 2/2); the PR changed 85 files and had no formal review submissions or inline review threads. All seven CI/Geneva/Build-app check-runs on the exact merge SHA have now also completed successfully; Nix and RustSec remain on a previous SHA.

**Exact-main validation on `fa68c059945e0eef9151d0fd2010134f41620c05` completed successfully for the workflows triggered by this source change.** CI run [45047192734](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192734) passed all three jobs: Format/locales/models, real-fixture analysis and Engine tests/lints. The engine log records 2,246 workspace tests passed, 13 skipped and one slow; the gRPC suite passed 30/30; workspace and gRPC Clippy, documentation examples and generated-plan schema checks passed. Geneva run [45047192665](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192665) passed both jobs, and Build app run [45047192666](https://github.com/coreselfmastery/FUNCUT/actions/runs/45047192666) passed Linux x86_64 and aarch64. The earlier Nix and RustSec successes are on `c8505e021f72824bfe5f6a08dda141522b2db690`, not this merge SHA. A fresh RustSec run on the previous PR #202 head found `RUSTSEC-2026-0592` in `url 2.5.8`. Updated PR #202 head `53213b2dcec42ab323ef6f87ea14fab4aac36c71` upgrades `url` to `2.5.9` and its compatible `form_urlencoded` lock to `1.2.3`. RustSec passes on that head in [run 45053366735](https://github.com/coreselfmastery/FUNCUT/actions/runs/45053366735), with 11 allowed warnings (nine unmaintained crates and two yanked versions); Format/locales/models and real-fixture analysis pass in [run 45053366739](https://github.com/coreselfmastery/FUNCUT/actions/runs/45053366739). Engine tests, both Geneva jobs, both Linux package jobs and Nix remain in progress on the updated head. These are PR-head checks, not current-main evidence.

GitHub reports `main` as `protected=true`, but the branch API currently reports `required_status_checks.enforcement_level=off` and an empty `contexts` list: no status checks are enforced by that branch-protection object. The direct protection-details request returned HTTP 403, so required-review rules remain unverified. PR #200 had no formal review submissions; its PR-head checks did pass before merge.

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
- [ ] Current main `fa68c059945e0eef9151d0fd2010134f41620c05` is 249 commits ahead of the published `.13` tag. All seven exact-main CI/Geneva/Build-app check-runs passed; Nix and RustSec did not run on this SHA. PR #202's updated head `53213b2dcec42ab323ef6f87ea14fab4aac36c71` fixes the `url 2.5.8` advisory and its RustSec audit passes, but Nix and the rest of that PR's matrix are still running. Target-machine proxy/Geneva plus real Director acceptance remain open. See the exact-main reconciliation below.
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
- [ ] Current main `fa68c059945e0eef9151d0fd2010134f41620c05` is not yet release-ready. Its seven triggered CI/Geneva/Build-app check-runs are green, but Nix and RustSec are not validated on this SHA. PR #202's updated head `53213b2dcec42ab323ef6f87ea14fab4aac36c71` now passes RustSec after upgrading `url` and `form_urlencoded`; Nix and the remaining CI/Geneva/Build jobs are still in progress. Target-hardware acceptance and a real Director project run remain open.
- [ ] Nix's latest success is on `c8505e021f72824bfe5f6a08dda141522b2db690` (run [45007325513](https://github.com/coreselfmastery/FUNCUT/actions/runs/45007325513), 91m35s), not the current `fa68c059` merge. RustSec also passed on `c8505e021f72824bfe5f6a08dda141522b2db690` (run [45007325573](https://github.com/coreselfmastery/FUNCUT/actions/runs/45007325573), no `--ignore` suppressions), but emitted 11 allowed warnings: nine unmaintained crates and two yanked versions. These warnings need dependency triage; do not call the audit warning-free.
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

**Merge-control finding:** GitHub reports `main` protected, but `required_status_checks.enforcement_level=off` with no required contexts. Direct protection-detail access returned 403, so approval rules could not be inspected. PR #200 had zero formal review submissions and zero inline review threads. Its PR-head checks were green, but the repository is not currently enforcing required status checks through the branch-protection object.

**Release finding:** `.13` remains the latest verified published tag. The `.14` release-note file is not evidence of a published `.14` tag; GitHub did not find that tag. Keep the target-machine and real-project acceptance blockers open.
**Status rule:** never mark the whole FUNCUT product “ready” from this Geneva checklist alone. This page certifies only the Geneva motion-rendering subsystem and its stated boundaries.
