# TEST_EVIDENCE.md — what was actually run, and what it said

Every entry names the commit and the command. CI results are read from the
GitHub check-runs API for the exact SHA.

## Baseline `ab7c0704917d6f3181b1fb847fead86dc6392fcf`

- GitHub Actions (7 checks): all **success** — Engine tests and lints; The
  analysis senses against real fixtures; Format, locales and models; The
  dependency dance, pinned and green; linux-aarch64; The multi-layer
  example, rendered and inspected; linux-x86_64.
- Local: `cargo test -p concat-motion` → **426 passed, 0 failed, 5 ignored**
  (+6 doc-tests passed). Machine: 2 cores / 4 GB.

## `54e13922` (PR #76 branch + rustfmt fix)

- Local: `cargo fmt --check` clean; `python3 scripts/locales.py --check`
  clean (25 locales, 0 missing, 0 stale).
- GitHub Actions: all five checks **success** — Format/locales/models,
  analysis senses, multi-layer example, dependency dance, Engine tests
  and lints. **PR #76 merged at `6dafbeee`.**

## `feat/completion-2026-11-10` units (A/C/D/F/G + ledger)

- Local at each commit: `cargo test -p concat-motion` → 451 passed
  0 failed 5 ignored (+6 doc-tests); `cargo test -p geneva-adapter` →
  42 passed; `cargo test -p concat-export` → new read-back test green
  (4 pre-existing no-GPU failures reproduced on the UNMODIFIED tree —
  the sandbox has no lavapipe; CI passes them);
  `cargo fmt --check` and `cargo clippy -p <crate> --all-targets --
  -D warnings` clean for every touched crate.
- `concat-mcp` context budget measured: 241,881 whole catalogue (cap
  242,000), 228,233 director surface over 99 tools — both under.
- Branch CI: pending PR creation (push filter is `[main]`; PRs carry it).

## Local build environment reproduction (for reproducibility)

`bash /home/z/my-project/scripts/setup-build-env.sh` fetches the exact
digest-pinned FFmpeg 8.1.3/ONNX 1.28.0/sherpa 1.13.7 from
`scripts/ci/deps.lock` (all sha256-verified), then
`source /home/z/my-project/scripts/funcut-env.sh` provides the env the CI
engine job uses. No system packages are installed (no root): clang 19 and
the dev sysroot come from extracted Debian debs under
`/home/z/my-project/clang-deb` and `/home/z/my-project/sysroot`, cmake from
the portable 3.31.6 tarball.

The `concat` GUI crate cannot compile on this box (rustc OOM at 4 GB even
in check mode, `-j1`, no incremental, debuginfo 0). Its validation is CI's;
this is recorded rather than hidden.
