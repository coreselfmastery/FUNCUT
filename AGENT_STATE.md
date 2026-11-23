# AGENT_STATE.md — the mission's running state

## Current state — 2026-11-23, the `.15` release

- `main` is at `573a661`; all seven checks on the latest commit are green
  (CI run `45169594535`, Geneva `45169594539`, Build app `45169594545`).
- `v0.2.8-funcut.15` is the latest published release: ten assets, all
  downloadable, `SHA256SUMS` matching GitHub's digests, and a manifest
  naming x86_64, aarch64 and 23 models. `install-linux.sh --list` sees
  `.15` first.
- The `.15` release workflow (`45163610747`) is red only at its Nix job:
  the hosted runner lost communication after about 1.5 hours, so the
  publish job was skipped. The release itself was published manually and
  verified. A rerun of the Nix job is the way to clear the workflow.
- The test count on the latest green engine lane is 2,286 workspace
  tests (13 skipped), 9 documentation tests and 30 gRPC tests: 2,325 in
  all. `README.md` and `docs/TESTING.md` now carry that count.
- Remaining product work is the target-machine acceptance list in
  `KNOWN_ISSUES.md`: proxy/Geneva performance, VAAPI on real hardware,
  motion-clip and AI Director end-to-end runs, and publishing provenance.
- Branch protection still needs to be configured to require the CI
  checks; the API currently reports no required status checks.

The session logs below are historical records of the 2026-11-10 to
2026-11-21 completion passes. They are not the current state.

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

## Session 9 log (2026-11-21) - the shapes pass

Base: main at `4b1cd4e` (PR #186 merged, all 7 checks green at its
head `b5847ac`). PR #189 (`fix/detectors-2026-11-21`) carries this
session, every unit one commit, every non-GUI gate green locally
(concat-mcp 161/161, voice-signals 31/31, fmt + clippy clean, the
seven sense selftests, the harness self-test exit 0):

