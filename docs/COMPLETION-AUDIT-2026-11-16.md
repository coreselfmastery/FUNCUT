# Completion audit — 2026-11-16

This is a **completion and integration audit**, not a feature list. It answers
one question: of everything FUNCUT claims to do, what is wired end to end and
proven, what is wired but unproven, and what is not wired at all. It records
the state observed on `main` at `fa6f566a` (the merge of PR #157) on
2026-11-16, the open fixes in flight, and the residue that only hardware or a
real project run can close.

## 1. The two entry points on the Forge page, and why they are not duplicates

The Forge page offers two ways to make a video. They look like two automations
because they are two buttons, but they are **two layers of one pipeline**, not
two pipelines:

| | **Start Forge** | **AUTO EDIT** |
| --- | --- | --- |
| What drives it | the `forge run` harness + the `funcut-director` skill (an agent, a model) | the `concat-director` runner (deterministic Rust, no model) |
| Stages | the full creative pipeline: gather, motion, plan, preview, revise, watch | a fixed 12-stage subset of the same `Stage` enum |
| Cost | model calls; free tiers work | no model calls |
| Judgement | B-roll, captions, the grade, the mood | measured, verified, unattended |
| Output | a directed edit | the voiceover edit, shipped and verified |

They share one handoff: a sibling project built beside the window's project,
the voiceover reconstructed on its own lane. The creative layer is the agent's;
the deterministic spine is the runner's. AUTO EDIT is not a second product —
it is the part of the pipeline that needs no judgement, run on its own.

**The bug this audit found and fixed:** nothing stopped both from running at
once. Both write the same sibling project through the window's own server, so a
Start during an auto edit (or the reverse) put two writers on one timeline.
`src/crates/concat/src/panes/forge.rs` now refuses the second run, greys its
button (`forge-page.slint`), and names the run to stop. See PR #160.

## 2. "AUTO EDIT" and "the 12-stage pipeline" are the same thing

There is one stage enum, `Stage::ALL`, of **17** stages:

```
handoff-voiceover, transcribe, audio-repair, voice-signals, understand,
plan, gather, motion, timeline, captions, music, sfx, preview, export,
watch, revise, verify
```

AUTO EDIT runs a **12**-stage subset of it, in this order:

```
handoff-voiceover, audio-repair, transcribe, voice-signals, understand,
timeline, captions, music, sfx, export, verify, watch
```

The five the auto edit does **not** run are the judgement stages — `plan`,
`gather`, `motion`, `preview`, `revise`. Those need a director's eye (which
B-roll, which comp, what to change after watching), so they belong to the agent
run. The 12-stage program is the deterministic remainder: clean the audio,
read the voice, place the timeline, caption it, bed it, accent it, export it,
verify it, watch it. So "AUTO EDIT" and "the 12-stage pipeline" name the same
object. The 17-stage enum is the whole pipeline; the 12-stage program is the
auto edit's slice of it.

PR #159 fixes the one place those stages ended before their work was done: a
stage's calls could all land while a silent-success gate was still unanswered,
so the run advanced on work it had not verified.

## 3. What is wired and proven

- The 113 MCP tools and the 55 edit commands are wired and dispatched.
- The director runner drives both programs (agent-directed and auto edit) over
  the window's own server or a headless one.
- The export path reads its own output back and checks it opens, contains
  video, and matches the requested duration.
- The stage gates (verify's verdict, the music bed's presence) are enforced in
  the runner: a gate no reply answered fails the stage.
- `scripts/locales.py --check` is green: every interface string has all 26
  locales.

## 4. What is wired but unproven (needs hardware or a real project)

These are not code defects. They are evidence gaps that CI cannot close, and
they are the real remaining work (tracked in `KNOWN_ISSUES.md`):

- **A real brief-to-export run of the AUTO EDIT button** on the intended
  laptop (KNOWN_ISSUES 8 and 11).
- **1080p proxy performance** measured on the target low-memory machine (7).
- **VAAPI hardware encode** on real Intel/AMD hardware (3).
- **Motion save/reopen and final-export acceptance** (6).
- **Target-machine timing/memory** for realistic motion workloads.

## 5. The release

The release **`v0.2.8-funcut.13` is published** (2026-11-16) with ten assets:
the AppImage, deb, rpm and pkg.tar.zst for x86_64 and aarch64, plus
`manifest.json` and `SHA256SUMS`. Its tag `ebfd9e30` is an ancestor **28
commits behind** current `main`, so the published bundles predate the
current-main work. Cutting `v0.2.8-funcut.14` owes, in order: green `main`,
the AUTO EDIT acceptance run, and the hardware verification above.

## 6. Open fixes in flight

| PR | What it fixes |
| --- | --- |
| #159 | the 12-stage program's stages ending before their work is done |
| #153 | the GSAP `drawSVG` window validation |
| #160 | one run at a time on the shared project (this audit's finding) |

## 7. Residue that is not this repository's to close

The FreePD BGM shelf depends on archive.org (KNOWN_ISSUES 12); the sherpa-onnx
2.x migration is the one remaining dependency fix (KNOWN_ISSUES 10), and the
seven advisories it carries are reviewed and ignored per-ID with proof.

**Status rule:** FUNCUT is not "release ready" from CI alone. The code paths
are wired and tested; the acceptance runs and the hardware measurements are
what remain, and this page does not substitute for them.
