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
- Branch CI: **all five checks success on PR #78's head `4b9f8f81`** —
  Format/locales/models; analysis senses; the dependency dance; Engine
  tests and lints; the multi-layer example rendered and inspected (the
  Geneva sidecar lane). **PR #78 merged at `3adb448c`.** The merge
  commit's own main-push run and this final state push's run are the
  last two records; both are docs-equivalent content over a green tree.

## Historical local build environment (not a clean-clone recipe)

> **Portability note:** this section records a previous developer machine, not a clean-clone setup. The absolute `/home/z/my-project/scripts/setup-build-env.sh` and `funcut-env.sh` paths are not present in the repository. Do not copy these commands into a new checkout. The reproducible packaged build is defined by `.github/workflows/build-app.yml`, which fetches pinned assets from `scripts/ci/deps.lock` using `scripts/ci/fetch.sh`. A portable local setup script is not currently shipped.



`bash scripts/setup-build-env.sh` (in the checkout, repo-relative) fetches
the exact digest-pinned FFmpeg 8.1.3/ONNX 1.28.0/sherpa 1.13.7 through
`scripts/ci/fetch.sh` (all sha256-verified against `scripts/ci/deps.lock`)
and unpacks them at the repo root the way the CI engine job does, then

> Version note (18 November 2026): this recorded reproduction used ONNX Runtime 1.28.0. The 1.28.3 GitHub and SourceForge download attempts returned HTTP 404 in hosted CI. The current candidate is ONNX Runtime 1.30.1; its newer C API retains the requested v1.28 table (GetApi returns a requested supported API version). This historical local run is not evidence for the new runtime. The PR's CI must validate both 1.30.1 downloads, engine tests, Geneva, the x86 ISA scan and both packaged architectures.
`source scripts/funcut-env.sh` exports the env the CI engine job uses
(`FFMPEG_DIR`, `ORT_LIB_LOCATION`, `ORT_PREFER_DYNAMIC_LINK`,
`LD_LIBRARY_PATH`, `SHERPA_ONNX_LIB_DIR`). No system packages are
installed (no root): `bash scripts/setup-build-env.sh --debs` also
downloads the clang 19 runtime and builtin headers as Debian packages
(`apt-get download`, nothing installed) and unpacks them under `sysroot/`,
where `funcut-env.sh` picks them up as `LIBCLANG_PATH` for the bindgen
crates. Verified on a clean clone of `dd109b8d` plus this change
(2 cores / 4 GB / no root): `cargo check -p concat-media` green against
the pinned FFmpeg, and `cargo test -p concat-host --lib` green (198/198)
with the alsa pkg-config entry the sysroot provides on such a box;
cmake for whisper.cpp remains the system's to give (the script names it).

The `concat` GUI crate cannot compile on this box (rustc OOM at 4 GB even
in check mode, `-j1`, no incremental, debuginfo 0). Its validation is CI's;
this is recorded rather than hidden.

## `fix/deep-review-2026-11-16` (the 2026-11-16 deep-review session)

Branch head of this session, base `81439cc3` (the PR #147/#149 content).
The same box class as the record above (2 cores / 4 GB / no root; the
sysroot carries clang 19, alsa, and - new this session - the Vulkan
loader plus Mesa's lavapipe ICD, unpacked the same no-root way with
`apt-get download` + `dpkg-deb -x`, driven through `VK_ICD_FILENAMES`
with the ICD json's `library_path` rewritten to the sysroot copy).

- `cargo fmt --all --check`: green.
- `cargo clippy -p concat-motion -p concat-host -p concat-export -p
  concat-render -p concat-mcp -p concat-api -p concat-project -p
  concat-effects -p voice-signals --all-targets -- -D warnings`: green.
- `cargo test -p concat-motion`: 535 passed, 0 failed, 5 ignored (the
  Geneva-sidecar lanes; no sidecar binary on this box, unchanged
  before this session). The new regressions among them:
  `a_translucent_child_of_a_group_composites_once`,
  `a_matte_source_inside_a_group_draws_nothing_of_its_own`,
  `a_positionless_child_of_a_positioned_group_sits_at_the_group`, the
  morph `MoveTo`/`LineTo` assertions inside
  `a_morph_between_disagreeing_shapes_resamples_both`, and
  `soft_light_follows_the_w3c_table_it_cites`.
- `cargo test -p voice-signals`: 73 passed, 0 failed.
- `cargo test -p concat-effects`: 23 passed, 0 failed (incl.
  `resolved_values_are_held_to_their_parameter_ranges`).
- `cargo test -p concat-project`: 214 passed, 0 failed (incl.
  `a_detached_sound_survives_the_slot_being_filled`).
- `cargo test -p concat-api` (lavapipe): 43 passed, 0 failed (incl.
  `the_durable_ledger_adopts_its_rows_oldest_first`).
- `cargo test -p concat-host`: 198 lib passed + 17 + 1, 0 failed, 4
  ignored (unchanged GPU/environment lanes; incl.
  `one_turn_per_sound_queues_askers_behind_each_other`).
- `cargo test -p concat-export` (lavapipe): 52 passed, 0 failed, 1
  ignored (incl. `a_clip_marked_preserve_alpha_keeps_its_background_layer`).
- `cargo test -p concat-render` (lavapipe): 103 passed, 0 failed, 1
  ignored.
- `cargo test -p concat-mcp`: 67 lib + 7 e2e + 1 doc passed, 0 failed.
- `python3 scripts/auto_director.py --self-test`: exit 0, no FAIL
  lines - the full mock-suite (run, resume, revision, gates, verify
  record) green against the fixed harness, with the assertions
  re-pinned to the sped-timeline timebase and the real `signal`
  confidence key.
- The `concat` GUI crate remains CI's on this box (rustc OOM at 4 GB,
  unchanged); its four fixes this session (wave/speed, mute/muted,
  apply_within flush, monitor quality clear) are validated by CI's
  engine lane on the branch.
