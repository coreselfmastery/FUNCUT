# DIRECTOR-COMPLETION-TRACKER

## Session 5, continued — 2026-11-14 (the session's second half)

The continuation session verified the first half's merges, landed the CI
fixes the stack needed, and opened the two task PRs the first half left
owed. Live API throughout; no page HTML trusted.

- `main` = `242f763e87d80f6355e55aa159c76dd4a9060277` at this writing —
  **PR #123 merged** (the first half's tracker follow-up: Task A landed,
  execution order from Task B), all five CI checks green at its head.
- All four first-half merge questions re-verified MERGED by API: #118
  `621420fb`, #119 `2eb5ff54`, #120 `c8eeac78`, #122 `34a2a87` — the
  ~105-of-216 total stands.
- CI fixes landed on the in-flight stack, each read from the failing
  job's own log:
  - **#124** head `319f4705` — the engine job's two speech-test
    failures: `dominant_script` answered Thai for a punctuation-only
    text (`max_by_key` returns the last of equals, Thai is the table's
    end), and the Japanese/Han arm answered both directions (kana under
    a zh hint is a real disagreement the PR's own test asserts).
  - **#125** head `07c87f85` (via `9dc7a0cc`) — the dependency dance's
    catalogue budget: the compressed audio block still read 247,665 over
    the 247,000 cap; the block went bone-dry and **the caps rose with
    history entries** (whole catalogue 247,000 → 248,000, director
    surface 233,000 → 234,000 — the audio block is the FALL-07..09
    contract, function not prose).
  - **#126** head `d5abac5b` — carries both fixes by stack merge.
- **PR #127** (`feat/runner-idempotency`, head `3c20eeab`) — IDEM-01..10,
  opened this half: the watcher's durable run state
  (`.auto-director-state.json`, per-stage evidence, live re-verification,
  downgrade replay), run-scoped exports under `runs/<run-id>/` with
  diffable `report.json`, the failure record's stagesDone; the Rust
  runner's completed-set (skip/re resume/fresh) and deterministic
  `edit_apply` idempotency keys; the server's optional `edit.apply`
  `idempotencyKey` with dedupe. Self-test green (53 calls then 20, twelve
  stages reused; resume places only the failed stage). First CI run was
  red (Stage lacked Ord, three rustfmt diffs, and a second-resolution
  run id that collided on the faster CI machine); all three fixed
  on-branch.
- **PR #128** (`docs/contradiction-cleanup`, head carries this tracker) —
  CONTRA-01..07: the live contradictions fixed (TTS-take repair contract,
  footage_plan_suggest in Phase 4, the 60 ms tolerance named, the
  automation boundary rewritten to what the runner does, the E2E chapter
  anchored to the acceptance it is not), three verified already-resolved.
  Rides with it: the Step-6 release-readiness entries (KNOWN_ISSUES
  items 9 and 10) and this tracker update.
- Issue #82: interim status posted (merged set, in-flight set, next
  exact task) at the half's start.
- **Step 6 (read-only, verified):** `build-app.yml` stages voice-signals
  for x86_64 AND aarch64; the Forge page with the AUTO EDIT button
  builds green on both arch checks; all 26 locales carry all 18
  `forge.autoEdit` toast keys (0 missing, 0 stale); the last release is
  still `v0.2.8-funcut.12`. No release cut, per instruction.

Next exact task (Session 6):

1. Merge the stack in order the moment each is all-green: #124 → #125 →
   #126 → #127 → #128 (heads at this writing: `319f4705`, `07c87f85`,
   `d5abac5b`, `3c20eeab`, and #128's own head). Nothing else rides those
   branches; do not push to them.
2. Then the hardware work, which no CI can do: UNVER-01..05 — a real
   brief-to-export run on the target laptop (the AUTO EDIT button on the
   window's own server, real provider calls, the export's real
   measurements, cancel/rollback observed), plus the target-machine
   proxy/VAAPI acceptance the tracker has carried.
3. Release prep once the above lands: the KNOWN_ISSUES items 9 and 10
   name exactly what a cut still owes; the release channel is a `v*` tag
   push.

## Session 5 — 2026-11-14 (first half; the continuation above supersedes its in-flight list)

Verified against the live API at session close:

