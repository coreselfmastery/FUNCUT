# Forge AUTO EDIT — 20-session completion plan

**Mission:** complete and validate the Forge AUTO EDIT button as a connected, dependable editing workflow. This plan is the durable hand-off record for future `continue` sessions.

**Repository:** `coreselfmastery/FUNCUT`  
**Plan branch:** `auto-edit/20-session-completion-2026-11-21`  
**Planning base (observed):** `main @ b644a7283df0fac49d7eccdba9888816d8f5537b`  
**Initial audit date:** 2026-11-21  
**Scope:** Forge AUTO EDIT, Forge Director, the in-process Auto Edit runner, MCP tools, signal/voice/script/footage/motion/audio/caption/export integration, tests and acceptance. Do not broaden into unrelated editor features unless a dependency blocks this mission.

## Ground rules

1. Never equate a tool existing, being allowlisted, or appearing in a prompt with the Auto Edit path actually calling it and using its result.
2. Audit the two routes separately: (a) Rust `auto_edit_program` / `concat-director::Runner`; (b) Forge Director harness launched by the Forge pane. Record which route owns each decision.
3. Every signal or analysis result must have a traceable consumer and must affect a named edit decision, or be explicitly marked intentionally unused with a reason.
4. Every stage must have a typed result, truthful progress, bounded timeout/retry behavior, cancellation semantics, and durable evidence where applicable.
5. Never silently substitute a degraded path. Report which path ran, which tools were unavailable, which steps were skipped, and why.
6. Preserve licensing and safety gates. Monetized edits must not import unlicensed media. Never close or cancel user-owned work through an unsafe director allowlist.
7. No fabricated green checks or acceptance claims. Separate static/code evidence, unit tests, integration tests, CI, and real-machine acceptance.
8. Work on a feature branch, commit coherent units, open/update PRs, and merge only after required checks and review are green. Do not force-push or write directly to `main`.
9. At every session, update this plan's status and the running hand-off section. A later `continue` resumes from the first incomplete session.
10. A 20-session budget is a work plan, not a guarantee that every hardware-dependent acceptance test can be completed without the target machine, credentials, sample media, or owner action.

## Baseline facts observed on 2026-11-21

- `main` was observed at `b644a7283df0fac49d7eccdba9888816d8f5537b`.
- PR #186 (`signals/fifty-ears`, head `b5847ac30907d68197f87e92a2be76acc34672c0`) is **open and unmerged** at audit time; it changes 47 files and contains 18 commits.
- PR #186's current checks at audit time: Geneva passed; Linux x86_64 and aarch64 app builds passed; dependency workflow passed; format/locales/models passed; analysis senses passed; Engine tests and lints was still in progress. The PR must not be called merged or fully green until the exact head's required checks finish and GitHub confirms merge.
- PR #186 describes substantial voice-signal consumer wiring and a Forge signal lab. Treat those changes as branch-only until merged.
- `AGENT_STATE.md` and `IMPLEMENTATION_LEDGER.md` contain a Session 8 full-power pass. Reconcile their claims against current source and PR #185's merge before repeating work.
- `docs/AUTO_EDIT_AUDIT.md` and `docs/DIRECTOR-INTEGRATION-PLAN.md` contain earlier path-specific findings. They are valuable evidence/history, but must be revalidated against the latest main and PR #186 before implementation.
- The repo's `AGENTS.md` states 113 catalogue tools and 105 director-served tools at its current main snapshot; the allowlist, `.forge/mcp.toml`, docs and tool catalogue must be reconciled to exact source rather than relying on stale counts.

## Definition of complete

The button is complete only when all of the following are true:

- The UI starts the intended workflow, identifies its input source (script or existing voiceover), validates prerequisites, and never silently runs a different route.
- The script, transcript, voiceover timing, speaker turns, prosody, emotion, pauses, emphasis, and all measured voice signals are consumed by explicit, tested decisions.
- Footage is inspected before selection; B-roll is relevant, continuity-checked, correctly oriented/reframed, rights-cleared for the project's monetization state, and placed at measured story gaps.
- Timeline pacing, hook, chapter/story beats, cuts, transitions, captions, motion graphics, grade, music, ducking, SFX and silence handling are coherent and derived from the actual edit evidence.
- Forge Director and MCP tools coordinate without losing fan-stage replies, confusing sibling/window projects, exceeding tool budgets, or silently skipping failed capabilities.
- The run supports truthful progress, cancellation, recovery/resume or a documented safe restart, and no false success state.
- Export is reopened/probed and checked against the actual timeline and source speech. QC returns machine-readable pass/fail/skipped evidence with actionable reasons.
- The final media is actually watched/inspected; the system does not claim quality based only on a prompt or a zero exit code.
- Tests cover success, missing keys/tools, provider failure, empty/poor footage, short takes, speaker changes, signal absence, interrupted export, cancellation, retry, resume and output mismatch.
- UI, docs, tool catalogue, allowlist, ledger and release notes agree with observed behavior.

