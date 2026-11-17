# A letter about Concat 0.2.8-funcut.14

To whoever installs this build,

This is the note that accompanies `v0.2.8-funcut.14`, cut on the tree
that carries everything `.13` left behind. `.13` (16 November 2026) was
the largest jump the fork had taken, but the day after it shipped the
golden Geneva lane went red on `main`; this release is the tree with that
lane green again, plus the work that landed in the hours and days after
the `.13` tag, plus two API-wiring repairs that make a key you typed once
behave the same wherever you use it.

If you are here for the short version: nothing in this release changes
what the editor is. It is the same motion engine, the same AUTO EDIT
button, the same live stock shelves, the same gates. What it adds is the
front of new work that missed `.13` by hours - a shared-memory frame
pipe, a streaming playback ring, safe zones and a WCAG gate - the fix
for the one defect `.13` shipped with, and a small set of connection
repairs. The details are below.

## What this release is

`v0.2.8-funcut.14` is a re-cut of the 0.2.8 line, not a new version
number - the same convention as every fork tag. The installer, the
updater and GitHub's "latest" pointer read these tags, so a `.14` is a
full release even though the version string inside the app still reads
`0.2.8`.

## The fix `.13` owed: the dry cut

`.13` shipped with a real defect that the golden-frame harness was the
first caller to meet. A choreographed cut attaches its own sound to the
entering scene's comp; the flatten drops the audio-reaction node the
sound rides on, so the clip is declared as an asset but nothing
references it; and the plan then tried to copy every declared asset into
the render folder and aborted the whole render when one was not there -
over a sound the timeline never played.

