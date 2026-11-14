# FUNCUT repository-wide bug audit — current-main supplement

Audit date: 2026-11-14  
Repository: `coreselfmastery/FUNCUT`  
Audited base: `main` at `dd109b8d056c0b33c1fcf78fb5021078d71f89c7` (PR #131 merge)  
Scope requested: functional correctness, feature wiring, incomplete implementations, runtime failure paths, data integrity, API/schema contracts, tests and documentation. **Performance benchmarking and license/compliance review are deliberately excluded.**

This report supplements `docs/BUG-AUDIT-FINAL.md`. That earlier report was written against `21d136c3972ad87033992968b987b3b0c6ebb5c7`; its statuses for PRs #130–#134 must not be read as current-main truth.

## Executive result

This pass found **4 confirmed defects / engineering gaps** on the audited main SHA, plus **1 conditional panic hazard** and several integration-acceptance gaps. This is not a claim that the whole repository contains only four bugs. The full line-by-line/dynamic audit is not complete, and several historical ledger IDs remain undefined because their original private session-brief definitions are not present in the repository.

| ID | Severity | Status | Finding |
|---|---|---|---|
| FUNCUT-001 | P2 | Confirmed on main; fix PR open | Startup I/O gate has a queued-proxy race |
| FUNCUT-002 | P1 | Confirmed verification gap on main; fix PR open | Export verification does not explicitly prove an audio stream exists |
| FUNCUT-003 | P2 | Confirmed on main; fix PR open | Edit-plan schema is duplicated and can drift |
| FUNCUT-004 | P3 | Confirmed documentation defect | Build reproduction instructions call scripts absent from the repository |
| FUNCUT-005 | P2 conditional | Code hazard; reachability unproven | Slint Skia image conversion contains a `todo!` panic arm for StaticTextures |

## Confirmed findings

### FUNCUT-001 — queued proxy can start during the OpenVid index warm

- **Area:** startup orchestration / media proxy scheduler.
- **Severity:** P2 (resource-contention and startup reliability defect; not a data-corruption claim).
- **Main evidence:** `src/crates/concat-host/src/proxy.rs` checks `startup_io::index_warming()` before it submits a job to the scheduler. The false branch queues the job directly. A warm can begin after that check but before a scheduler worker dequeues the proxy job, so the proxy starts during the index warm despite the documented ordering guarantee.
- **Independent corroboration:** PR #132 explains this exact interleaving and adds `a_queued_proxy_rechecks_the_gate_when_its_worker_starts`. Its fix moves the gate check into the worker-start wrapper and re-submits parked work through the same guard.
- **Fix status:** PR #131 added the first gate and is merged; PR #132 is still open against the audited main SHA. Its description says CI has not yet run on that branch. Do not mark closed until PR #132 is merged with green exact-head CI.
- **Fix:** merge/review PR #132; retain the scheduler-dequeue race regression test and test nested warms plus queued jobs.

### FUNCUT-002 — Director Verify does not explicitly verify audio-stream presence

- **Area:** AI Director final export verification.
- **Severity:** P1 for a narration/music workflow because an export can be visually valid but lack its required audio stream.
- **Main evidence:** `scripts/auto_director.py`'s `_ffprobe()` invokes ffprobe with `-select_streams v:0` and records video codec, dimensions, frame rate and duration only. It does not enumerate all stream types or return an audio-presence field. The render-level `_probe_checks()` therefore has no independent both-streams assertion.
- **Independent corroboration:** PR #134 identifies the missing-audio-stream gap and adds VERIFY-16, probes all stream kinds, records `has_audio` and `audio_codec`, and self-tests a silent MP4 refusal.
- **Fix status:** PR #134 is open, not merged at this audit snapshot. Its self-test is reported as 14 passed / 0 skipped in the PR description; exact-head CI and merge are still required.
- **Fix:** review and merge PR #134 only after CI proves the real synthetic silent-export refusal. Keep the export's video read-back check and the Director's explicit audio check separate.

### FUNCUT-003 — edit-plan schemas are duplicated and drifted

- **Area:** MCP plan validation / Director contract.
- **Severity:** P2 because agents can be told a different input contract from the one the runtime deserializer actually accepts.
- **Main evidence:** `src/crates/concat-mcp/src/tools.rs` contains a hand-written `canonical_plan_schema()` include of the skill JSON and a separate hand-written `plan_validate_schema()` JSON object. These are independent representations of the same contract.
- **Independent corroboration:** PR #130 documents concrete drift: the compact schema carried `bgm.mood` and `voiceover.whisperModel` while the canonical schema did not, and neither was generated from the Rust slot types. The PR adds schemars-derived plan types, a canonical template assembly, a projected compact schema, a drift test and a CI check.
- **Fix status:** PR #130 is open and unmerged at this snapshot. The report in the earlier audit that described SCH-02 as partial remains correct; do not mark it closed because a branch exists.
- **Fix:** review/merge PR #130 after its generator, committed artifacts, negative drift test and CI check all pass on the exact PR head.

### FUNCUT-004 — clean-clone build reproduction instructions point to missing local scripts

- **Area:** developer documentation / reproducible builds.
- **Severity:** P3.
- **Evidence:** `TEST_EVIDENCE.md` instructs users to run `/home/z/my-project/scripts/setup-build-env.sh` and source `/home/z/my-project/scripts/funcut-env.sh`. `AGENT_STATE.md` and `SESSION_HANDOFF.md` repeat the latter path. Neither script exists in the current repository tree, and the instructions use an absolute machine-local path.
- **Impact:** a clean clone cannot reproduce the documented local build setup from those instructions. This does not establish that the packaged CI build is broken; CI has its own dependency-fetch procedure.
- **Fix:** commit portable, repository-relative scripts or rewrite the evidence docs to point to the actual checked-in CI setup steps. Verify every command from a clean clone.

## Conditional code hazard — do not count as a proven user-facing bug yet

### FUNCUT-005 — unimplemented StaticTextures image conversion arm

- **Area:** patched Slint Skia renderer.
- **Evidence:** `src/patches/i-slint-renderer-skia/cached_image.rs` contains `ImageInner::StaticTextures(_) => todo!()`.
- **Impact if reached:** this arm panics instead of returning a supported image, a graceful refusal or a structured error.
- **Confidence:** the unimplemented branch is confirmed; whether the FUNCUT application can produce this ImageInner variant on the selected Skia backend is not established by this source review. Treat as a conditional crash hazard, not a confirmed production crash.
- **Fix:** establish whether the variant is reachable in the app's renderer configuration. If reachable, implement conversion or return a non-panicking unsupported result and add a regression test. If unreachable by invariant, document and test that invariant.

## Dead-code / fake-implementation checks

The old implementation ledger listed `BeatGrid`, `Value::Size` and `BakeRequest.crf` as possible dead code. This pass does **not** confirm that entire claim:

- `Value::Size` is used in the interpolation match in `concat-motion/src/track.rs`.
- `BakeRequest.crf` is read by the MCP motion-bake path in `concat-mcp/src/motion.rs` and also appears in the motion agent/sequence paths.
- `BeatGrid` is defined and publicly re-exported, but this search found no production consumer elsewhere in the repository. That makes it an unused-in-repo public API candidate, not proof that it is dead or safe to delete.
- Do not remove any of these without checking external API expectations and full references.

The code search also found upstream/vendor TODOs in the bundled Whisper/ggml and Slint patches. A TODO comment by itself is not a FUNCUT bug. The `todo!` arm above is different because it is executable panic behavior.

## Integration and acceptance gaps — not counted as confirmed code bugs

These require real workflow acceptance before the feature can be called fully verified:

1. **AI Director:** no recorded real project from brief/script through research, asset selection, voice, motion, edit, final export and review. Unit tests and mocked provider calls are not equivalent to a real end-to-end acceptance.
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
- PR #135 (audit documentation): this report is being added to that audit PR.
- PR #133 is licensing-specific and intentionally outside this audit's scope.
- PR #136 records an external archive.org dependency; it is not counted as a code defect here.

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
