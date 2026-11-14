# DIRECTOR-COMPLETION-TRACKER

## Session 3 — 2026-11-14 (current status)

Fresh repository state for this reconciliation, verified against the API and
a fresh clone:

- `main` = `80d98b6985c17e045d061bd4568ef3698b2cf7f6`, the merge of PR #111
  (Kokoro classifier floor window, P3-004).
- Merged into main, in order, all verified by merge-commit SHA:
  - PR #110 → `77392956859352c18924e728b1ccdaa286ac60f7` — the
    `concat-director` runner crate (P0-015..021, P2-001..010, RUN-01..17,
    CONC-01/02).
  - PR #115 → `e14cb278…` — slot contract unified (P0-001/002), closing the
    plan/executor mismatch that PR #112 had been opened for.
  - PR #116 → `a5971920…` — the revise loop (P1-006/007, INT-04/06,
    PLAN-05, FALL-04).
  - PR #111 → `80d98b69…` — the recording noise floor measured across
    short pauses (P3-004).
  - PR #113 and #114 (Geneva and tracker doc reconciliation) also merged.
- Session 1's ~30 Python fixes are intact on main.
- PR #112 is **closed** (duplicate of the merged #115). PR #108 is
  **closed** (superseded by the merged #111). Both closures were posted
  with comments naming their replacements.
- `funcut-env.sh` is **NOT FOUND** on main (verified by `git ls-files` on a
  fresh clone); no environment script of that name exists in the tree.
- The runner crate is still **not wired** into `concat-mcp` or the editor
  UI: no AUTO EDIT button, no integrated progress/cancel surface. That
  wiring is this session's Task A (branch `feat/auto-edit-button`).
- No credential material or credential-store paths belong in this
  repository or tracker. Use the connected GitHub integration or an
  approved secret manager; never paste tokens into repository files.

## Current execution order

1. Wire `concat-director` into the MCP backend and editor UI: the Backend →
   ToolCaller adapter, the AUTO EDIT button, progress/cancel and the
   terminal states (Task A, branch `feat/auto-edit-button`).
2. Verification expansion: the 11 missing Verify-stage checks (VERIFY-01..05,
   08..14), branch `fix/verification-contract`.
3. P1 leftovers (P1-004, 005, 009, 010), branch `fix/p1-leftovers`.
4. Silent degradation (DEG-01, 03..10), branch `fix/silent-degradation`.
5. Provider fallbacks (FALL-01..03, 06..10), branch `fix/provider-fallbacks`.
6. Data/resource/schema (DATA, RES-02/03, SCH-02, FAIL-02/03, SRC-08..12),
   branch `fix/data-resource-schema`.
7. Runner idempotency (IDEM-01..10), branch `feat/runner-idempotency`.
8. Verify PR #105 (OpenVid cold load) and #111 (Kokoro classifier) on a
   real run.
9. CONTRA-01..07 contradiction cleanup, branch `docs/contradiction-cleanup`.
10. Complete the real brief-to-export AI Director acceptance run and inspect
    the resulting media.
11. Complete target-machine 1080p proxy and real Intel/AMD VAAPI acceptance.
12. Publish only after all required checks, reviews, packaging and release
    evidence are green.

Progress log:

- [2026-11-14] Session 3 opened: preconditions verified (token, main SHA,
  merge CI), PR #112 and #108 closed with comments, this tracker brought up
  to the real state.
- [2026-11-13] PR #110 merged; PR #112 opened for the plan/executor
  contract mismatch (later made redundant by #115); PR #113 opened for
  Geneva evidence.
- [2026-11-13] Session 2 reconciliation recorded below.

---

# Session 2 record (2026-11-13) — superseded, preserved verbatim below

The state this record describes was true at its writing; every open item it
lists was since resolved (see the Session 3 header above for what actually
landed).

## Session 2 — 2026-11-13 (status at that writing)

- `main` = `77392956859352c18924e728b1ccdaa286ac60f7`, merge of PR #110.
- PR #110 added the `concat-director` runner crate. Its Geneva workflow passed both jobs; CI format/locales/models and real-fixture analysis passed while engine tests/lints were still running; Nix was still building at this snapshot. The push-triggered Build app run for this exact main commit was not discoverable through the connected workflow-run interface.
- The runner crate is **not wired** into `concat-mcp` or the editor UI. No AUTO EDIT button or integrated progress/cancel surface is present.
- The revise loop is **not implemented**: `scripts/auto_director.py` still stops when the craft score is below 7 instead of revising and re-watching.
- PR #108 remains open on an old base. PR #111 is its replacement based on current main; close #108 only after #111 passes.
- PR #112 aligns `plan_validate` with executor requirements. Current head: `d682198cafbf3380c78773f1951e6997e5f4b862`; its previous CI attempt exposed a missing storyboard in the canonical example and rustfmt differences, which were fixed in follow-up commits. The new exact-head CI/Geneva runs are pending.
- PR #113 reconciles the Geneva master list to current main. Current head: `bf481d1d7302895230340f001907b966a3b797fd`; CI/Geneva are pending.
- No credential material or credential-store paths belong in this repository or tracker. Use the connected GitHub integration or an approved secret manager; never paste tokens into repository files.

## Session 2 execution order (all since done or closed)

1. Finish and validate PR #112; fix every exact-head CI/Geneva failure.
2. Finish PR #111; close stale PR #108 only after the replacement is green.
3. Implement the WATCH → per-slot revise (maximum two attempts) → re-WATCH loop, with explicit warning outcomes and regression tests.
4. Wire `concat-director` into the MCP backend and editor UI, including progress, cancellation, rollback and terminal states.
5. Complete the real brief-to-export AI Director acceptance run and inspect the resulting media.
6. Complete target-machine 1080p proxy and real Intel/AMD VAAPI acceptance.
7. Publish only after all required checks, reviews, packaging and release evidence are green.

Session 2 progress log:

- [2026-11-13] PR #110 merged as `77392956859352c18924e728b1ccdaa286ac60f7`; its full CI and Nix validation were still running at the latest poll.
- [2026-11-13] PR #112 opened for the plan/executor contract mismatch; first CI run found and exposed a canonical example that was not executable, plus rustfmt differences. Both were corrected in follow-up commits; fresh checks pending.
- [2026-11-13] PR #113 opened to update Geneva's current-main validation evidence.

---

# Session 1 record (2026-11-13/14) — preserved verbatim below

Session of 2026-11-13/14. This file did not exist in the repo; committed
as `docs/DIRECTOR-COMPLETION-TRACKER.md` with the concat-director PR.

## Baseline at session start

- `main` = `ca7142e9` (merge of PR #109, merged 2026-11-13T15:54:24Z). HEAD
  confirmed by fresh clone.
- Merge-commit CI on `ca7142e9` was STILL RUNNING at last observation
  (~17:35 UTC). Exact run table for the merge commit: Geneva #607
  SUCCESS, Build app #87 SUCCESS, Models passing; CI #751 (id
  43457293479, created 15:54:26Z) IN PROGRESS - its engine job has a
  75-minute budget, so still in normal range. The ci.yml badge reads
  "failing" but that is the PRE-repair run #750 (GitHub badges show the
  last COMPLETED run while a newer one is in flight; proved: build-app's
  badge said "passing" while #87 was still in progress). Main is NOT
  known-red and NOT known-green on #751 yet. RE-VERIFY #751's conclusion
  before building on main; if it failed, read the failing job's log
  first and stay revert-only per the rules.
- PR #105 (fix/openvid-cold-load, "the rerank asks whether it can run
  before it fetches"): **MERGED** 2026-11-13T10:25:43Z, merge SHA
  `ef8a4c64`. P3-001's fix has landed; still needs a real-run verification
  (cold index on a long-lived session).
- PR #108 (fix/recording-quality-floor-window, "the floor is measured
  where the pauses are"): **OPEN**, untouched, as required. Closes P3-004
  when merged. Do not touch its files.
- PR #109: merged = main's HEAD. Confirmed.

## Historical access limitation

The previous automation sandbox did not have a usable authenticated GitHub write path. That limitation is historical and superseded by the current connected GitHub integration. No tokens, credential-store paths, account permissions or local credential locations are recorded in this repository.

## Task B - concat-director crate: MERGED, NOT INTEGRATED

- PR #110 merged as `77392956859352c18924e728b1ccdaa286ac60f7`; head `91c5754f1cd7102a6206541af7b8cc0e24ea8c0b`.
- The crate adds the runner's stage program, retry classification, progress events, cancellation and rollback state. It is a foundation, not a working product entry point.
- `concat-mcp/Cargo.toml` and `concat/Cargo.toml` do not depend on `concat-director`; the MCP backend and Forge/editor UI do not call `Runner`. There is no AUTO EDIT button yet.
- PR #110 validation snapshot: Geneva passed 2/2; CI format/locales/models and real-fixture analysis passed, engine tests/lints were in progress; Nix build was in progress. Build app for the merge commit was not independently verified through the connected workflow-run interface.
- Do not describe the AI Director as integrated or end-to-end ready until the runner is wired, the revise loop is implemented, and a real project is accepted through final export.

## Task C - revise loop: NOT STARTED (deferred)

- Current behavior: scripts/auto_director.py:1879-1883 raises
  PipelineError "rendered cut scored {score}/10; revision is required
  before publishing" when craft score < 7; docstring lines 68-69 say the
  same. (The prompt's "line 51-52" numbering was stale.)
- What exists to build on: `craft_critique`'s reply already carries
  `overall`, `scores{}`, `notes[]`, `tier`, `confidence` (see the selftest
  mock at auto_director.py:2180-2181); `watch_render` returns notes
  (1857-1859); continuity replace machinery with `exclude_ids` +
  `attempt_replace(slot_id, avoid)` already exists at auto_director.py:1126+
  - the revise loop's B-roll re-shop can reuse its shape. Max 2 revisions;
  per-slot notes addressed individually; ship CompletedWithWarnings (the
  runner type from Task B exists for the Rust side later).

## Task D - real E2E: NOT RUN (deferred)

Needs the pipeline runnable end to end (Kokoro TTS, real clips, the
14-point verification, runs/<timestamp>/report.json). Also needs a working
credential for any follow-up commits (KNOWN_ISSUES entries for failures).

## Quick verifications done this session (read-only)

- P0-014 (sounds_search): CONFIRMED wired - ENRICHMENT_TOOLS +
  RETRYABLE_TOOLS membership (auto_director.py:113-114, 130), SFX phase
  planning (616-618), execution with served-gate (1584-1602). No action.
- FAIL-02 (watch_render failure): current behavior confirmed - a
  watch_render refusal is retried per the retryable contract, then STOPS
  the run with the tool's words (auto_director.py:316-334, 1845-1856); it
  is not converted to a low craft score. Whether it should count as a
  failed critique (feeding the revise loop) is Task C's design decision.
- SCH-02: the canonical edit-plan schema is hand-maintained (see Step 3
  above) - not generated from Rust. Confirmed open.

## Environment notes

- Local Rust builds were not run in the current automation sandbox; use exact-head GitHub CI for source validation.
- Target-laptop proxy measurements and real Intel/AMD VAAPI tests require the actual hardware; hosted CI is not a substitute.
- Keep all credentials outside the repository and do not include private paths or account details in handoff notes.

## Next exact task

1. Complete PR #112's current CI and Geneva runs; fix any failures on the exact head.
2. Complete PR #111's current CI and Geneva runs; then close PR #108 as superseded.
3. Implement and test the revise loop.
4. Wire the runner into MCP and the editor UI, including progress and cancellation.
5. Run a real AI Director project from brief through final export and inspect the output.
6. Measure 1080p long-GOP proxy behavior on the target antiX machine and test VAAPI on real hardware.
7. Reconcile the release tracker, require review/check evidence, validate both Linux packages, and publish only a fully verified release.
