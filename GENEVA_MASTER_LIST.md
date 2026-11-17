# Geneva master list — current verified state

> Counts and the canonical feature inventory live in
> [docs/FEATURES.md](docs/FEATURES.md). This page carries Geneva's own
> dated detail.

**Last reconciled:** 17 November 2026  
**Repository:** `coreselfmastery/FUNCUT`  
**Canonical implementation:** `src/crates/concat-motion`  
**Sidecar contract:** Geneva 1.6.0, pinned by the installer and CI dependency lock  
**Purpose:** Geneva's own current status page; the canonical whole-repo inventory is [docs/FEATURES.md](docs/FEATURES.md). The 1 November four-pillar audit remains historical evidence; its original “Findings register” and “What remains” sections are superseded by its 3 November postscript and this page.

## Executive status

Geneva is a substantial, integrated motion-rendering path, not a placeholder. The four pillars—Creative, Performance, Reliability and Agent—are implemented in the repository. The ten findings in the original 1 November register are recorded as closed in the 3 November postscript. The current Geneva workflow also exercises the real sidecar, rendered output, scene-quality floors and golden-frame comparisons.

**Do not interpret this as proof that every FUNCUT release requirement is complete.** This page is scoped to Geneva. It does not certify the separate full AI Director workflow, target-laptop performance, or the editor's general release readiness. The latest observed `main` commit is `eaa3072b8d6986e19eb9d9d76758ccab8350c5fb` (17 November 2026), the merge of PR #166. PR #166's tested head `154f8f802211177e625e140026735819ff2ac33e` is an ancestor of main: compare reports main 3 commits ahead, 0 behind, with only documentation files changed after that head. Its CI passed all 3 jobs in [run 44113256469](https://github.com/coreselfmastery/FUNCUT/actions/runs/44113256469), and Geneva passed both jobs in [run 44113256479](https://github.com/coreselfmastery/FUNCUT/actions/runs/44113256479). For exact main SHA `eaa3072...`, the connected commit-status endpoint returned no contexts and the workflow-run endpoint returned no push-triggered runs; exact-main push CI, Geneva, Nix and Build app are therefore still unverified. The security-audit run triggered by PR #168 [failed](https://github.com/coreselfmastery/FUNCUT/actions/runs/44139603702): two new, unignored `zip 2.4.2` advisories (`RUSTSEC-2026-0548` and medium `RUSTSEC-2026-0545`) remain in the `sherpa-onnx-sys 1.13.7` dependency subtree. No ignore was added. The two open PRs are #167 (the AUTO EDIT licensing fix) and #168 (this Geneva/release evidence refresh). PR #167 replaces AUTO EDIT's unconditional OpenVid import with Pexels/Pixabay live-stock imports. Its latest head is `c4121fa54ae548def8fbbddcf7b5509c945036a0`. On the preceding head `71f4ba653af2825bd6477a6101feffcb1ef2579b`, Geneva's dependency-dance job failed while compiling a new test: Rust E0515, the test returned a borrowed `call.tool` from a temporary. The test now keeps the call value before asserting its tool name. On corrected head `c4121fa54ae548def8fbbddcf7b5509c945036a0`, [CI run 44139424113](https://github.com/coreselfmastery/FUNCUT/actions/runs/44139424113) has Format/locales/models and real-fixture analysis passed; the engine job is still running. In Geneva [run 44139424119](https://github.com/coreselfmastery/FUNCUT/actions/runs/44139424119), dependency-dance passed and its log confirms `cargo test -p concat-motion -p concat-core -p concat-mcp -j 1`, including both new B-roll tests (`concat-mcp`: 81 passed, 0 failed); the multi-layer render/inspection job is still running. The release `v0.2.8-funcut.13` is published with ten recorded assets, but its tag `ebfd9e30cf0e321236c4a73e231367289b3f41da` is 48 commits behind this main head. A compare request for `v0.2.8-funcut.14` returned 404; no .14 tag is currently present.

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

**17 November 2026 follow-up:** PR #166's head `154f8f802211177e625e140026735819ff2ac33e` completed the full CI workflow: Format/locales/models; workspace Clippy; test-binary build; unit tests; documentation examples; generated plan-schema check; gRPC Clippy/tests; and the real-fixture analysis job, including the Auto Director end-to-end safety self-test. Geneva completed both jobs: pinned sidecar/dependency setup, motion tests, rendered multi-layer example, benchmark-tool smoke, frame/style inspection, scene-quality floors and golden-frame DSSIM. The merge commit is current main `eaa3072b8d6986e19eb9d9d76758ccab8350c5fb`; compare shows only three subsequent documentation commits and no code-file changes. This is strong PR-head evidence for the merged code, but not a replacement for an exact-main push run. Build app (Linux x86_64/ARM64), Nix and release publication on this exact main remain unverified.

## 7. Remaining work — Geneva scope

No item from the original ten-finding register is currently listed as open. The following are the remaining validation/roadmap boundaries, not claims that the closed findings are still broken:

- [x] Historical 12 November baseline `cbcf821e465c265aef26071be3350fe239619aaf`: Geneva, Nix and Linux x86_64/aarch64 packaging passed on that exact SHA; general CI's format/locales/models and real-fixture jobs passed, with engine tests still running at the last poll. This is historical evidence, not current-main validation.
- [ ] Current main `eaa3072b8d6986e19eb9d9d76758ccab8350c5fb` is 48 commits ahead of the published `.13` tag. PR #166's head CI (3/3 jobs) and Geneva (2/2 jobs) passed, and main is three documentation-only commits ahead of that tested head. The connected workflow-run interface returned no push-triggered runs and the combined-status endpoint returned no contexts for exact main, so exact-main push CI, Geneva, Nix and package results remain unverified. PR #141's RustSec workflow is present, but its current-main execution result was not retrievable.
- [x] Historical scene-quality and golden-frame gates passed on the 12 November baseline in Geneva run `43236482716`; keep these mandatory on every relevant future change and review any intentional golden/quality-floor update.
- [ ] Re-run scene-quality and golden-frame gates on exact current main `eaa3072b8d6986e19eb9d9d76758ccab8350c5fb` and retain the render reports; PR #166's tested head passed both Geneva jobs, but no push-triggered Geneva run for the merge SHA was retrievable.
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
- [ ] Current main `eaa3072b8d6986e19eb9d9d76758ccab8350c5fb` is not exact-SHA validated: the connected interface returned no push-triggered workflow runs and no combined-status contexts for this SHA. PR #166's head passed all CI/Geneva jobs and main has only three documentation commits beyond it, but do not call that an exact-main push run.
- [ ] Exact-main Geneva, Nix, Linux x86_64 and ARM64 packaging results are unverified through the connected workflow-run interface. PR #166's Geneva and CI head checks passed, but there is no exact-main Build app or Nix evidence.
- [x] Historical scene-quality and golden-frame checks passed on the 12 November baseline in Geneva run [43236482716](https://github.com/coreselfmastery/FUNCUT/actions/runs/43236482716).
- [ ] Re-run scene-quality and golden-frame checks on current main and retain the exact-head render reports.
- [x] Mixed markup comps that contain native-only features are explicitly refused with a useful diagnostic; PR #85's selection-time explanation and regression test merged as `cf82462c`.
- [x] The pinned sidecar installed and was discovered in the Linux x86_64 and aarch64 package builds on the historical 12 November baseline (run [43236482829](https://github.com/coreselfmastery/FUNCUT/actions/runs/43236482829)); these artifacts do not validate the current main SHA.
- [ ] Target-machine timing/memory evidence is attached, or release notes clearly state that only hosted correctness—not target-device performance—has been validated.
- [x] The `v0.2.8-funcut.13` release is published (2026-11-16) with ten recorded assets; its tag `ebfd9e30cf0e321236c4a73e231367289b3f41da` is 48 commits behind current main `eaa3072b8d6986e19eb9d9d76758ccab8350c5fb`. A compare request for `v0.2.8-funcut.14` returned 404, so a `.14` tag/release is still owed after current-main, hardware and real-project acceptance.

**Status rule:** never mark the whole FUNCUT product “ready” from this Geneva checklist alone. This page certifies only the Geneva motion-rendering subsystem and its stated boundaries.