## The 20 sessions

Each session ends with a commit or an explicit no-code/evidence-only checkpoint. If a session uncovers a blocker, fix it before moving on; session numbers are priorities, not permission to skip dependencies.

| Session | Focus | Required work and exit evidence | Status |
|---:|---|---|---|
| 1 | Baseline and PR reconciliation | Re-read exact main and every open Auto Edit PR; inspect CI/checks, changed files and merge base; reconcile Session 8 claims, old audit docs and PR #186; publish a verified path-by-path gap matrix. | in progress |
| 2 | End-to-end architecture map | Trace Forge click -> UI callback -> Rust pane -> Director handoff/fallback -> MCP server -> backend -> timeline/project -> export -> result UI. Draw actual call graph and identify duplicated/competing routes. | pending |
| 3 | Tool and signal contract audit | Enumerate every MCP tool available/required/optional; map every script/transcript/voice/signal/footage/motion/audio result to a named consumer and edit decision; check context budgets and truthful skipped reasons. | pending |
| 4 | One authoritative orchestration contract | Define a shared run/job identity, source project/sibling project identity, typed stage outputs, cancellation token, errors, progress events and evidence log. Avoid parallel implementations drifting. | pending |
| 5 | Director completion and acceptance receipt | Ensure Director success means a valid completed edit, not exit code zero; write/read the same structured acceptance receipt as the runner; verify export path, size, streams, timeline identity and QC verdict. | pending |
| 6 | Progress, cancel, retry and resume | Replace fragile prose-derived progress where feasible with structured events; ensure cancel is safe, edits are recoverable, retries reuse the correct sibling/project, and restart behavior is explicit and tested. | pending |
| 7 | Script/voiceover alignment | Verify script-to-TTS and existing-voiceover routes; preserve source timing; align transcript words to sped/trimmed timeline; detect mismatches, missing speech and stale transcripts. | pending |
| 8 | Voice intelligence and signal spending | Audit all signal families/readings, confidence and basis; connect useful signals to cuts, pacing, transitions, captions, speaker seams, music ducking and repair; test every consumer and intentionally-unspent reading. | pending |
| 9 | Footage intelligence and B-roll | Derive story/gap slots from transcript and brief; inspect contact sheets; score relevance/quality/duplicates; apply continuity and aspect/reframe hints; enforce license/monetization gates; prove placed footage in timeline. | pending |
| 10 | Timeline and story construction | Validate hook, pacing, dead-air handling, shot lengths, dialogue protection, speaker turn seams, narrative progression, B-roll coverage and continuity against actual timeline state. | pending |
| 11 | Captions and typography | Verify word-level timing, punctuation, breath-aware grouping, safe filler removal, stress/emphasis, aspect-aware layout, readability, safe areas and multilingual fallbacks; compare captions to final audio. | pending |
| 12 | Motion graphics and transitions | Inspect motion presets/frames before bake; choose motion only where story evidence warrants it; use Geneva/linked motion where supported; verify transitions, no repeated seams, sync points and graceful unsupported-feature refusal. | pending |
| 13 | Music, SFX and audio finishing | Select licensed fitting bed; shape ducking from actual speech/gaps; place SFX at measured accents; prevent clipping, masking, abrupt tails and over-processing; verify loudness/true peak and voice intelligibility. | pending |
| 14 | Color and visual consistency | Ensure grade decisions follow story/acts and measured footage; prevent exposure/white-balance jumps; preserve skin/subject appearance; test no-op and unsupported cases. | pending |
| 15 | Preview, critique and revision loop | Make preview inspectable before final export; run critique on actual rendered frames/audio; revise bounded per-slot issues; avoid endless loops; record exhausted attempts and reasons. | pending |
| 16 | Export and machine-readable QC | Verify codec/container, streams, duration, dimensions, fps, audio loudness, transcript match, black/silence/freeze checks, missing media, timeline/export correspondence and platform targets; gate success on required checks. | pending |
| 17 | UI truth and user control | Ensure button availability, setup/preflight, selected source, stage/progress, warnings, cancel/retry, result links and final state reflect backend truth; localize all new strings. | pending |
| 18 | Reliability, providers and low-resource operation | Test missing/invalid API keys, provider timeouts/rate limits, retries/fallback, MCP disconnect, tool budget, cache, memory/CPU/disk constraints and 1080p editing; no silent downgrade. | pending |
| 19 | Full integration regression and CI | Add deterministic fixture projects and stubbed provider/MCP scenarios; test both routes and their differences; run fmt, clippy, crate tests, Python selftests, locales, Geneva, app build and security lanes on exact PR head. | pending |
| 20 | Real acceptance, documentation and release decision | Run a real script/voiceover + footage project with real configured providers on target Linux hardware; inspect the exported video and evidence; close only proven issues; update ledger/known issues/docs; publish PR and final readiness report. | pending |

