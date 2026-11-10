# SESSION_HANDOFF.md — exact resume point

If a new session starts: read AGENT_STATE.md, IMPLEMENTATION_LEDGER.md,
TEST_EVIDENCE.md and KNOWN_ISSUES.md first, then this file, then:

```sh
git -C /home/z/my-project/FUNCUT fetch origin
git -C /home/z/my-project/FUNCUT checkout main
git -C /home/z/my-project/FUNCUT pull --ff-only
```

## Where this session stands

- `main` at `6dafbeeeeae9a1f1db93845bfac8abcd003e1ace` (PR #76 merged;
  its engine job was the last check still running at handoff — every
  other check green, and the merged content equals PR #76's CI-green
  head plus a main that was 7/7 green at `94ebfdde`).
- **PR #78 open** (`feat/completion-2026-11-10` → `main`), head
  `68dbf225`. At last check: Format/locales/models, analysis senses,
  dependency dance and the Geneva multi-layer render all **success**;
  "Engine tests and lints" **in progress**.
- All local gates already passed for every unit on the branch (see
  TEST_EVIDENCE.md).

## Resume checklist

1. **PR #78**: when its engine check is green, merge it (merge commit,
   matching repo convention). If RED: read the failing job's log from
   the check-run URL first; every unit is locally tested, so a failure
   is most likely an environment-only lane (the repo's CI has no
   known gaps against this branch's diff).
2. **Verify the final main HEAD**: fetch, read its check-runs (all seven
   must be success), `git log --oneline -8` to see the delivery.
3. **Final state-file update** on main: mark units merged with their
   final SHAs, append the CI evidence, commit, push.
4. **Final report** per mission section 14: starting/final main SHAs,
   every commit pushed, PRs resolved (#76 merged, #73 proven re-landed,
   #78 the delivery), the five units, test results, CI evidence,
   known limitations (KNOWN_ISSUES.md), and the honest statement of
   what the mission's full A–L scope still holds open.

## The stretch unit, if a future session is authorized

The native `CompEngine` (KNOWN_ISSUES.md #1) is the repo's own declared
gap and the mission's area-A unblock. Scope it as: a tiny_skia-based
CPU raster behind a cargo feature in `concat-motion` (tiny_skia is
already in-tree via concat-media's resvg), drawing Group / Rect /
Ellipse / Path (fill, stroke, dash draw-on) / Image / nested comps /
masks, with the 9 blends — text needs the shaped-run path and can
follow second. Wire it into `EngineKind::Native`'s existing, tested
fallback plumbing (`engine.rs:99-142`), honour the bake-cache contract
(`bake_cache_hit`, manifest written last), and update MOTION.md's
engine table and `lib.rs` to the new truth when it renders. The
examples/comps family and the golden-frame lanes are the validation
surface. Do NOT claim area A complete until paths actually render.

## Hard-won environment notes

- ALWAYS `git -C /home/z/my-project/FUNCUT ...` — the shell's cwd resets
  between calls to `/home/z/my-project`, which is a *different* git repo.
- Background processes are killed when a Bash call returns: run long
  builds in the foreground with `timeout 600000`; cargo resumes
  incrementally across calls.
- `source /home/z/my-project/scripts/funcut-env.sh` before any cargo
  command in `src/`.
- Disk fills at ~5.8 GB of target/ artifacts: run
  `/home/z/my-project/scripts/prune-dupes.sh` (reclaims duplicate
  cargo artifacts; 2.7 GB once).
- Tool-output display can mangle brackets (a `[main]` once read as
  `ain]`): verify surprising bytes with `od -c` before declaring a bug.
- The token lives in the git credential store file; never write it into
  the repo. Do not print it.
