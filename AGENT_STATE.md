# AGENT_STATE.md — the mission's running state

Principal agent: the FUNCUT completion mission (2026-11-10 start).
Baseline `main` for the 2026-11-10 completion pass: `ab7c0704917d6f3181b1fb847fead86dc6392fcf` (CI: 7/7 green).
Subsequent verified `main` (11 November 2026): PR #78 and #79 are merged; PR #80 merged persistent native image caching; PR #81 merged the known-issues reconciliation; PR #83 merged the canonical Geneva master list; PR #84 merged Geneva fidelity-preserving engine selection and safe refusal; PR #85 merged the mixed-markup refusal explanation; PR #86 merged checksum-fail-closed Geneva installation and the reproducible 1080p30 benchmark. Latest observed `main` commit: `56bde7c85e9fedc817092e2fdad59a3c19aeb09f` (PR #88: per-timeline view-state isolation).

- PR #88 exact merge commit `56bde7c85e9fedc817092e2fdad59a3c19aeb09f` is fully green: CI 3/3, Geneva 2/2 and Build app Linux x86_64 + aarch64 2/2 (`43020200650`, `43020200640`, `43020200660`).
- Open engineering PR #89 adds a minimal native Motion-clip project command; PR #91 adds rasterlottie-backed native Lottie/dotLottie playback. Both are review branches only and require full CI/Geneva/Nix validation before merge.

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
- Build env scripts, in the repo and repo-relative (see TEST_EVIDENCE.md's
  reproduction section): `bash scripts/setup-build-env.sh` fetches the
  digest-pinned FFmpeg 8.1 dev / ONNX Runtime 1.30.1 / sherpa-onnx 2.0.2
  from `scripts/ci/deps.lock`, and `--debs` adds the no-root clang 19
  runtime and headers under `sysroot/`; `source scripts/funcut-env.sh`
  before any cargo command in `src/` exports the CI engine job's env
  (`FFMPEG_DIR`, `ORT_LIB_LOCATION`, `ORT_PREFER_DYNAMIC_LINK`,
  `LD_LIBRARY_PATH`, `SHERPA_ONNX_LIB_DIR`, `CARGO_PROFILE_DEV_DEBUG=0`,
  `CARGO_INCREMENTAL=0`, and `LIBCLANG_PATH` when the sysroot is there).
- Disk management: `bash scripts/prune-dupes.sh` hardlinks byte-identical
  rlib/rmeta duplicates under `src/target` (2.7 GB reclaimed once on the
  machine that record is from; `--dry-run` reports only). Run it when full.
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

## Session 8 log (2026-11-21) - the AUTO EDIT full-power pass

Branch `auto-edit/full-power-2026-11-21`, one unit per commit, every
non-GUI crate's tests green locally (concat-director 60/60,
concat-mcp 145/145, fmt + clippy clean; the watcher's mock self-test
passes; the GUI crate is CI's as always):

1. `fix(build-env)`: the SourceForge mirror's `/download` suffix broke
   `version_from_url` - the local reproduction path works again.
2. `fix(director)` - the P0 of this pass: fan stages' lane replies
   never reached the log every later derived step reads, so on the
   button's real path (run_with_lanes) the transcribe/voice-signal
   replies were invisible to the caption, B-roll, tighten, motion and
   SFX stages - the button shipped a nearly-unedited video while the
   same program without lanes edited fully. The merge now extends the
   log in group order; regression test included.
3. `fix(watcher)`: the five audio-repair readings now ride the
   `sense_audio_repair` door instead of bare tool names, and a
   monetized brief shops the licensed live shelves when OpenVid
   refuses. KNOWN_ISSUES item 11's watcher half closed.
4. `feat(auto-edit)` Gather: stock is looked at before it is chosen
   (stock_fetch + stock_contact_sheet, scored on chroma/contrast/
   detail, black/blown/letterbox/duplicates against), gated with
   footage_continuity before import (colour jumps swap to a measured
   alternate, category runs hold), reframed by the sheet's own crop
   hint, and the shelf search carries the project's orientation. The
   sheet gained a measured `warmth` tile field (sense.py).
5. `feat(auto-edit)` Captions: word groups of two to four on the
   recogniser's own word times (punctuation, breath gaps, four words
   are the seams; safe-remove fillers never show), a second pass
   accents the stressed words per word through styleTextRange, the
   caption band is aspect-aware (0.22 portrait / 0.35 landscape) and
   the hook's first three seconds are bigger.
6. `feat(auto-edit)` Transitions/SFX/hook: the mood map draws on the
   viral pack (zoom-blur-punch, hyper-zoom, glitch-warp...), the seam
   map too (whip-pan, slice-social, glitch-social, snap-zoom), every
   id is held to a package the catalogue ships (new test), accents
   land on their peak via the manifest's syncAt, hook peaks shop
   entrance energy, the hook's shot pushes harder, and the final
   watch critique carries the measured brief + autoSetup it always
   owed.