The fix (PR #156) is in `concat-motion`'s sequence planner: a declared
asset whose file is not beside the comp is left out of the timeline and
named in the warnings. A comp that carries a sound nothing plays is a
**dry cut** - it renders, quietly, and says so. The fix is deliberately
narrow: an asset a clip still *names* is still refused when its file is
missing, so a missing picture cannot hide behind a warning. A regression
test pins the behaviour, and the golden lane passes against the real
Geneva sidecar with the harness unchanged. `main` is green again.

## What landed after the `.13` tag

These reached `main` in the hours after `.13` was cut. They are new work,
not fixes to `.13`, and `.14` is the first release to carry them:

- **The shared-memory frame pipe** (`concat-motion`). A page-aligned
  header and RGBA slots published through an acquire/release turnstile;
  named POSIX shm for two processes, an `memfd` for two ends of one. The
  Geneva bake and frame seams offer it to the sidecar when
  `CONCAT_FRAME_PIPE` names it, and read the frames back from the bytes;
  PNGs on disk either way. This is the seam that lets a render stop
  round-tripping frames through the filesystem.
- **The playback's streaming ring, four frames deep** (`concat-media`).
  The prefetcher kept nothing the cursor passed; it now keeps the newest
  four, so a scrub back is answered from memory instead of a re-decode,
  and the ring survives a seek.
- **Safe zones, the WCAG gate, their patches, and incremental bakes**
  (`concat-motion`, `concat-mcp`). Two lint families: L600, the platform
  safe zones (9:16 Shorts/Reels, 16:9, 4:5), and L610, the contrast gate
  (4.5:1 at body sizes, 3:1 from 24 px up). Both carry `patches()` that
  apply a fix and audit clean after. Incremental bakes: `diff()` reports
  which frames a comp change can reach, across sub-comps.
- **The GSAP grid stagger is bounded** (`geneva-adapter`). A stagger over
  a grid whose cell count ran away could size a work buffer without a
  ceiling; the dimension is bounded now.

## The wiring repairs

Two handoffs were wired into the AUTO EDIT path but not the Forge
Director launch, so a key the user typed once behaved differently
depending on which button they pressed. Both are fixed (PR #158):

- **The stock keys reach the Director.** The Pexels and Pixabay keys the
  Stock page keeps are now passed to the spawned `forge` harness, the way
  the AUTO EDIT path already exported them; the Director's gather stage
  shops the live shelves with no further configuration.
- **A provider key activates the hosted critic.** `vlm.detect()` now
  resolves a named provider key in the environment
  (`GROQ`/`GEMINI`/`ZAI`/`OPENROUTER`) as the hosted vision critic when no
  `vlm.json` is saved - after the saved config, before the local
  candidates. A GLM/Z.AI key typed once in the Forge page now drives
  `craft_critique`'s hosted tier instead of leaving it on the
  measured-pixel fallback. `FUNCUT_VLM_DISABLE_HOSTED=1` keeps frames on
  the machine, and the critique reply already names the engine that
  answered, so a fall-back is visible, never silent.

Two smaller reliability fixes ride along: the Slint Skia image conversion
returns "draw nothing" for a static texture instead of the upstream
`todo!()` that aborted the process (PR #157), and the GSAP `drawSVG` trim
windows and repeated SVG path segments are validated rather than trusted
(PRs #153, #154).

## The completion audit's two fixes

The 2026-11-16 completion audit (a wired-but-not-properly pass, not a
feature list) found one integration defect and one place where a stage
could end before its work was done. Both are in this release:

- **One run at a time on the shared project** (PR #160). The Forge page
  offers two ways to edit the same sibling project - the agent run (Start)
  and the unattended AUTO EDIT - and nothing stopped them from running at
  once, so a Start during an auto edit (or the reverse) put two writers on
  one timeline. The second run is now refused, its button greys, and the
  user is told which run to stop. The labels are in all 26 locales, and a
  regression test holds the contract.
- **No stage ends before its work is done** (PR #159). A stage's calls
  could all land while its own check was still unanswered, so the run
  advanced on work it had not verified; the 12-stage auto-edit program now
  waits for each stage's gate before it moves on.

The audit's full classification - what is wired and proven, what is wired
but needs hardware or a real run, and the release state - is in
[`docs/COMPLETION-AUDIT-2026-11-16.md`](COMPLETION-AUDIT-2026-11-16.md).

## What this release does not prove

A release note that reads as more checked than it was is worse than no
note. So, plainly:

- **No target-hardware acceptance.** The AUTO EDIT button and the
  director's Verify record are unit- and CI-tested, but no real
  brief-to-export run on the intended low-memory Linux laptop has been
  recorded. The release rules do not accept CI evidence alone for that.
- **VAAPI hardware encode is opt-in and unvalidated on real GPUs.** The
  encoder is wired and gated on the DRI render node, but driver
  compatibility and fallback behavior still need testing on real Intel
  and AMD hardware.
- **The Geneva sidecar is a pinned closed-source binary (1.6.0).** Its
  limits are documented in `docs/MOTION.md`: no path drawing, no
  variable-font axes, keyframes in px and % only.
- **The BGM library's FreePD shelf depends on archive.org.** If the
  Internet Archive is slow or reorganizes an item, `bgm_prepare` and
  `bgm_fetch` fail through no fault of the code.

`KNOWN_ISSUES.md` is the authoritative list; read it before you trust a
feature on your own machine.

## Getting it

Every architecture Concat ships on gets a self-contained bundle, and
they are attached to the `v0.2.8-funcut.14` release:

| | x86_64 | arm64 |
|---|---|---|
| Linux | `linux-x86_64.deb`, `.rpm`, `.pkg.tar.zst`, `x86_64.AppImage` | `linux-aarch64.deb`, `.rpm`, `.pkg.tar.zst`, `aarch64.AppImage` |

`SHA256SUMS` lists each file's checksum, and `manifest.json` names the
bundle set. The `.deb` and `.rpm` carry the FFmpeg runtime libraries
beside the binary; the AppImage needs no `libfuse2`.

## The ledger

The per-commit list is generated by `.github/release-notes.sh` from the
range `v0.2.8-funcut.13..v0.2.8-funcut.14`, and the release workflow
attaches it to the GitHub release when the tag builds. This letter is the
prose around that list: what the entries add up to, and what the release
still owes.

---

This letter was written by an AI agent (OpenHands) on behalf of the
maintainer, from the repository's own commit history, CI logs, release
records and `KNOWN_ISSUES.md`.
