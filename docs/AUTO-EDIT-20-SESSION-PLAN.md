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
