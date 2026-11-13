# DIRECTOR-COMPLETION-TRACKER

## Session 2 — 2026-11-14 (in progress)

Preconditions checked at session start (per the Session 2 handoff):

- `main` = `ca7142e9`. **CI GREEN** — all 7 check-runs success (Format,
  locales and models; analysis senses; Engine tests and lints; multi-layer
  example; linux-aarch64; dependency dance; linux-x86_64), verified via the
  authenticated check-runs API. Session 1's open question (run #751 in
  progress) is resolved: it concluded green.
- concat-director crate NOT on main (local branch only) — precondition 1
  failed at start; this session lands it (below).
- Revise loop NOT on main (`auto_director.py:69` stub still present) —
  precondition 2 failed at start; this session lands it (Task C).
- PR #105 (fix/openvid-cold-load): **MERGED**, merge SHA `ef8a4c64`
  (2026-11-13T10:25:43Z).
- PR #108 (fix/recording-quality-floor-window): **OPEN**, untouched, per
  the rules. Branch head `fe15de9e` on origin.
- Credential: valid token supplied by the user mid-session (login
  `coreselfmastery`, admin/push on the repo). Stored OUTSIDE the repo
  (`/home/z/my-project/.git-credentials-funcut`, mode 600); never
  committed. **Rotate it after the session** — it transited the chat.

User directive for this session: land everything on main, keep working,
do not stop. Execution order (Session 1 leftovers first, they block):

1. **S1 Task B landing** — push `feat/concat-director` (commit `4584370d`
   + this tracker as `docs/DIRECTOR-COMPLETION-TRACKER.md`), open PR,
   wait CI green, merge. Closes P0-015..021, P2-001..010, RUN-01..17,
   CONC-01/02 (the types, stages, API contract and state machine).
2. **P0-001/002 unification** — tighten `plan_validate` to the executor's
   requirements (broll: real query required; textCard/titleCard:
   storyboard.scenes required), per Session 1's evidence (tools.rs:3141+
   vs auto_director.py:1029+). Branch `fix/slot-contract`.
3. **S1 Task C revise loop** — replace the score<7 stop
   (auto_director.py:1879+) with WATCH→revise-per-slot (max 2)→re-WATCH,
   ship CompletedWithWarnings. Closes P1-006/007, INT-04/06, PLAN-05,
   FALL-04/05. Branch `feat/revise-loop`.
4. **S2 Task A** — AUTO EDIT button + toasts (P4-001/002), needs the
   merged crate. Branch `feat/auto-edit-button`.
5. **S2 Task B** — verification expansion VERIFY-01..14.
6. **S2 Tasks C..I** as context allows; Task H = #105 verification
   report + #108 noted.
7. Tracker updated after every landing; final report per the handoff
   format.

Progress log (appended as tasks land):

- [2026-11-14] Local gates re-verified on `feat/concat-director` before
  push: fmt clean, clippy -D warnings clean, 26/26 tests pass (Rust
  1.93.1, workspace at `src/`).

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

## BLOCKER that defined this session

The GitHub token pasted into the session prompt is DEAD: `GET /user`
returns 401 "Bad credentials" (tested with both `Bearer` and `token`
schemes). The prior agent's credential store
(`/home/z/.git-credentials-funcut`, documented in AGENT_STATE.md) no longer
exists - the sandbox was recycled on 2026-11-13 15:45 and everything
outside /home/z/my-project was lost. The repository is public, so all work
below is READ-ONLY plus LOCAL branch preparation. NOTHING was pushed;
no PRs were opened; main was not touched.

**Next session needs: a valid GitHub credential with write access to
coreselfmastery/FUNCUT before anything can land.**

## Task B - concat-director crate: PREPARED, LOCAL BRANCH, NOT PUSHED

- Branch: `feat/concat-director` (in the sandbox clone at
  `/home/z/my-project/funcut`), commit `4584370d`, 8 files, +1859 lines.
- Files: `src/crates/concat-director/{Cargo.toml,
  src/{lib,error,stage,state,runner}.rs}` + workspace manifest entry
  (`src/Cargo.toml` [workspace.dependencies]) + `src/Cargo.lock`.
- Local gates ALL GREEN with the pinned toolchain (1.93.1, installed at
  /home/z/my-project/.rustup, CARGO_HOME=/home/z/my-project/.cargo):
  - `cargo fmt --check` (workspace-wide): CLEAN
  - `cargo clippy -p concat-director --all-targets -- -D warnings`: CLEAN
  - `cargo test -p concat-director`: 26/26 PASS
- CI-equivalent gates from ci.yml were replicated for the new crate only;
  the workspace-wide clippy and the multi-platform builds still need the
  real CI run after push.