## Session 1 audit checklist

- [x] Confirm repository and default branch.
- [x] Confirm main SHA and open PR #186 state.
- [x] Inspect PR #186 changed-file list.
- [x] Inspect PR #186 workflow/check status.
- [ ] Read exact current main source for both orchestration paths.
- [ ] Read PR #186's relevant diffs and verify each claimed signal consumer.
- [ ] Reconcile PR #185 merge contents with Session 8 ledger.
- [ ] Produce a code-evidence matrix: done on main / PR-only / missing / unproven.
- [ ] Update this file with the exact checkpoint and next actions.

## Evidence levels

- **E0 — claimed:** docs, prompt text, issue/PR body, or a developer statement only.
- **E1 — source present:** implementation is in the inspected tree, but not proven to execute.
- **E2 — automated proof:** regression/unit/integration tests exercise the real function/path.
- **E3 — CI proof:** required workflows pass on the exact commit/PR head.
- **E4 — product acceptance:** real project, real media/provider setup, actual output inspected, target hardware and user workflow exercised.

Never promote a finding to a higher level without its evidence. “Ready” requires E3 for code and E4 for the user-facing end-to-end workflow.

## Session hand-off / resume point

**Last completed action:** initial live repository, PR and CI reconnaissance.  
**Current session:** Session 1 — baseline audit.  
**Next action:** inspect the exact main and PR #186 source for Forge orchestration, Auto Edit stages, MCP tool dispatch, signal consumption and export verification; then fill the evidence matrix.  
**Do not start Session 2 implementation until Session 1's gap matrix is complete and PR #186's merge status is reconciled.**

## Progress log

- 2026-11-21: Created this 20-session plan from live GitHub state. PR #186 is open; 6 of 7 check runs completed successfully at the observation time, with Engine tests and lints still running. No claim of merge or completion is made.


## Session 1 — interim source findings (2026-11-21)

This section supersedes older gap statements where the current source proves the work has since landed. It is an interim audit, not a claim that every PR #186 file has been reviewed or that the workflow has E4 acceptance.

### Verified on current main (E1 source evidence)

| Area | Current-main finding | Evidence / implication |
|---|---|---|
| Two execution routes | Confirmed: Forge prefers the external Director when `director_ready` succeeds; otherwise it warns and runs the in-process fixed program. | `src/crates/concat/src/panes/forge.rs::auto_edit_start`. Test both routes separately. |
| Fixed program scope | The Rust program is no longer the old 8/12-stage post-production-only path. Its stage register contains 17 stages: handoff, repair, transcription, voice signals, understanding, plan, gather, timeline, captions, motion, music, SFX, preview, revise, export, verify and watch. | `src/crates/concat-mcp/src/director_program.rs::auto_edit_program`; `src/crates/concat-director/src/stage.rs::Stage::ALL`. Old audit documents describing the 8/12-stage program are stale. |
| Creative Director path | The prompt explicitly instructs the Director to measure the voice, create and validate a plan, inspect B-roll, use motion, word-group captions, grade/music/SFX, preview, revise and inspect the exported file. | `auto_director_prompt` in `forge.rs`; the skill's 25-phase workflow. Prompt instructions are E0/E1 until a real run proves them. |
| Director success gate | A zero exit code starts independent acceptance; it does not itself declare success. The pane probes the export, measures loudness, watches the actual render, critiques its sheet, revalidates a saved plan where present, and compares rendered speech with the voiceover mapping; it writes `auto-edit-receipt.json`. | `src/crates/concat-mcp/src/acceptance.rs` and `forge.rs::auto_director_ended/auto_director_checked`. This refutes the old audit's P0 claim that the Director route trusts exit code alone. |
| Fixed-run verification | The fixed program has explicit preview, revision, export, verify and watch stages, with reply gates and durable runner state. | `director_program.rs`, `concat-director/src/stage.rs`, `concat-director/src/runner.rs`. Inspect individual gates and error policy during later sessions. |
| B-roll | The fixed program has a Gather stage that searches the configured licensed live provider, fetches and contact-sheet-inspects candidates, runs continuity checks, then imports/places and styles a chosen clip. | `director_program.rs` Gather implementation and `the_broll_stage_imports_only_licensed_live_stock` regression test. The old audit's claim that Gather is absent is stale on this main snapshot. |
| UI fallback behavior | Director-first routing is explicit. If Director setup is unavailable, the UI reports why and then uses the fixed program; it does not pretend the fallback was the Director. | `forge.rs::auto_edit_start`. Verify the wording and all missing-tool/key cases in UI acceptance. |
| Voice-signal improvements | PR #186 adds further parsed readings and consumption tests, including phrase boundaries, pitch, clipping/safety, hum, spectral, rhythm and speaker evidence. | These are **PR-only**, not current-main evidence, until #186 merges. Review the entire signal diff and exact-head CI before treating them as shipped. |