1. `fix(signals)`: the detectors the previous session left unlanded,
   cherry-picked onto #186's head (the hum Goertzel, the reverb
   overlapping frames, the 85th-percentile rolloff, the bed skip
   gate on the decision the profile reached, hook_lift, the
   catalogue's confidence honesty) - plus the two tests that
   finishing pass re-pinned (the collision gate, the bed's shape).
2. `fix(signals)`: entitiesAvailable counts only the NER ear's own
   answer - the CI lane without the Rust ears caught the complement
   source note flipping the flag (DEG-01).
3. `fix(auto-edit)`: seven stage fixes against the producers' real
   shapes - the bed import's missing `path` (the bed was NEVER
   imported), the entrance keys keeping the caption band (keys are
   absolute; a keyed constant is the engine's neutral zero), the beat
   grid on `suggest_cuts` (analyze_beats is a job whose handle has no
   grid), per-take SFX peaks and caption emphasis sets, the critic's
   one-based frame labels, the act/claim confidences, the lane key.
4. `fix(sense)`: eight script fixes - arcShape, the Goertzel both-end
   clamp, the source-rate probe for the formants gate, WhisperX words
   by original line index, prosody_style's null rows and real fields
   and its aggregated formants, the octave span for f0RangeSemitones,
   the 50 ms repair floor, the codec per container, durationSeconds.
5. `fix(forge)`: the harness reads the shapes the server really
   sends (the watcher NEVER fired on the real wire - the bin read
   empty on every project_get), VERIFY-15 through sense_captions (a
   phantom standalone tool name), the bed picked to fit and trimmed
   to the cut (a 180 s bed rendered a black tail its own run
   refused), the Ctrl-C exit code, the transport reap.
6. `fix(voice-read)`: the speaker ear's `available` gates its
   utterance count (absence read as a measured "not punchy"), and the
   entity's own confidence gates its stat/timeline cards.

Found and left for the next pass (verified, not yet fixed): the
consumer-guard's quoted-literal arm passes same-name strings
(`LineRead.speaker`, `PhraseSpan.label` ride it); the Sfx stage
re-walks the emphasis reply raw where a typed read exists; the brief
reads crest and degradations raw beside the typed fields.

## Session 10 log (2026-11-22) - the audit answered twice: the pipeline, the Forge pane, and the Geneva seams

Base: main at `cdddade9` (PRs #199 and #198 merged, the #195 typed-read
cherry-picks and the release-check residue landed). Every unit below is
one commit on main, pushed as verified. Local gates: concat-mcp 177/177,
concat-motion 613/613, concat-director 65/65, voice-signals 32/32,
concat-export 48/48 (the 4 monitor tests need Vulkan, CI's), fmt clean,
clippy clean on every non-GUI crate (the GUI crate OOMs this 4 GB box -
`cargo check -p concat` passes, clippy is CI's).

Two end-to-end audits (the auto-edit program read line by line; the
Forge pane, the watcher contract, the motion engine and the Geneva
adapter the same way) found twenty-five faults; the fixes:

1. `fix(auto-edit)`: the plan-landed music bed was never re-trimmed
   after the tighten - the Music stage's plan-bed path read
   `bed_runtime.unwrap_or(0.0)`, so the trim guard never fired and every
   plan-bed run shipped the bed's scored black tail. The runtime now
   falls back to `narration_runtime(log)` - the latest view's own
   longest non-bed end.
2. `fix(auto-edit)`: one `segment_topics` refusal (its 3-line minimum)
   aborted the Understand stage's whole queue, killing that take's and
   every later take's `emotion` extract - the grade, bed words, caption
   weight, B-roll register and motion cards all silently defaulted. A
   take under three lines now skips the tiling, never the reads after it.
3. `fix(auto-edit)`: the Plan stage ended ITSELF on an empty bed search
   (`phase = 2; return None`), so the planner's ranked B-roll asks and
   the `plan_validate` gate never ran on a cold shelf. Both paths now
   fall through to the planner in the same call.
4. `fix(auto-edit)`: the captions' closing bound used the exclusive
   `timeline_at`, so a group whose last word ended exactly at a piece
   edge (a trimmed take, a tightened pause) lost its WHOLE caption. The
   closing bound now maps `timeline_at_or_before`, the mercy the B-roll
   slot bounds already had.
5. `fix(auto-edit)`: the emotion easing curve for caption entrances was
   dead code - `entrance_targets` read `clip["media"]`, a field a text
   clip has never carried, so every caption took the default settle
   since the field was first emitted. The line is now found through the
   TIMELINE: `take_second_at` (the inverse of `timeline_at`) reads the
   take playing under the caption's first second, at the file-second
   that moment plays, and the test pins a caption over a measured take
   taking the line's own curve.
6. `fix(auto-edit)`: `dead_air_cuts` read the OLDEST timeline view in
   the log (a `find`, where every other consumer uses `latest_view`), a
   latent mis-move for any visual clip added before the tighten; and
   `timeline_at` aborted the whole scan on one clip without a `start` -
   both now behave as the rest of the file does.
7. `fix(motion)`: the joy and sadness seam transitions named
   effect-kind packages (`concat.zoom-blur-punch`, `concat.zoom-blur`),
   which the export resolver's catalogue arm silently falls through to a
   plain cut - the very move the emotion asked for never rendered. Both
   now name transition-kind packages, and the test asserts every id the
   program can name is transition-kind with a `[transition]` section.
8. `fix(motion)`: `diff()` compared only nodes, so an `ops:
   set-background` / `set-duration` / curve-edit revision produced an
   EMPTY diff and the bake inherited every frame from the predecessor
   under a new hash - the new background never rendered. `Diff.document`
   now flags comp-level changes and dirties every frame.
9. `fix(motion)`: the dirty mask was computed at the comp documents'
   fps but applied at the bake's own, so an explicit `fps` mis-scaled
   every window (the second half of a retimed node at 60 over 30
   inherited stale). `dirty_frames_at(fps)` computes mask AND windows at
   the bake's rate.
10. `fix(motion)`: `inherit_outside` skipped the donor's copy with a
    half-open range, so the LAST dirty frame of every ranged incremental
    bake was overwritten by the stale donor file - the skip is now the
    closed range the engine actually renders. The inheritance is also
    capped at the comp's own frame count (a longer donor no longer ships
    its tail through the export's every-PNG listing).
11. `fix(motion)`: the predecessor lookup matched `name.contains(suffix)`
    (`-1920x1080@30` matched `@30.5`, `@24` matched `@240`) and never
    compared the manifest's `assets` key - a bake could inherit frames a
    different frame rate rendered, or drawn over a re-saved asset's old
    pixels. `ends_with` + the assets fingerprint, the same strict
    equality the cache hit uses.
12. `fix(motion)`: the Geneva frame-pipe path wrote `frame_name(index)`
    0-based, off by one from every other producer's 1-based
    `0001.png` contract - pipe slot 0 now lands in 0001.png.
13. `fix(comp)`: `inexpressible_in` had no `NodeKind::Camera` arm, so a
    camera comp with the sidecar installed passed `engine_for` and baked
    WITHOUT its camera move (Geneva's mapper cannot draw cameras at
    all). The arm names it now.
14. `fix(export)`: `motion_sequence` never compared the manifest's
    `assets` key with the current bytes, so a re-saved logo exported the
    old pixels until someone re-ran motion_bake - the export now refuses
    a recorded key that contradicts (pre-field manifests keep exporting).
15. `fix(schemas)`: `motion_bake` / `motion_inspect` / `scene_analysis`
    / `motion_frame` / `motion_preview` advertised `required: []` while
    the handler refuses a call naming neither `preset` nor `comp` - the
    five doors now carry `anyOf: [{required: [preset]}, {required:
    [comp]}]`.
16. `fix(forge)`: a landed run left the finished edit READ-ONLY - the
    landing's `open_project` hit the same-folder early return (the
    live-follow display IS the sibling), skipping the claim and leaving
    the follow's guard standing. `land_live_follow` ends the display
    first, at all three landing paths.
17. `fix(forge)`: the signals lab could brick itself busy forever - the
    reading ran on `spawn_in_project`, whose epoch guard DROPS the reply
    when the project changed mid-read, leaving `busy == true` for the
    session. Plain `spawn`: the reading always arrives, the card names
    its signal.
18. `fix(forge)`: both run readers killed the process GROUP after the
    leader was reaped - a recycled pid's group would take the SIGKILL.
    The shared `supervise_forge_child` observes the exit WITHOUT
    reaping (`waitpid` + `WNOWAIT`), cleans the group while the leader
    still owns its pid, then reaps.
19. `fix(acceptance)`: the pane's independent sync check asked
    `toleranceMs: 150` where the watcher's own contract is 60 - one
    shared `SYNC_TOLERANCE_MS` const now feeds both the acceptance and
    the fixed spine's verify.

## Session 9 log (2026-11-21, later) - the timeline-live branch

Branch `auto-edit/timeline-live-2026-11-21`, delivered as **PR #190**
(open against main at this writing; the user's standing instruction is
commit-early-push-often and do not babysit CI). Six units, one per
commit, plus the evidence doc:

1. `fix(build-env)`: the SourceForge `/download` suffix (pushed first,
   before this log's session began).
2. `feat(geneva)`: the GSAP ease families by name (30 named curves),
   colour/size/sweep tracks, validator diagnostics, native scene
   rendering. concat-motion 612/612.
3. `feat(mcp)`: the motion tools choose the engine the compositor would
   (`EnginePreference::choose`): native for comps the sidecar would
   drop or approximate and for machines without the sidecar; markup
   keeps Geneva and its honest installer refusal; the incremental seam
   inherits only from a bake the same engine drew; native bakes encode
   through ffmpeg (`.mov`/ProRes 4444 keeps alpha).
4. `feat(project)`: `AddMotionClip { engine }` - the clip records the
   engine that drew the bake it points at; unknown engines refused.
5. `fix(mcp)`: the director program's motion cards carry the bake's
   engine into `addMotionClip`.
6. `feat(auto-edit)`: **the live timeline follow** - the window takes
   the run's sibling as a claim-free display (Session::open_info, no
   claim), a watcher polls the document fingerprint and the studio
   re-reads on the run's autosaves (playhead held, art re-asked, proxies
   built as media lands); user edits/undo/save refused while the follow
   holds; the landing now asks the server to release the run's session
   first (`concat_mcp::backend::release_window_session`, ProjectClose
   without save - proven over a real server in a new test) because the
   server's Api claim otherwise refuses the window's claim forever
   ("open through a remote connection") - the landing could never have
   worked before; failed/cancelled runs hand the window back its parked
   project, saved, and keep the sibling for the retry. Three locale keys
   x 26 languages via the repo's own updater; FORGE.md updated.

Environment finding worth keeping: this box's disk (9.9 GB) fills during
test linking; `prune-dupes.sh` finds nothing (no identical duplicates),
so `scripts/prune-stale.sh` (in /home/z/my-project/scripts, OUTSIDE the
repo - it is a machine tool, not a deliverable) drops stale fingerprints
and old test binaries; ~3 GB per cycle.

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