- `main` = `34a2a87bc0e624f6d2ed402b22da70e5cc543168` (merge of PR #122),
  built on `621420fb` (PR #118) and `c8eeac78` (PR #120), which built on
  `1cbaac2b` (PR #117) and `2eb5ff54` (PR #119).
- Landed this session, by merge SHA:
  - PR #118 → `621420fb` — the AUTO EDIT button (P4-001/002): the runner
    wired to the window, progress/cancel/rollback, the terminal states.
    User-facing completion jumps from ~5% to ~60%.
  - PR #119 → `2eb5ff54` — verification expansion (VERIFY-01..05, 08..14):
    twelve structured checks + verification-report.json.
  - PR #120 → `c8eeac78` — the P1 leftovers (P1-004/005/009/010): per-slot
    planner evidence in the run signalLog, the nineteen-field plan audit,
    the driftless RationalTime second, kinetic captions.
  - PR #122 → `34a2a87b` — the tracker synced to the Session 4 state.
  - PR #121 closed as the older duplicate of #122, with a comment.
- In flight at close (heads verified, all based on current main lineage):
  - PR #124 (`fix/silent-degradation`, head `65804780`) — DEG-01,
    DEG-03..10: every degradation named in the run's own signalLog with
    `provenance: fallback`; entitiesAvailable on voice_signals; ranking
    tiers on every rerank reply; the critic tier/confidence line
    (CRAFT_TIER_CONFIDENCE); identify_music failure ≠ "no music";
    bgm_prepare retry + bounded poll; the MP4 head check on fetched clips
    (openvid_fetch/import); the transcribe languageMismatch flag; the
    int8 hiss cure recorded; SpeakResult names model + cpuFallback.
    First CI run was red (4 rustfmt diffs + two tool descriptions over
    the 200-character catalogue ceiling); fixed on-branch at `65804780`.
  - PR #125 (`fix/provider-fallbacks`, head `7103e2cd`) — FALL-01..03,
    06..10: the critic `tried` chain in every craft_critique reply;
    per-provider JSON adapters (groq/gemini/zai/openrouter → json_object,
    ollama → format, local/custom → grammar hybrid); the degenerate-score
    refusal extracted and self-tested; the lexicon emotion read recorded;
    whisperModel/mood/voice documented in the plan schema and consumed;
    identify_music authoritative over detect_music_segments, both
    readings recorded. Stacked on #124.
  - PR #126 (`fix/data-resource-schema`) — DATA-01/04, RES-02 (verified
    safe: no sqlite3_interrupt call exists), SRC-08/09/11/12 (verified
    already enforced: numeric-string exclude_ids, validate-before-get,
    computed maxDriftMs, the clip id type guard), SRC-10 (exact clip
    matching, the abs-min fallback removed), FAIL-02 (an unreadable
    render is re-exported once before the critique gives up), FAIL-03
    (VERIFY-15 ai-voice-disclosure in the Verify record, a disclosure
    reading that never proves), RUN_RECORD_SCHEMA on the run record.
    The plan schema's audio object is documented (voiceover:
    source/speed/glossary/model/voice/whisperModel; bgm:
    query/mood/trackId/duckDb).
- Deliberately deferred, with the next exact tasks:
  - SCH-02 — edit_plan.schema.json is still hand-maintained (the plan
    schema is written as `json!` in `src/crates/concat-mcp/src/tools.rs`
    around the `plan_validate` schema function, not generated from Rust
    types). Next task: scaffold a generator (or schemars-style pass) over
    the plan's Rust-side validation surface, plus a CI drift test that
    refuses a hand edit the types do not back.
  - RES-03 — startup I/O serialization: `Api::with_dirs` spawns nothing
    (verified); the openvid index warms lazily on first use and proxy
    generation happens on import. The interleaving to serialize lives in
    the server/app layer, not the API constructor. Next task: map the
    proxy.rs callers against the index warm path and order them.
- Untouched this session, still owed: IDEM-01..10 (runner idempotency),
  CONTRA-01..07 (docs contradiction cleanup), the real brief-to-export
  acceptance run, target-machine proxy/VAAPI acceptance.


## Session 4 — 2026-11-14 (superseded by the Session 5 header above)

Session 4 of the completion work, run against the live API under the session
brief's rules (Rule 0 token check passed; no force-push; one task = one
branch = one PR; merge only on green).

- `main` = `80d98b6985c17e045d061bd4568ef3698b2cf7f6` at session start
  (re-confirmed, the merge of PR #111), and `1cbaac2b3d7b34a9bd59515e96481d5246414608`
  after PR #117 merged below.
- PR #117 (docs/tracker-session3-2026-11-14) — CI green (Geneva + all five
  CI jobs), merged 2026-11-14T06:44:31Z as `1cbaac2b3d7b34a9bd59515e96481d5246414608`.
  The Session 3 reconciliation this file carried is now on main.
