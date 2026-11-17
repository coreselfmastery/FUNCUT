# FUNCUT repository-wide bug audit — current-main supplement

Audit date: 2026-11-14  
Repository: `coreselfmastery/FUNCUT`  
Audited base: `main` at `dd109b8d056c0b33c1fcf78fb5021078d71f89c7` (PR #131 merge)  
Scope requested: functional correctness, feature wiring, incomplete implementations, runtime failure paths, data integrity, API/schema contracts, tests and documentation. **Performance benchmarking and license/compliance review are deliberately excluded.**

This report supplements `docs/BUG-AUDIT-FINAL.md`. That earlier report was written against `21d136c3972ad87033992968b987b3b0c6ebb5c7`; its statuses for PRs #130–#134 must not be read as current-main truth.

## Status update — 2026-11-17 (verified against `main` `c05f743a`)

The findings below are historical. Read against the current tree, they
stand as:

| ID | Was | Now |
|---|---|---|
| FUNCUT-001 | queued-proxy race, fix PR open | **Fixed on `main`** — `startup_io.rs` rechecks the gate in the worker-start wrapper; the regression test is present. |
| FUNCUT-002 | export verify missed the audio stream | **Fixed** — PR #134 merged; the Director probe enumerates streams and refuses a silent export. |
| FUNCUT-003 | edit-plan schema could drift | **Fixed** — PR #130 merged; the generated schema and its drift tests are present. |
| FUNCUT-004 | clean-clone build scripts missing | **Fixed** — both portable scripts are in the tree. |
| FUNCUT-005 | Slint Skia `todo!` panic on `StaticTextures` | **Still on `main`; fix open as PR #157** — the arm returns `None` so a stray static texture draws nothing instead of aborting the process. |

Four of the five are resolved. Keep this file for the reasoning of each
finding; do not read its "fix PR open" statuses as current.

## Executive result

This pass found **4 confirmed defects / engineering gaps** on the audited main SHA, plus **1 conditional panic hazard** and several integration-acceptance gaps. This is not a claim that the whole repository contains only four bugs. The full line-by-line/dynamic audit is not complete, and several historical ledger IDs remain undefined because their original private session-brief definitions are not present in the repository.

| ID | Severity | Status | Finding |
|---|---|---|---|
| FUNCUT-001 | P2 | Confirmed on main; fix PR open | Startup I/O gate has a queued-proxy race |
| FUNCUT-002 | P1 | Confirmed verification gap on main; fix PR open | Export verification does not explicitly prove an audio stream exists |
| FUNCUT-003 | P2 | Confirmed on main; fix PR open | Edit-plan schema is duplicated and can drift |
| FUNCUT-004 | P3 | Corrected in PR #135; portable script still absent | Historical build notes previously read as clean-clone instructions |
| FUNCUT-005 | P2 conditional | Code hazard; reachability unproven | Slint Skia image conversion contains a `todo!` panic arm for StaticTextures |

## Confirmed findings

### FUNCUT-001 — queued proxy can start during the OpenVid index warm

- **Area:** startup orchestration / media proxy scheduler.
- **Severity:** P2 (resource-contention and startup reliability defect; not a data-corruption claim).
- **Main evidence:** `src/crates/concat-host/src/proxy.rs` checks `startup_io::index_warming()` before it submits a job to the scheduler. The false branch queues the job directly. A warm can begin after that check but before a scheduler worker dequeues the proxy job, so the proxy starts during the index warm despite the documented ordering guarantee.
- **Independent corroboration:** PR #132 explains this exact interleaving and adds `a_queued_proxy_rechecks_the_gate_when_its_worker_starts`. Its fix moves the gate check into the worker-start wrapper and re-submits parked work through the same guard.
- **Fix status:** PR #131 added the first gate and is merged; PR #132 is still open. On its latest head, format/locales/models and real-fixture analysis passed, Geneva dependency setup passed, and engine tests plus Geneva render inspection remain in progress. Do not mark closed until PR #132 is merged with green exact-head CI.
- **Fix:** merge/review PR #132; retain the scheduler-dequeue race regression test and test nested warms plus queued jobs.

### FUNCUT-002 — Director Verify does not explicitly verify audio-stream presence

- **Area:** AI Director final export verification.
- **Severity:** P1 for a narration/music workflow because an export can be visually valid but lack its required audio stream.
- **Main evidence:** `scripts/auto_director.py`'s `_ffprobe()` invokes ffprobe with `-select_streams v:0` and records video codec, dimensions, frame rate and duration only. It does not enumerate all stream types or return an audio-presence field. The render-level `_probe_checks()` therefore has no independent both-streams assertion.
- **Independent corroboration:** PR #134 identifies the missing-audio-stream gap and adds VERIFY-16, probes all stream kinds, records `has_audio` and `audio_codec`, and self-tests a silent MP4 refusal.
- **Fix status:** PR #134 is open, not merged. On its latest head, format/locales/models and real-fixture analysis passed; the analysis job's `Auto Director end-to-end safety self-test` passed after being moved to the FFmpeg-equipped job; engine tests and Geneva render inspection remain in progress. The app package stages FFmpeg libraries but not the `ffprobe` CLI; the Director's probe uses `shutil.which`, so target antiX acceptance must verify `ffprobe` is available or add a supported fallback, otherwise the check can be skipped.
- **Fix:** review and merge PR #134 only after CI proves the real synthetic silent-export refusal. Keep the export's video read-back check and the Director's explicit audio check separate.

### FUNCUT-003 — edit-plan schemas are duplicated and drifted

- **Area:** MCP plan validation / Director contract.
- **Severity:** P2 because agents can be told a different input contract from the one the runtime deserializer actually accepts.
- **Main evidence:** `src/crates/concat-mcp/src/tools.rs` contains a hand-written `canonical_plan_schema()` include of the skill JSON and a separate hand-written `plan_validate_schema()` JSON object. These are independent representations of the same contract.
- **Independent corroboration:** PR #130 documents concrete drift: the compact schema carried `bgm.mood` and `voiceover.whisperModel` while the canonical schema did not, and neither was generated from the Rust slot types. The PR adds schemars-derived plan types, a canonical template assembly, a projected compact schema, a drift test and a CI check.
- **Fix status:** PR #130 is open and unmerged at head `e93ccf5f4e8db045aa6966ca7975b9da7753ced5`. Format and real-fixture analysis passed, Geneva dependency/render passed, while engine tests and Nix remain in progress. The explicit schema-check timeout was raised from five to fifteen minutes after the earlier run expired during compilation. Do not mark closed because a branch exists.
- **Fix:** review/merge PR #130 after its generator, committed artifacts, negative drift test and CI check all pass on the exact PR head.

### FUNCUT-004 — clean-clone build reproduction instructions point to missing local scripts

- **Area:** developer documentation / reproducible builds.
- **Severity:** P3.
- **Evidence:** `TEST_EVIDENCE.md` previously presented `/home/z/my-project/scripts/setup-build-env.sh` and `funcut-env.sh` as a clean-clone reproduction recipe. `AGENT_STATE.md` and `SESSION_HANDOFF.md` also referenced the latter absolute path. Both scripts return 404 in the repository tree.
- **Impact:** the original instructions were not reproducible from a clean clone. This did not establish that the packaged CI build was broken; CI has its own dependency-fetch procedure.
- **Fix status:** PR #135 now labels those paths as historical machine-local notes and directs clean-clone readers to `.github/workflows/build-app.yml`, `scripts/ci/deps.lock` and `scripts/ci/fetch.sh`. The misleading clean-clone instructions are corrected in that PR. A turnkey portable local setup script remains a separate developer-experience task.

## Conditional code hazard — do not count as a proven user-facing bug yet

### FUNCUT-005 — unimplemented StaticTextures image conversion arm

- **Area:** patched Slint Skia renderer.
- **Evidence:** `src/patches/i-slint-renderer-skia/cached_image.rs` contains `ImageInner::StaticTextures(_) => todo!()`.
- **Impact if reached:** this arm panics instead of returning a supported image, a graceful refusal or a structured error.
- **Reachability evidence:** repository search on the audited main finds one `@image-url`, in `src/crates/concat/ui/app.slint` for the window icon. No visible `Image` element uses a static `@image-url`; media and thumbnails use runtime image sources. The Skia `as_skia_image` path is called for rendered image items, while the window icon is a window/platform property. This makes the arm appear unreachable in the current UI, but no built-app runtime test established that invariant.
- **Confidence:** the unimplemented branch is confirmed; current UI reachability is unlikely but not proven. Treat as a conditional crash hazard, not a confirmed production crash.
- **Fix:** add a regression test proving the current window-icon path does not call `as_skia_image`, or implement conversion/non-panicking refusal for StaticTextures before introducing any visible static texture image.

## Dead-code / fake-implementation checks

The old implementation ledger listed `BeatGrid`, `Value::Size` and `BakeRequest.crf` as possible dead code. This pass does **not** confirm that entire claim:

- `Value::Size` is used in the interpolation match in `concat-motion/src/track.rs`.
- `BakeRequest.crf` is read by the MCP motion-bake path in `concat-mcp/src/motion.rs` and also appears in the motion agent/sequence paths.
- `BeatGrid` is defined and publicly re-exported, but this search found no production consumer elsewhere in the repository. That makes it an unused-in-repo public API candidate, not proof that it is dead or safe to delete.
- Do not remove any of these without checking external API expectations and full references.

The code search also found upstream/vendor TODOs in the bundled Whisper/ggml and Slint patches. A TODO comment by itself is not a FUNCUT bug. The `todo!` arm above is different because it is executable panic behavior.

## Integration and acceptance gaps — not counted as confirmed code bugs

These require real workflow acceptance before the feature can be called fully verified:

1. **AI Director — major integration gap:** the Forge AUTO EDIT button is wired, but `auto_edit_program` only runs HandoffVoiceover, AudioRepair, Transcribe, VoiceSignals, Understand, Timeline, Export, Verify and Watch. It omits topic/research/script/plan generation, B-roll gathering, motion, captions, preview and revision. The separate Python helper requires a Director-authored plan; its two-pass revise loop is not wired into the UI Runner. No real brief-to-final-export project has been accepted. Unit tests and mocked provider calls are not equivalent to end-to-end acceptance.
2. **Motion clips:** the command and Motion page exist, but the project-level authoring path still needs a representative real-project acceptance for edit metadata, save/reopen, undo/redo, preview and final export.
3. **Lottie/dotLottie:** implementation exists with an intentionally bounded feature profile. Validate representative real files, asset references, unsupported-feature refusal and final export; do not describe it as full After Effects parity.
4. **VAAPI:** the integration is present, but real Intel/AMD hardware acceptance is outstanding. This is a functional hardware-compatibility check, not a performance benchmark.
5. **Geneva:** documented unsupported mixed compositions and animated inherited transforms are product limitations where the renderer safely refuses; they are not automatically bugs unless the UI/tool contract promises support or the refusal is bypassed.

## Current CI snapshot on audited main SHA

Main SHA: `dd109b8d056c0b33c1fcf78fb5021078d71f89c7`.

- **Format/locales/models:** passed.
- **Real-fixture analysis:** passed.
- **Geneva:** passed.
- **Build app:** passed for Linux x86_64 and Linux aarch64.
- **Engine tests and lints:** still running at the last inspected poll; Clippy had passed and test-binary compilation was in progress.
- **Nix:** still running at the last inspected poll.

A job that is still running is neither a pass nor a failure. Recheck the exact SHA before using this snapshot as release evidence.

## Repository inventory and audit coverage

The recursive Git tree for the audited SHA was not truncated and contained **4,407 entries: 3,590 files and 817 directories**. The file inventory includes approximately:

- 283 Rust files
- 75 Slint UI files
- 302 WGSL shader files
- 77 Python files
- 242 TSX files and 9 TypeScript files
- 6 GitHub workflow files
- 167 files under `src/patches/` (vendored/maintained upstream patches)
- 1,840 files under `src/` excluding `src/patches/`

Areas mapped for the audit: UI, timeline/project model, media import/decode/proxy, render/export, effects/shaders, motion/Geneva, MCP/API, AI Director, speech/analysis, stock/music integration, installer/packaging, documentation, tests and CI.

**Important scope honesty:** the full tree was inventoried and targeted source searches/reviews were performed, but every one of the 3,590 files has not been manually read line-by-line, every UI action has not been clicked in a built app, and every end-to-end scenario has not been run. This report is an evidence-backed current-main supplement, not a claim of exhaustive dynamic verification.

## Historical ledger reconciliation

The earlier `docs/BUG-AUDIT-FINAL.md` contains the detailed TIME / ENV / XSTAGE / LATENT / DEG / VERIFY ledger reconciliation. It identified a set of IDs that had no definition in the public repository, issues or PR history. Those remain **UNDEFINED** until the original definitions from the private session briefs are supplied; they must not be guessed or silently marked closed.

The earlier audit's PR status snapshot is stale. At the current-main snapshot:

- PR #130 (SCH-02 schema generation): open, unmerged.
- PR #131 (initial RES-03 gate): merged as the audited main commit.
- PR #132 (RES-03 worker-start race fix): open, unmerged.
- PR #134 (VERIFY-16 audio stream presence): open, unmerged.
- PR #133 (RISK-01/02 monetized licensing): open, licensing-specific and outside the code-defect count in this audit.
- PR #135 (audit documentation): this report, current-main addendum and clean-clone documentation correction are in this PR.
- PR #136 (P3-008 archive.org dependency): open; records an external environmental dependency, not a code defect.
- PR #137 (Geneva master-list reconciliation): open; adds current-main Geneva and workflow evidence.
- PR #138 (Director tracker reconciliation): open; corrects stale integration/revise-loop status.

## Next audit passes needed to finish the full repository audit

1. Build from a clean clone and run the exact CI commands on the audited SHA.
2. Inspect every first-party Rust module and every UI action/Slint callback for missing wiring, ignored results, stale state, error swallowing and unreachable code.
3. Trace every MCP tool from schema → request dispatch → backend → project mutation → response, and test every tool's happy path and failure path.
4. Exercise import → decode → timeline → preview → export → reopen for video, audio, stills, text, effects, masks, transitions, captions and motion clips.
5. Run the Director with real local/provider fixtures, cancellation, retry, resume, partial failure and final verification.
6. Run property/fuzz tests for project JSON, media metadata, schema validation, timeline arithmetic, keyframes, speed curves and corrupted cache/state files.
7. Review all non-vendored TODO/panic/unwrap/unsafe sites in context; classify each as invariant, test-only, recoverable error or user-triggerable crash.
8. Reconcile every finding against the private 216-item master ledger once its undefined definitions are available.

No arbitrary 200–300 total is asserted. The count grows only when a distinct, evidence-backed defect is identified; duplicate symptoms of one root cause should share one ID.