### Remaining risks / not yet proven

1. **E4 acceptance is still open.** `KNOWN_ISSUES.md` explicitly says a real AI Director brief-to-final-export project still needs acceptance. Unit tests and CI cannot prove the actual target laptop/provider workflow.
2. **Director progress is still inferred from stdout prose.** `DIRECTOR_PHASES` and `director_phase` in `forge.rs` search for phase headings/keywords. This is less reliable than typed events and should be replaced or hardened with structured progress.
3. **Director cancellation/recovery differs from the fixed runner.** The external child is terminated on cancel; the sibling is retained and retry reuses it, but there is no equivalent stage checkpoint/resume contract for the Director's partial work. The fixed runner has its own resume record. Define and test safe recovery rather than assuming parity.
4. **The signal contract still needs exact-head reconciliation.** PR #186's 50-reading additions must be checked against the catalogue, parser, per-reading consumer guard, named decision policy, confidence/basis, tests, docs and Forge UI catalogue. Do not accept a family count alone as proof.
5. **The Director route and fixed route have different creative contracts.** The fixed route has deterministic plan/gather/motion/revision stages; the external Director has the skill and harness. Confirm both satisfy the same essential acceptance receipt and licensing/QC guarantees, while allowing their creative methods to differ.
6. **The UI's Director progress is not the same as actual work progress.** A log line can mention a phase without completing it; stage fraction must not imply a completed stage until structured evidence supports it.
7. **Target-machine constraints remain.** The repo is tuned for small Linux systems, but real render time, RAM, provider latency, stock downloads and 1080p playback/export need measured acceptance on the intended machine.

### Corrected audit status

- `docs/AUTO_EDIT_AUDIT.md`: historical findings; its claims about missing Director export acceptance and missing fallback Gather/Motion/Captions/Preview/Revise must be reconciled before use.
- `docs/DIRECTOR-INTEGRATION-PLAN.md`: historical plan; its old 12-stage and no-Gather statements are stale against current main.
- `KNOWN_ISSUES.md`: use as the current limitations register; its E2E acceptance item remains the principal product-level blocker.
- `AGENT_STATE.md` / `IMPLEMENTATION_LEDGER.md`: use as historical session evidence, but verify each claim against current code and exact PR/CI state.

### Session 1 checklist update

- [x] Read current-main source for the Director-first route and fixed-run fallback.
- [x] Read current-main Auto Edit stage register and representative Gather, preview/revise and verification implementations.
- [x] Read current-main Director acceptance receipt/check implementation.
- [x] Inspect PR #186 diffs for signal parser/consumer changes in the main affected crates.
- [ ] Finish line-by-line review of all PR #186 changed files and reconcile its test evidence.
- [ ] Confirm PR #185 merge commit and map every Session 8 claim to current source/tests.
- [ ] Produce final Session 1 gap matrix including tool coverage, provider setup, failure policy and route parity.

### PR/checkpoint

- Planning document commit: `1870ec31344332de41aa7ef9908041d8b04bebdc`.
- Planning PR: [#187](https://github.com/coreselfmastery/FUNCUT/pull/187) — open; CI was still running at the last status check.
- Implementation PR under review: [#186](https://github.com/coreselfmastery/FUNCUT/pull/186) — open and unmerged; six of seven check runs were successful and Engine tests/lints remained in progress at the last status check.

**Resume next:** complete the remaining Session 1 reconciliation; do not implement a feature already present on main or PR #186.