- Design (per the handoff spec):
  - `AutoEditError {stage, tool_call, message, retryable, retry_after,
    context}`, `AutoEditProgress {stage, stage_index, total_stages,
    elapsed_total, elapsed_stage, detail}`, `AutoEditEvent {state,
    progress}`, `AutoEditState {Idle | Running{stage,started} |
    Failed{stage,error,attempt} | Completed{export,duration,verification}
    | CompletedWithWarnings{export,duration,warnings,verification} |
    Cancelled{at,rolled_back,undone_to}}`, `Stage` with the 15 specified
    variants, `Runner` holding `project: PathBuf`, `cancel:
    Arc<AtomicBool>`, `state: Arc<Mutex<AutoEditState>>`, `events:
    Sender<AutoEditEvent>` (behind a Mutex so the cloneable runner is
    Sync).
  - `progress()` reads the last emitted event (single source of truth).
  - `cancel()` stops at the next stage boundary; rollback is one
    `edit_undo` per applied `edit_apply` (the API's own per-edit history,
    message.rs:125-139); terminal `Cancelled{rolled_back, undone_to}` says
    whether the rollback reached all the way back.
  - Retries mirror the watcher exactly: 3 attempts when the tool is in
    RETRYABLE_TOOLS and the message matches the markers, pauses 2s/4s
    (settable for tests).
  - `ErrorPolicy::Warn` stages record per-slot failures and the run ships
    `CompletedWithWarnings` - the seam Task C's revise loop needs.
  - THE SEAM: the runner drives a `ToolCaller` (one MCP-shaped
    `ToolCall{tool, arguments}` at a time) because `Backend` lives in
    concat-mcp and concat-mcp is the crate that will depend on
    concat-director (dependency direction forbids naming Backend here).
    The Backend->ToolCaller adapter lands with the AUTO EDIT wiring
    (P4-001). This is not an invented backend: backend.rs
    (src/crates/concat-mcp/src/backend.rs:38-57) has the real trait:
    `pub trait Backend: Send { fn call(&mut self, request: Request) ->
    Result<Value, ApiError>; fn describe(&self) -> Value; fn roots(&self)
    -> Vec<PathBuf> {..} fn finish(&mut self) {} }` with `Window` and
    `Headless` implementations.
- Unblocks (once landed + wired): P0-015..P0-021, P2-001..P2-010,
  RUN-01..RUN-17, CONC-01, CONC-02, P4-001 (button), P4-002 (toast),
  TIME-01..13 and XSTAGE-01..10 and IDEM-01..10 (which need the state
  machine).
- NOT DONE: push, PR, CI, merge. Rule 7 applies: no green CI, no "done".

## Step 3 - P0-001/002 slot contract: ANALYSIS COMPLETE, FIX NOT LANDED

The core contract IS unified and matches the docs: "slots[] is the
shopping list for segments[].visual - the executor runs the segments, not
the slots" (tools.rs:3386-3460) and the executor (auto_director.py:1029+
`broll_segments()`) indeed iterates segments and never reads `slots[]`.
BUT plan_validate accepts two plan shapes the executor cannot run - the
derivation is NOT the same on two fields:

1. **broll + clipId, no query** - validator passes ("B-roll needs a search
   query or a non-empty clipId", tools.rs:3141-3152; both schemas leave
   `query` optional) but the executor REQUIRES a real `visual.query` for
   EVERY broll segment (auto_director.py:1032-1034, PipelineError) and
   never honors `clipId` on broll (clipId is read only for `existing`
   segments: lines 585, 876, 931-938, 1257).
2. **textCard/titleCard without storyboard** - validator only checks
   non-empty `text` (tools.rs:3170-3177; `storyboard` optional in both
   schemas) but the executor requires a complete `visual.storyboard` with
   `scenes` (auto_director.py:870-873 pre-flight and 1282-1286: "needs an
   authored storyboard; refusing to export an empty graphic beat").

Unification PR needed (next session). Recommended direction: tighten
plan_validate to require exactly what the executor requires (broll: real
query; textCard/titleCard: storyboard.scenes) - the executor is the ground
truth of what can run. Honoring broll clipId in the executor would be a
feature ADDITION, not a unification; decide deliberately, do not do both
silently.

Also recorded while here (SCH-02): the canonical plan schema is a
hand-maintained skill template include_str!'d from
`.claude/skills/funcut-director/templates/edit_plan.schema.json`
(tools.rs:1846-1851), NOT generated from Rust types - SCH-02's suspicion
is confirmed; the compact tool-input schema in Rust is separately
hand-written (tools.rs:1897+).

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

## Environment notes for the next session

- Sandbox was recycled 2026-11-13 15:45: prior agent's home files are
  GONE (credential store, funcut-env.sh build environment, FFmpeg 8.1 dev
  digest setup, ONNX, sherpa). ffmpeg 7.1.5 runtime exists but NO dev
  headers (pkg-config finds no libavcodec) - concat-media
  (ffmpeg-the-third) cannot build locally without reinstalling the dev
  environment; concat-director deliberately depends on nothing but
  serde/serde_json/thiserror so it builds standalone (verified).
- Rust 1.93.1 (rustc/cargo/clippy/rustfmt) is installed at
  /home/z/my-project/.rustup + /home/z/my-project/.cargo. Export:
  RUSTUP_HOME=/home/z/my-project/.rustup CARGO_HOME=/home/z/my-project/.cargo
  PATH=/home/z/my-project/.cargo/bin:$PATH
- Background processes are KILLED when a Bash call returns: run long
  builds in the foreground with a long timeout.
- GitHub anonymous API quota is tiny and shared by the egress IP: use
  workflow badges (https://github.com/coreselfmastery/FUNCUT/actions/workflows/<wf>.yml/badge.svg?branch=main)
  or the Actions page JSON, or bring a token.
- Repo: https://github.com/coreselfmastery/FUNCUT (public). Clone lives at
  /home/z/my-project/funcut with branch feat/concat-director ready.

## Next exact task (in order)

1. Obtain a valid GitHub credential (the pasted one is dead).
2. Confirm main's merge-commit CI (#751 on ca7142e9) concluded green; if
   red, read the failing job's log FIRST (revert-only mode per the rules).
3. Push `feat/concat-director` (commit 4584370d), open PR, wait CI green,
   merge. Report SHA. Commit this tracker file with that PR or the next.
4. P0-001/002 unification PR per the evidence above (one decision: tighten
   the validator to the executor's requirements).
5. Task C PR (revise loop) per the evidence above.
6. Task D real E2E run + runs/<timestamp>/report.json.
7. Update this tracker; prune the pending list.