7. `feat(auto-edit)` Music/grade: the bed is the track that fits
   (shortest covering / longest partial), the duck is a volume shape
   that follows the measured lines (lifts only in gaps that can hold
   it), and the grade layer is keyed to the acts (hook full, body
   settled, cta lifted) through the package's own parameter.
8. `fix(forge)` (GUI crate, CI-validated): a cancelled Director run
   reports the stop and keeps the sibling instead of "failed"; a
   Director retry hands the SAME project over again instead of
   building "edit 2"; the button probes the harness synchronously
   when the pane was never shown, so it never falls back silently;
   the run prompt now spends the whole vocabulary (continuity gate,
   word-group captions, syncAt, per-act grade, transition breadth,
   hook, aspect, motion ops as authoring material).
9. `feat(mcp)`: `track_subject` joins DIRECTOR_ALLOWLIST (105 of the
   113) - a subject path a vertical reframe can follow is a creative
   tool. mcp.toml, FORGE.md, MCP.md, README and AGENTS.md say the
   same count, and the allowlist test holds the eight left off.

## Next actions

1. Finish review and exact-head CI/Geneva/Nix validation for PR #91's supported native Lottie/dotLottie renderer; merge only when all required checks pass, retaining explicit unsupported-feature diagnostics.
2. Extend PR #89's validated Motion-clip command foundation with the normal editor authoring path: composition picker, metadata editing, move/trim, save/reopen, undo/redo, preview and final-export acceptance.
3. PR #88 per-timeline view-state is merged and its exact merge-commit CI/Geneva/Build-app checks are green.
4. Add an explicit target-timeline picker and multi-job export queue with progress, cancellation, failure/retry handling and per-job timeline identity tests.
5. Integrate explicit Linux VAAPI encoding capability detection, selection and safe fallback; validate on supported Intel/AMD hardware.
6. Run the documented 1080p30 benchmark and proxy scrub/playback measurements on the actual antiX laptop; compare H.264 and MJPEG generation time, responsiveness and disk use. Do not infer results from hosted CI.
7. Run a real AI Director brief-to-final-export acceptance project and attach artifacts/logs.
8. Keep `KNOWN_ISSUES.md` and this state file synchronized with implementation and evidence; close blockers only after code, regression tests and required validation are present on `main`.

## Session 7 log (2026-11-16) - the deep review

Principal instruction: fix everything, in depth, no mistakes, use
parallel agents. Base: the PR #147 head `81439cc3` (the panic-site and
deps content), carried to main through **PR #149** after #147's merge
API call landed it into its stacked base branch instead of main - #147
is merged (into `deps/rustsec-clearance-2026-11-15`), and #149 carries
that merge the rest of the way to main. Branch
`fix/deep-review-2026-11-16` (37 fixes, 12 new regression tests, the
director self-test re-pinned; evidence in TEST_EVIDENCE.md, units in
IMPLEMENTATION_LEDGER.md) is this session's delivery.

1. Seven parallel read-only reviews (host, motion+geneva,
   media/render/export, mcp+director+server+api, project-side crates,
   the GUI crate, the director harness Python + CI/packaging); every
   finding re-verified against the code before a fix was written, and
   everything already fixed by the open PRs (the #146 deps, the #147
   panic sites) excluded.
2. All fixes implemented with regression tests where the crate's test
   surface allows (see the units table); the director harness's own
   60+-scenario mock suite passes green after the harness fixes, with
   the caption/kinetic/duck assertions re-pinned to the sped-timeline
   timebase and the real `signal` confidence key.
3. The two P0s were evidence bugs, not editor crashes: the
   forged-signalLog gate was dead (wrong reply key, plus a gate that
   needed a non-empty reading to refuse), and the watcher re-directed
   its own imports.
4. PR #141 (the RustSec audit workflow) still wants its seven
   per-ID ignores now that #146/#149 cleared thirteen advisories; PR
   #148 (docs) wants a rebase onto current main.
5. Local gates (this box): fmt, clippy -D warnings, and every non-GUI
   crate's tests green - including the GPU export/render lanes through
   a no-root lavapipe ICD (see TEST_EVIDENCE.md). The GUI crate is
   CI's, unchanged (OOM at 4 GB); its four fixes ride the CI engine
   lane on this branch.

## Next actions

1. Merge PR #149 (the deps/panic content to main) once its checks
   finish; then rebase #148 and give #141 its ignores (the seven
   sherpa-subtree IDs: zip 0367/0368/0369/0429, zstd 0391, zstd-safe
   0398/0403) per its own PR body's plan.
2. Merge this session's branch after its CI; the hardware/real-project
   acceptance items (KNOWN_ISSUES.md items 8-10) remain open and are
   unchanged by this session.
