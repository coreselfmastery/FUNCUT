# AGENT_STATE.md — the mission's running state

Principal agent: the FUNCUT completion mission (2026-11-10 start).
Baseline `main` for the 2026-11-10 completion pass: `ab7c0704917d6f3181b1fb847fead86dc6392fcf` (CI: 7/7 green).
Subsequent verified `main`: PR #78 merged at `3adb448c`; PR #79 then merged the native compositor at `de804c75`. PR #80 and PR #81 are follow-up work in progress and are not part of main until their checks pass and they merge.

## Mission

Complete the whole FUNCUT repository — editor, Geneva motion A–L, Director,
MCP surface — with real implementations, real tests, incremental commits,
and an honest feature ledger. No fake green, no doc-only features.

## Environment facts (verified this session)

- Build box: 2 cores, 4 GB RAM, 9.9 GB disk. The GUI `concat` crate OOMs
  rustc locally even in `cargo check` mode; it is validated by CI only.
- Local verification scope: every non-GUI crate. At last run:
  `concat-motion` 451/451, `geneva-adapter` 42/42, `concat-export` green
  beside 4 pre-existing no-GPU environmental failures (confirmed present
  on the unmodified tree; CI's lavapipe passes them), `concat-mcp` builds
  and its context-budget test passes (241,881 / 242,000 tokens, 119
  tokens of headroom).
- Build env script: `/home/z/my-project/scripts/funcut-env.sh` (FFmpeg 8.1
  dev digest-pinned, ONNX Runtime 1.28.0, sherpa-onnx 1.13.7, libclang 19
  from extracted debs + `/home/z/my-project/clang-lib`, portable cmake
  3.31.6, dev sysroot incl. libasound2t64 runtime, `CARGO_PROFILE_DEV_DEBUG=0`,
  `CARGO_INCREMENTAL=0`).
- Disk management: `/home/z/my-project/scripts/prune-dupes.sh` reclaims
  duplicate cargo artifacts (2.7 GB reclaimed once). Run it when full.
- Git credentials: store helper at `/home/z/.git-credentials-funcut`
  (admin token, never committed).
- CI triggers: `ci.yml` push filter is `[main]` (verified at byte level —
  an earlier "corrupted filter" reading was a display artifact, not a
  bug). Feature branches get CI through pull requests. Geneva and
  Build-app workflows also run on main pushes.

## Session 1 log (2026-11-10) — what landed

1. **PR #76 resolved**: its CI failed only `cargo fmt --check`; fixed
   with rustfmt (`54e13922`), all five checks went green, **merged**
   (merge commit `6dafbeee`).
2. **PR #73 resolved by proof**: `git merge-tree --write-tree` shows the
   branch's tree is **identical to main** — 100% of its 68 commits were
   re-landed via PR #74 (`df5150f5`). Nothing to recover; do not merge or
   squash it (a misread squash would read as a 77k-line deletion).
3. **Five implementation units on `feat/completion-2026-11-10`** (all
   locally tested, fmt+clippy clean, pushed; ledger section 47):
   - E303 keyframe-collision fix + regression test
   - Eleven named particle presets + patch semantics
   - Camera waypoint paths
   - Export output read-back verification
   - Docs truth pass (four corrected claims)
4. **Considered and declined** (principal's call, see ledger): a
   `motion_edit` MCP tool — `motion_bake` already persists ops-edited
   comps content-addressed, and the 242k-token catalogue cap has 119
   tokens of headroom; the edit-without-bake convenience is not worth
   burning a deliberately guarded budget.

## Next actions

1. PR #80: keep the native still-image decode cache alive across all frames
   in one bake; merge only after every required CI lane passes.
2. PR #81: reconcile the known-issues list, implementation ledger, and
   agent state with the code already merged in PRs #78 and #79.
3. Remaining engineering work: native Lottie playback, VAAPI integration
   with hardware validation, per-timeline view-state isolation, export
   queue and timeline picker.
4. Acceptance work that cannot be certified by hosted CI alone: measure
   1080p proxy scrubbing on the actual antiX laptop and run a real
   script-to-export Director project end to end. Do not claim hardware
   speedups or complete workflow readiness without those tests.
