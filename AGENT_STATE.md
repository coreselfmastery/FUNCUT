# AGENT_STATE.md — the mission's running state

Principal agent: the FUNCUT completion mission (2026-11-10 start).
Baseline `main`: `ab7c0704917d6f3181b1fb847fead86dc6392fcf` (CI: 7/7 green).

## Mission

Complete the whole FUNCUT repository — editor, Geneva motion A–L, Director,
MCP surface — with real implementations, real tests, incremental commits,
and an honest feature ledger. No fake green, no doc-only features.

## Environment facts (verified this session)

- Build box: 2 cores, 4 GB RAM, 9.9 GB disk. The GUI `concat` crate OOMs
  rustc locally even in `cargo check` mode; it is validated by CI only.
- Local verification scope: every non-GUI crate (`concat-motion` 426 tests
  green locally at baseline; `concat-export`, `concat-mcp`, `concat-project`
  build locally). Full workspace validated by GitHub Actions.
- Build env script: `/home/z/my-project/scripts/funcut-env.sh` (FFmpeg 8.1
  dev from `scripts/ci/fetch.sh`, ONNX Runtime 1.28.0, sherpa-onnx 1.13.7,
  libclang 19 via extracted debs, portable cmake 3.31.6, dev sysroot for
  alsa/udev/ssl/sqlite/egl/etc, `CARGO_PROFILE_DEV_DEBUG=0`).
- Git credentials: store helper at `/home/z/.git-credentials-funcut` (admin
  token, never committed).

## Session log

### Session 1 — 2026-11-10

- Audited the live repository at baseline (three parallel audit agents +
  principal verification; see IMPLEMENTATION_LEDGER.md).
- PR #76 (four audit bugs): its CI failed only `cargo fmt --check`
  (`panes/mod.rs:135`). Fixed with rustfmt, pushed `54e13922` to the PR
  branch. All other checks green; engine tests were still running at time
  of writing. ACTION: merge when green.
- PR #73: verified with `git merge-tree --write-tree` that merging
  `origin/feat/director-story-led-integration-verified` into `main`
  produces a tree **identical to main** (empty diff) — 100% of the branch's
  68 commits were re-landed on main via PR #74 (`df5150f5`). Nothing to
  recover; branch is safe to delete. DO NOT squash-merge it (a naive
  squash reads as a 77k-line deletion of main's newer work).
- Verified on main: 10 chart kinds (Bar/Line/Area/Donut/Dot/Compare/
  Progress/Candlestick/Timeline/Table) — the audit agent that reported
  them missing had read the PR #76 branch checkout, not main.
- Verified `export_cancel`/`project_close` are excluded from
  `DIRECTOR_ALLOWLIST` **deliberately** (tested refusal; "a harness must
  never call" per tools.rs:6293). Not a bug; do not "fix".

## Next actions

1. Merge PR #76 once its engine job is green.
2. Work branch `feat/completion-2026-11-10`: implement units A–G of the
   implementation ledger, one commit each, local tests before push.
3. See SESSION_HANDOFF.md for the precise resume point.
