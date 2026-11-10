# SESSION_HANDOFF.md — exact resume point

If a new session starts: read AGENT_STATE.md, IMPLEMENTATION_LEDGER.md,
TEST_EVIDENCE.md and KNOWN_ISSUES.md first, then this file, then:

```sh
git -C /home/z/my-project/FUNCUT fetch origin
git -C /home/z/my-project/FUNCUT checkout main
git -C /home/z/my-project/FUNCUT pull --ff-only
```

## Resume checklist

1. **PR #76**: if its engine check is green on `54e13922`, merge the PR
   (squash not needed — one logical fix commit + one fmt commit), then
   `git pull` main. If red, read the failing job's log from the check-run
   URL before touching anything.
2. **Branch**: create/checkout `feat/completion-2026-11-10` from main.
   All units below land there, one commit per unit, push after local
   tests pass.
3. **Unit A (E303 dedup)**: `src/crates/concat-motion/src/geneva.rs`,
   fn `keyframes` — dedup keys whose `seconds()` time equals the previous
   emitted time (later key wins, matching pulse.rs merge). Regression
   test: a track with two keys <0.5 ms apart must emit strictly
   increasing times. Run `cargo test -p concat-motion`.
4. **Unit C (particle presets)**: `particles.rs` — named `preset`
   configs (snow/rain/embers/sparks/dust/confetti/fireflies/energy/
   atmosphere), overridable fields, docs in MOTION.md's particle section
   + `.claude/skills/motion/SKILL.md` if it documents add-particles, and
   the `add-particles` op in `authoring.rs` takes `preset`. Tests for
   each preset's bounds and determinism.
5. **Unit D (waypoint camera)**: `camera.rs` — `CameraMove` gains
   waypoints; from/to stays sugar for two waypoints. Tests: N-waypoint
   paths bake into merged tracks; key times strictly increase.
6. **Unit F (export verification)**: `concat-export/src/lib.rs` after the
   final rename — probe the file (ffprobe via the existing decode path),
   compare duration/streams against the plan, warn (not fail) on anomaly
   with the numbers. Tests with a tiny synthetic export.
7. **Unit G (docs truth)**: fix `docs/MOTION.md` still-to-come list
   (PNG-sequence export EXISTS in flatten.rs since d07597c8), the
   `lib.rs:19` "already shipped" line, geneva-adapter's Lottie claim,
   `docs/FEATURES.md:315` Phase 1c omission; remove dead `BeatGrid` /
   `Value::Size` / `BakeRequest.crf` only if removal is clean (they are
   pub API — if not removable, document why they stay).
8. **Unit I (native engine)**: only after A/C/D/F/G are green on CI.
   Scope: `impl CompEngine` over tiny_skia (in-tree via concat-media's
   resvg), drawing Group/Rect/Ellipse/Path(fill, stroke, dash)/Text/
   Image/nested comps, masks (feathered rect/ellipse), 9 blends. Smoke
   tests assert non-blank renders and pixel-level expectations on
   crafted comps. Update MOTION.md + lib.rs table to the truth.
9. **Final**: merge the branch to main via PR (or direct push if faster
   and allowed), verify final main SHA's check-runs are green, update
   all five state files, write the final report with evidence.

## Hard-won environment notes

- ALWAYS `git -C /home/z/my-project/FUNCUT ...` — the shell's cwd resets
  between calls to `/home/z/my-project`, which is a *different* git repo.
- Background processes are killed when a Bash call returns: run long
  builds in the foreground with `timeout 600000`; cargo resumes
  incrementally across calls.
- `source /home/z/my-project/scripts/funcut-env.sh` before any cargo
  command in `src/`.
- The token lives in the git credential store file; never write it into
  the repo. Do not print it.
