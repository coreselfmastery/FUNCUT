# BUG-AUDIT-FINAL — the Session 6 audit of the residual ledger

Audited: 2026-11-14. `main` = `21d136c3972ad87033992968b987b3b0c6ebb5c7`
(merge of PR #129), fresh clone. Last release confirmed still
`v0.2.8-funcut.12` (tags read live; no `.13` exists).

## Current-main recheck — 2026-11-14 17:50 UTC

The audit snapshot below was prepared against `21d136c3972ad87033992968b987b3b0c6ebb5c7`. It is historical, not the current release state. The current `main` head is `dd109b8d056c0b33c1fcf78fb5021078d71f89c7` (merge of PR #131).

Current corrections to the snapshot:

- **RES-03:** PR #131 is merged and the startup gate is present on main. A second-order race remains: a proxy can be queued before a warm starts and dequeued during the warm. PR #132 adds a worker-start recheck and a regression test; it is open and not yet CI-verified.
- **SCH-02:** PR #130 remains open at `e93ccf5f4e8db045aa6966ca7975b9da7753ced5`. Its format and real-fixture jobs passed; engine tests, Geneva render inspection and Nix are still running. The schema-check timeout was raised from five to fifteen minutes after the earlier run expired while compiling the checker.
- **RISK-01/02:** PR #133 now carries the monetized OpenVid/BGM gates. Its current head is `55e3449d95d01495c25be136a04578257b6c18b1`; CI, Geneva and Nix are running. A fail-closed allow-list regression was added for unknown licence strings and unknown shelf IDs; it is not yet validated.
- **VERIFY-16:** PR #134 adds the audio-stream probe. Its first CI attempt failed because the Director self-test ran in the no-FFmpeg format job, so the synthetic silent render could not be probed. The self-test has been moved to the FFmpeg-equipped real-fixture job; the new run is pending.
- **P3-008:** PR #136 records archive.org as an environmental dependency of the FreePD shelf and remains open.
- **Current-main CI:** formatting/locales/models and real-fixture analysis passed; engine tests are still running. Geneva passed both jobs. Build app passed both Linux x86_64 and ARM64 lanes, including clean-distribution smoke tests, and uploaded artifacts. Nix remains in progress. No current-main release tag has been cut.
- **Review and protection:** the open PRs have no submitted approvals in the current review lists. The repository rulesets endpoint returned an empty list; the classic branch-protection endpoint returned HTTP 403 to the connected GitHub integration, so classic protection cannot be confirmed here.

Do not use the historical rows below as evidence that the current `main` is still at the earlier SHA. The current release gate remains open until the exact-head checks finish and the hardware/real-project acceptance evidence is recorded.

Method: grep of the working tree **and** the full ref history
(`git log -S` across all refs, all branches), GitHub issue/PR search
(bodies and comments), and the landed commits' own messages as the
definition source (`ec92e8d8`, `4b81453a`, `35e9eec6`, `006d598f`,
`00fc26ba`, `a8f67544`, `194a5452`, `904ae276`), PR #119's check
table, `docs/DIRECTOR-COMPLETION-TRACKER.md`, `KNOWN_ISSUES.md`, and
the session brief's own definitions (RISK-01/02, P3-008,
UNVER-03..05). No PR claim was trusted without a line in the tree.

## The definition problem — read this first

Twenty of the requested IDs have **no definition anywhere public**:
not in the working tree, not in any commit on any branch ever pushed
(pickaxe over all refs), not in any issue, PR body or comment (GitHub
search). The 216-item ledger lives in the principal's session briefs.
Per the brief's own "do not guess" rule these are reported
**UNDEFINED**, not classified. The principal must supply the ledger
lines for them; the audit finishes in minutes once they arrive.

Undefined: TIME-01, TIME-02, TIME-04, TIME-05, TIME-07, TIME-11,
ENV-10..14, XSTAGE-01..05, XSTAGE-07..10, LATENT-02, LATENT-03,
LATENT-04, LATENT-06, LATENT-07, LATENT-08, P3-003, P3-005, FALL-05,
DEG-02.

## Summary

| Group | CLOSED | OPEN | PARTIAL | UNDEFINED |
| --- | --- | --- | --- | --- |
| Code (SCH-02, RES-03, DATA-01, FALL-05) | 0 | 0 | 3 | 1 |
| TIME-01..13 | 7 | 0 | 0 | 6 |
| ENV-10..14 | 0 | 0 | 0 | 5 |
| XSTAGE-01..10 | 1 | 0 | 0 | 9 |
| LATENT (01..04, 06..08) | 1 | 0 | 0 | 6 |
| RISK-01/02 | 0 | 2 | 0 | 0 |
| P3-003/005/008 | 0 | 1 | 0 | 2 |
| CONTRA residuals | 0 | 1 | 0 | — |
| DEG residuals | 10 | 0 | 0 | 1 |
| VERIFY residuals | 15 wired | 1 gap | 0 | — |

## Code group

| ID | Status | Proof / what's missing |
| --- | --- | --- |
| SCH-02 | **PARTIAL** — open on `main`, fix written and in review | `main` still hand-maintains the plan schema: `src/crates/concat-mcp/src/tools.rs:1897` (`fn plan_validate_schema() -> Option<Value>`) plus the hand-written `json!` canonical. The generator fix exists complete on branch `feat/schema-generate` (head `31de8e5a`, "the plan schema is generated from the plan types (SCH-02)": schemars-generated slots, canonical assembled, compact projected from it, drift test, CI step) — **opened as PR #130 on 2026-11-14, unmerged**; its check state is not readable from this sandbox. Missing: the merge (green CI + merge), which needs the write token. |
| RES-03 | **PARTIAL** — open on `main`, fix written and in review | `main` has no startup I/O ordering: `src/crates/concat-host/src/startup_io.rs` does not exist on `main`, and `proxy.rs` on `main` submits proxy encodes with no gate against the index warm. The fix exists complete on branch `fix/startup-io` (head `ff4ffc87`, "the index warm owns the disk before the proxies start (RES-03)": process-local RAII startup gate in new `startup_io.rs`, `proxy::ensure` parks until the warm finishes, reentrant, nothing interrupted) — **opened as PR #131 on 2026-11-14, unmerged**. Missing: the merge. |
| DATA-01 | **PARTIAL** | The run's own signalLog is enforced (CLOSED on main): `scripts/auto_director.py:4214-4227` raises unless every entry carries `decision` and `provenance ∈ {measured, fallback}` before the record is written. The plan-side signalLog (`plan.signalLog`) is held only to its own gates — driven, coverage ≥ 50%, citations answered (`auto_director.py:2157-2169`) — **not** to the per-entry decision/provenance schema; by design it is the Director's, not the runner's. The brief calls DATA-01 partial; the missing half is not named in any public definition — the principal's ledger line is needed to fix the right thing. |
| FALL-05 | **UNDEFINED** | Never defined in any commit, branch, issue or PR. #125 closed FALL-01..03 and 06..10, skipping 05 — no record says why or what it is. |

## TIME-01..13

Definitions exist only where `ec92e8d8` ("the runner repairs, gates,
verifies and enriches the edit it runs") named them.

| ID | Status | Proof |
| --- | --- | --- |
| TIME-01 | UNDEFINED | — |
| TIME-02 | UNDEFINED | — |
| TIME-03 | **CLOSED** | The plan's voiceover speed applied to the placed clip: `scripts/auto_director.py:2352-2362` (phase `clip_speed` declared at 698-700). |
| TIME-04 | UNDEFINED | — |
| TIME-05 | UNDEFINED | — |
| TIME-06 | **CLOSED** | SFX placed so the file's own peak lands on the planned beat, `syncAt` backed out: `scripts/auto_director.py:3327-3369`. |
| TIME-07 | UNDEFINED | — |
| TIME-08 | **CLOSED** | BGM imported only when the library is ready, a missing library a logged absence: `scripts/auto_director.py:3428-3450` (with DEG-06/07). |
| TIME-09 | **CLOSED** | The bed ducked under the measured speech spans with keyframes, not one flat gain: `scripts/auto_director.py:3498-3516`. |
| TIME-10 | **CLOSED** | `transitionIn` patched onto the clip that begins the cut: `scripts/auto_director.py:2968-2979`; audited executed/skipped at 3062. |
| TIME-11 | UNDEFINED | — |
| TIME-12 | **CLOSED** | Speech verification tolerance 60 ms, drift logged: `VERIFICATION_SYNC_TOLERANCE_MS = 60` at `scripts/auto_director.py:814`, the gate at 1118-1131. |
| TIME-13 | **CLOSED** | One loudness normalize pass attempted before the refusal names the numbers: `scripts/auto_director.py:3704-3730`. |

## ENV-10..14 — all five UNDEFINED

ENV-01..09 (the server's minimal environment) are verified closed on
`main`: `_server_env` at `scripts/auto_director.py:102-216`
(`FUNCUT_KEEP_ENV=1` escape at 216). What ENV-10..14 are is recorded
nowhere public.

## XSTAGE-01..10

| ID | Status | Proof |
| --- | --- | --- |
| XSTAGE-01..05 | UNDEFINED | — |
| XSTAGE-06 | **CLOSED** | `measure_loudness` on the export measured against the platform's own target (-14 short-form / -23 broadcast), a plan's number a wish: `scripts/auto_director.py:3702-3712`. |
| XSTAGE-07..10 | UNDEFINED | — |

## LATENT-01..08 (05, 09, 10 excluded by the brief)

| ID | Status | Proof |
| --- | --- | --- |
| LATENT-01 | **CLOSED** | The handshake (`concat_version` + `tools/list`) refuses a server that cannot serve the core tools up front, naming them; absent enrichment tools are skipped with a logged line and become required when the plan asks for what they do: `scripts/auto_director.py:36-46, 126-165, 279-321`. |
| LATENT-02, 03, 04, 06, 07, 08 | UNDEFINED | — |

## RISK-01/02 — both OPEN (the only fully-defined code findings, and they are real)

| ID | Status | Proof / what's missing |
| --- | --- | --- |
| RISK-01 | **OPEN** | No per-clip license gate exists before OpenVid import. The index stores **no licence field, no author, no release information** — `docs/STOCK_LICENSING.md:36-37`; the monetized-work rules are prose only (47-54); the doc says it outright: "a future version should add a `licence` column to the index and refuse to import unlicensed shelves in a commercial export profile; **that is not implemented here**" (`STOCK_LICENSING.md:60-65`). No `commercial`/monetized concept exists in the project or export model to gate on. What a fix needs: a license/provenance record carried per imported clip + a monetized flag on the export (or project) + a refusal in the import path (Rust, `concat-host/openvid.rs` + `concat-api`), plus the director honoring it. Cannot be written honestly in this sandbox: no Rust toolchain, no push path, "no fake green". |
| RISK-02 | **OPEN** | MobyGratis (Lo-Fi shelf, 166 tracks) is non-commercial-only, and nothing enforces it: `BgmTrack` carries **no license field at all** (`src/crates/concat-api/src/message.rs:1217-1238`), `bgm_import` places any track unconditionally, and the director's bed placement (`scripts/auto_director.py:3494+`) never sees the shelf's terms. The warning is prose only (`docs/BGM.md:100-109`, `docs/STOCK_LICENSING.md:71-74`). Rides with it a live CONTRA (below): the `bgm_search` tool description claims licenses are carried. |

## P3-003/005/008

| ID | Status | Proof |
| --- | --- | --- |
| P3-003 | UNDEFINED | — |
| P3-005 | UNDEFINED | — |
| P3-008 | **OPEN** | `KNOWN_ISSUES.md` (read in full, 2026-11-14) carries **no archive.org entry**. The BGM library's FreePD catalog and every FreePD track are fetched from archive.org (`docs/BGM.md:24-25`) — an environmental dependency outside the repo's control that belongs in the remaining-limitations list. Fix is a docs entry (prepared this session on `docs/residual`). |

## CONTRA residuals

#128 closed CONTRA-01..07; spot-checks of its claims hold on `main`
(110 tools: `docs/FUNCUT.md:67,194,197`; the 60 ms constant named;
the automation boundary rewritten). One **live** contradiction found
this session:

| Where | Contradiction |
| --- | --- |
| `src/crates/concat-mcp/src/tools.rs:500` | `bgm_search`'s description says "candidates carry ids, durations **and licenses**" — `BgmTrack` (`message.rs:1217-1238`) serializes id, source, title, artist, category, seconds, bytes, localPath. No license. (The sibling `sounds_search` description at tools.rs:469 is honest — `SoundEntry` does carry `license`, `message.rs:1137`.) |

`SKILL.md` (`.claude/skills/clip-skills/SKILL.md`) was read in full:
it is an editor-agnostic routing doc and makes no testable claim the
code contradicts. No other doc/code contradiction found in the
spot-checks; a full re-read of every doc against every tool is beyond
one session and was not claimed.

## DEG residuals

DEG-02 is UNDEFINED (never defined, never claimed closed — #124
closed "DEG-01, DEG-03..10", skipping it). The defined ten are all
present on `main`:

| ID | Proof |
| --- | --- |
| DEG-01 | `entitiesAvailable` — `src/crates/concat-mcp/sense/voice_signals.py:1129-1150`, read at 1320; recorded by the watcher. |
| DEG-03 | keyword-tier named on every rerank reply — `src/crates/concat-mcp/src/sense.rs:356,440`. |
| DEG-04 | `CRAFT_TIER_CONFIDENCE` — `scripts/auto_director.py:4315`, applied at 3936. |
| DEG-05 | `identify_music` failure = a missing identification — `scripts/auto_director.py:2073-2084`. |
| DEG-06 | unready library a warned, recorded absence — `scripts/auto_director.py:3428-3450`. |
| DEG-07 | `bgm_prepare` started by the runner, bounded — `scripts/auto_director.py:3428-3436`. |
| DEG-08 | fetched clip proven before use (MP4 head check) — `src/crates/concat-api/src/lib.rs:1300` (`fetched_clip_checked`), used at 1317, 1358. |
| DEG-09 | `languageMismatch` — `src/crates/concat-api/src/speech.rs:534`, consumed by the watcher. |
| DEG-10 | the int8 hiss cure recorded (chained/attempted/impossible) — `scripts/auto_director.py:1679-1728`. |

## VERIFY residuals — what is actually wired

The Verify record lives in **`scripts/auto_director.py`** (the
watcher the server serves), not in `tools.rs`; `tools.rs` is the MCP
tool surface. Every entry emits id/check/status/reason/evidence and
lands in `verification-report.json` (`auto_director.py:1420-1423`).

| ID | check | wired at |
| --- | --- | --- |
| VERIFY-01 | black-frame-scan | `auto_director.py:940,1066-1091` |
| VERIFY-02 | caption-present | `1303-1342` |
| VERIFY-03 | transcript-sync ±60 ms | `1118-1131` |
| VERIFY-04 | visual-quality | `947,1071,1100-1108` |
| VERIFY-05 | media-availability | `1262-1281` |
| VERIFY-06/07 | the two always-run gates (transcript similarity, loudness) — folded into 03 and 13 by design (PR #119) | gates at `1118-1131`, `3702-3749` |
| VERIFY-08 | duplicate-clip-same-lane | `1229-1245` |
| VERIFY-09 | codec-match | `951-982` |
| VERIFY-10 | resolution-match | `955-1033` |
| VERIFY-11 | frame-rate-match | `959-1058` |
| VERIFY-12 | caption-on-frame | `1307-1363` |
| VERIFY-13 | true-peak-ceiling ≤ 0 dBFS | `815-817,1376-1387` |
| VERIFY-14 | post-final-edit-hash | `1182-1200` |
| VERIFY-15 | ai-voice-disclosure | `3758-3793` |

**One gap against the brief's 14-check E2E list: "both streams" has
no dedicated check.** The export read-back
(`src/crates/concat-export/src/lib.rs:1583-1608`) asserts the file is
readable, carries a **video** stream, and duration within ±0.5 s of
the timeline; an export with no audio stream passes it, and audio
presence is only implied by the loudness gate refusing
(`auto_director.py:3712`). "exists + readable" likewise rides the
Rust read-back, not the Verify record. A fix (a dedicated
audio-stream check in the Verify record) is prepared this session on
`fix/final-code`.

## UNVER-01..05 (verification items, not grep findings)

| ID | Status |
| --- | --- |
| UNVER-01 (antiX 1080p playhead/proxy/memory/export) | hardware-blocked; recorded `KNOWN_ISSUES.md` item 7 and the tracker. |
| UNVER-02 (real Intel/AMD VAAPI) | hardware-blocked; `KNOWN_ISSUES.md` item 3. |
| UNVER-03 (.deb in fresh container, `voice-signals --self-test`, OxiMedia signals) | **run against the published release this session; the check cannot pass there**: the `v0.2.8-funcut.12` `.deb` (x86_64, 197 MB, extracted with `dpkg-deb -x` in this container) contains exactly `concat` and `concat-motion-agent` — **no `voice-signals` binary**, no sense scripts. Expected per KNOWN_ISSUES item 9: `.12` (2026-11-12) predates the verified staging (build-app.yml:332-339, 397, 577-600, both arches). The named self-test still needs a build of current `main` — no Rust toolchain in this sandbox. Evidence: `UNVER-03-evidence.md` beside the session report. Not silently accepted. |
| UNVER-04 (motion clip: project create, save, reopen, play, export) | sandbox-verifiable with a built binary; not yet run. |
| UNVER-05 (Lottie playback + export + honest refusal) | sandbox-verifiable with a built binary; not yet run. |

## Open PRs found (the brief's precondition 3)

- **PR #130** — `feat/schema-generate` (SCH-02), head `31de8e5a`, opened 2026-11-14, author reports local tests green (66/66, 532/532, fmt/clippy clean) and "standing by for CI". Check state not readable from this sandbox.
- **PR #131** — `fix/startup-io` (RES-03), head `ff4ffc87`, opened 2026-11-14, same posture.

Both need their CI read and a merge-on-green, which requires the
write token (`$GITHUB_TOKEN`), absent this session. Do not re-write
what they already carry.

## What this session can and cannot finish

- Can (done, local branches, unpushed — no token): this audit; the
  P3-008 KNOWN_ISSUES entry (`docs/residual`); the both-streams
  Verify check in Python with the self-test run locally
  (`fix/final-code`).
- Cannot without the token / a Rust toolchain: merge #130/#131 on
  green; RISK-01/02 (Rust + a design decision the principal owns:
  where the monetized flag lives); the tools.rs:500 description fix
  (Rust, one string, but untestable here and "no fake green" applies);
  the release cut and its gate (Step 4 E2E needs the built app).
- Cannot without the ledger: TIME-01/02/04/05/07/11, ENV-10..14,
  XSTAGE-01..05/07..10, LATENT-02..04/06..08, P3-003/005, FALL-05,
  DEG-02, and the named "partial" half of DATA-01.
