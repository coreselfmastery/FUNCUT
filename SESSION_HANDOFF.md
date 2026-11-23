# SESSION_HANDOFF.md — exact resume point

If a new session starts: read AGENT_STATE.md, IMPLEMENTATION_LEDGER.md,
TEST_EVIDENCE.md and KNOWN_ISSUES.md first, then this file, then:

```sh
git -C /workspace/project/FUNCUT fetch origin
git -C /workspace/project/FUNCUT checkout main
git -C /workspace/project/FUNCUT pull --ff-only
```

## Where this stands — 2026-11-23, the `.15` release

- `main` at `573a661`; all seven checks on the latest commit are green
  (CI `45169594535`, Geneva `45169594539`, Build app `45169594545`).
- `v0.2.8-funcut.15` is the latest published release: ten assets, all
  downloadable, checksums matching GitHub's digests, manifest verified,
  and `install-linux.sh --list` sees `.15` first.
- The `.15` release workflow (`45163610747`) is red only at its Nix job:
  the hosted runner lost communication after about 1.5 hours, so the
  publish job was skipped. The release itself was published manually
  and verified. Rerun the Nix job to clear the workflow.
- The latest green engine lane counts 2,286 workspace tests (13 skipped),
  9 documentation tests and 30 gRPC tests: 2,325 in all. `README.md` and
  `docs/TESTING.md` now carry that count.
- Open product work is the target-machine acceptance list in
  `KNOWN_ISSUES.md`: proxy/Geneva performance, VAAPI on real hardware,
  motion-clip and AI Director end-to-end runs, and publishing provenance.
- Branch protection still needs required status checks; the API currently
  reports none.

## Resume checklist

1. **Rerun the `.15` Nix job** and confirm the release workflow. If the
   publish job runs, restore the release letter plus the `.14.. .15`
   commit list if the generated notes replace it.
2. **Verify the final `main` HEAD**: read its check-runs; all seven must
   be success on the exact SHA.
3. **Keep the docs honest**: `README.md`, `docs/TESTING.md`,
   `KNOWN_ISSUES.md`, `AGENT_STATE.md`, `TEST_EVIDENCE.md` and
   `IMPLEMENTATION_LEDGER.md` must name the same commit, count and
   release state.
4. **Configure `main` branch protection** to require the three `ci.yml`
   checks and enforce admins, matching `AGENTS.md` and
   `KNOWN_ISSUES.md` item 14.
5. **Do not claim target-machine acceptance or a real Director run**
   until one is recorded.

## Hard-won environment notes

- Use the checkout's own path (`git -C /workspace/project/FUNCUT ...` in
  this sandbox); do not assume the shell cwd.
- Background processes may be killed when a shell call returns: run long
  builds in the foreground with a generous timeout; cargo resumes
  incrementally across calls.
- `source scripts/funcut-env.sh` (repo-relative, after `bash
  scripts/setup-build-env.sh` once) before any cargo command in `src/`.
- `src/target` can reach 18 GB and leave little free space: check `df -h`
  and run `bash scripts/prune-dupes.sh` (hardlinks byte-identical
  rlib/rmeta duplicates; it reports only with `--dry-run`).
- This sandbox currently has no `cargo`/`rustc` and no `.ort-shared`;
  CI is the source of truth until the build environment is restored.
- Tool-output display can mangle brackets (a `[main]` once read as
  `ain]`): verify surprising bytes with `od -c` before declaring a bug.
- The token lives in the git credential store file; never write it into
  the repo. Do not print it.
