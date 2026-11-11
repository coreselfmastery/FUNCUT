# Geneva master list — current verified state

**Last reconciled:** 11 November 2026  
**Repository:** `coreselfmastery/FUNCUT`  
**Canonical implementation:** `src/crates/concat-motion`  
**Sidecar contract:** Geneva 1.6.0, pinned by the installer and CI dependency lock  
**Purpose:** one current status page. The 1 November four-pillar audit remains historical evidence; its original “Findings register” and “What remains” sections are superseded by its 3 November postscript and this page.

## Executive status

Geneva is a substantial, integrated motion-rendering path, not a placeholder. The four pillars—Creative, Performance, Reliability and Agent—are implemented in the repository. The ten findings in the original 1 November register are recorded as closed in the 3 November postscript. The current Geneva workflow also exercises the real sidecar, rendered output, scene-quality floors and golden-frame comparisons.

**Do not interpret this as proof that every FUNCUT release requirement is complete.** This page is scoped to Geneva. It does not certify the separate full AI Director workflow, target-laptop performance, Lottie playback, or the editor's general release readiness.

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
- `absolute_position` folds static ancestor positions only. PR #84 is merged to `main` (merge commit `c45ac873`): non-markup comps with inherited parent placement, position tracks, scale or rotation route to native; mixed markup comps with native-only features are refused until Geneva can represent them.
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
- Geneva workflow run `42975353241` (PR #81): both rendered-example and dependency-dance jobs completed successfully.
- PR #81's separate general Rust engine/lint job was still in progress at the last poll; do not call the entire PR green until that job completes.
- PR #84 merged as `c45ac873`; the merge-commit CI checks were still running at the last poll. Verify those checks before calling the merged implementation fully validated.
- PR #80's general Rust engine/lint job was also still in progress at the last poll.
- The main-branch CI state must be checked again after the open PRs land. A green Geneva-only run does not replace all workspace checks.

## 7. Remaining work — Geneva scope

No item from the original ten-finding register is currently listed as open. The following are the remaining validation/roadmap boundaries, not claims that the closed findings are still broken:

- [ ] Recheck the full Geneva workflow on the final merged `main` commit after pending PRs merge.
- [ ] Keep scene-quality and golden-frame gates green on every relevant change; review any intentional golden/quality-floor update.
- [ ] Run and record representative timing/memory measurements on the target low-memory Linux machine for realistic 1080p motion workloads. PR #87 adds a reproducible cold 1080p30 Geneva benchmark that records machine, sidecar version, wall time, output size and sampled process-tree RSS; it still needs CI/merge and a real antiX run. Hosted CI proves correctness, not target-device speed.
- [ ] Add true Geneva support for mixed markup comps that also use native-only shape gradients, mattes, effects, paths or masks. PR #84 has shipped an explicit safe refusal and native fallback for non-markup comps; true rendering parity remains open.
- [ ] Decide whether mixed markup needs full inherited transform parity; the native fallback cannot be used for HTML/CSS comps.
- [x] PR #84 makes `needs_markup()` recursive so nested HTML sub-comps do not incorrectly fall back to native when the sidecar is missing; regression coverage is included.
- [ ] If offline cache use without the sidecar is desired, change the version/key contract deliberately and test compatibility; current behaviour intentionally requires the sidecar.
- [ ] Keep Geneva 1.6.0 pin, checksum, installer, package contents and supported platform binaries aligned whenever the sidecar is upgraded.
- [ ] Merge PR #86: the installer currently has an unchecked-install fallback when neither `sha256sum` nor `shasum` is available. PR #86 makes the optional sidecar install fail closed and adds a CI shell-syntax check.

## 8. Separate FUNCUT work — not Geneva blockers

These must remain on the broader FUNCUT release tracker, not be misreported as Geneva defects:

- Native Lottie/dotLottie playback; the `lottie` engine is reserved and import/conversion is not playback.
- Timeline Motion-clip creation/edit command and full editor UI lifecycle.
- Per-timeline view-state isolation.
- Export queue and explicit timeline picker.
- Linux VAAPI encode integration and hardware validation.
- 1080p proxy performance measurements on the actual antiX laptop, including H.264 versus MJPEG tradeoffs.
- Full AI Director brief-to-final-export acceptance.

## 9. Definition of Geneva-ready

Geneva is ready for a release candidate when all of the following are true on the same commit:

- [ ] The current `main` SHA is recorded.
- [ ] Geneva example and dependency-dance jobs pass on that SHA.
- [ ] Required workspace tests, clippy and formatting checks pass on that SHA.
- [ ] Scene-quality and golden-frame checks pass without unreviewed baseline changes.
- [ ] The pinned sidecar installs and is discovered on each supported package/platform target.
- [ ] Target-machine timing/memory evidence is attached, or the release notes clearly state that only hosted correctness—not target performance—has been validated.

**Status rule:** never mark the whole FUNCUT product “ready” from this Geneva checklist alone. This page certifies only the Geneva motion-rendering subsystem and its stated boundaries.