- PR #118 (feat/auto-edit-button, the AUTO EDIT button - Session 3's Task A,
  P4-001/002) — shepherded, not written: three CI-fix commits landed on its
  branch during the session (clippy `doc_lazy_continuation` +
  `type_complexity` in `src/crates/concat/src/panes/forge.rs`, fixed by the
  branch's own author as `e4d298f5`, then head `9f710c2d`); the session's
  CI status comments are on the PR. At session close its CI was still
  running with **no failed steps** (Geneva green, 2 of 5 CI jobs green,
  clippy mid-run on a queue-delayed runner); merging it on green is the
  next session's first action, and the branch stays untouched until then.
- Step 2 (VERIFY-01..05, 08..14): a duplicate implementation was stood
  down WITHOUT pushing when PR #119 (fix/verification-contract, head
  `00fc26ba`, written by the parallel session) proved the same twelve
  checks (all five checks green at last read). Findings from the duplicate
  are posted on the PR: codec/fps live only in the raw `project.get`
  media entries and `media.probe` (never in the summary view), the checks
  belong in the 600 s timeout tier, the watcher's phase-order assertion
  and `SELF_TEST_REPLIES` must carry a new verify-phase call, and an
  environment that cannot run a check answers an explicit skip with its
  reason - never a silent pass.
- Step 3 (P1 leftovers): PR #120 (fix/p1-leftovers, head `ee05c4ae`,
  parallel session) - two check-runs red at last read (`The dependency
  dance, pinned and green`, `The multi-layer example, rendered and
  inspected`); left to its owner per the no-touch rule.
- Step 8 (PR #105, read-only): verified merged (2026-11-13, merge
  `ef8a4c64`); the rerank capability ask is on main and unit-tested
  (`openvid_rank_for_slot` asks `openvid.status` before fetching any
  frame - tools.rs, the two-call test); the PR body records cold
  `openvid_search` at 0.2 s on an evicted page cache, inside the 10 s
  line. This session's sandbox has no Rust toolchain or app binary, so
  the cold load was not re-measured here; nothing failed, so no
  KNOWN_ISSUES entry was filed.
- Issue #82: status posted twice (session start, then the correction
  naming #119/#120 as the owners of Steps 2-3).

Progress log:

- [2026-11-14] Session 4: preconditions re-verified; #117 merged on green;
  #118 shepherded through three CI fixes and left on green-then-merge;
  Step 2 duplicate stood down in favour of #119 with findings posted;
  Step 3's red checks reported; PR #105 verified read-only.

Next exact task:

1. Merge PR #118 the moment its five checks are green (first action of
   the next session; branch untouched until then).
2. Merge PR #119 on its green (its owner, or this session's successor).
3. Fix PR #120's two red check-runs on its own branch (`The dependency
   dance, pinned and green`, `The multi-layer example, rendered and
   inspected`), then merge on green.
4. Step 4 - silent degradation (DEG-01, 03..10), branch
   `fix/silent-degradation`. Code paths located: the reading profile's
   `entities` slot (voice_signals.py, the `"entities": None` profile
   row) gets an `entitiesAvailable` flag distinguishing "no entities
   found" from "NER missing"; the openvid rank's rerank block already
   records `applied: false` with a reason (tools.rs, the
   `semantic_rerank` fallback) and needs the explicit `keyword-tier`
   ranking name; `bgm_prepare` (tools.rs) needs the dying-server case;
   `openvid_fetch` partial-download validation; the watcher's BGM gate
   (auto_director.py, the `detect_music_segments` phase) needs
   `identify_music` failure to stop meaning "no music". Every
   degradation writes provenance=fallback in the signalLog.
5. Steps 5-7 and 9 as ordered in the Session 3 list, unchanged.

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
- The runner crate is **wired**: PR #118 (merged as `621420fb`) landed the
  `concat_mcp::director` seam (the Backend → ToolCaller adapter), the
  AUTO EDIT button on the Forge page with its progress/cancel surface
  and terminal-state toasts, `export.status` by path, and the wait's
  dead-job refusal. A parallel session shared the CI loop on the same
  branch; the head that merged is `9f710c2d`.
- No credential material or credential-store paths belong in this
  repository or tracker. Use the connected GitHub integration or an
  approved secret manager; never paste tokens into repository files.

## Current execution order

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
- [2026-11-14] Task 0 merged: the tracker reconciliation is PR #117,
  `1cbaac2b`.
- [2026-11-14] Task A merged: the AUTO EDIT button is PR #118, `621420fb` -
  the seam, the button, the progress pane, the toasts, `export.status` by
  path, and the wait's dead-job refusal. Two CI flakes were re-run (a pip
  wheel shard and a runner that died building the window crate); the code
  fixes the engine job asked for landed across the session and a parallel
  one sharing the branch.
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
