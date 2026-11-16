# A letter about Concat 0.2.8-funcut.13

To whoever installs this build,

This is the note that should have gone out with `v0.2.8-funcut.13`. It
was written because the tag exists and the release did not: the tag was
pushed on 16 November 2026, the Geneva golden-frame job on `main` went
red the same day, and the release workflow will not publish a bundle
until that job is green. So this letter says three things at once - what
is in the release, why it did not ship when it was tagged, and what the
release still does not prove.

If you are here for the short version: the last release you could
download was `v0.2.8-funcut.12` (12 November 2026). This one is the
largest jump the fork has taken since then - two hundred and thirty-five
commits of motion engine, auto-editing, live stock shelves and a long
hardening sweep - and it was held back by a single missing sound clip
that no one played. That clip is fixed. The details are below.

## What this release is

`v0.2.8-funcut.13` is a re-cut of the 0.2.8 line, not a new version
number. The fork's releases are its own tags on top of upstream 0.2.8;
the installer, the updater and GitHub's "latest" pointer all read those
tags, so a `.13` is a full release even though the version string inside
the app still reads `0.2.8`.

The notes the release workflow generates between `v0.2.8-funcut.12` and
this tag run to well over a hundred entries. This letter groups them by
what a person running the app would actually notice, and it names the
caveats that the raw list does not.

## The headline work

### The motion engine grew a body

The Geneva motion path - the comps, presets and kinetic typography the
editor has carried for a while - gained the pieces that let an agent
compose real motion rather than decorate a slideshow:

- **Track mattes, range selectors and animated ramps**, plus importers
  for SVG, Rive and SRT.
- **Template preprocessing and a thirty-two preset library**, with
  continuous mood morphing and the cut's own sound (the whip, the
  zoom-through and the rack focus each carry a shelf clip now).
- **A GSAP parity shelf** - forever repeats, stagger objects, `morphSVG`,
  `drawSVG` and `splitText`.
- **A Lottie parity shelf** - text, trims, gradients, dashes, repeaters,
  stars and expressions, on top of the native rasterlottie playback that
  already landed.
- **The audio reaction** - bands, bindings, lipsync, foley and mood - and
  a **bone rig** with 2D inverse kinematics for puppet chains.
- **The 2.5D/3D chain** - layer brackets, a six-degree-of-freedom spline
  camera, depth of field and z-ordering - with a **distortion catalogue**
  and velocity-adaptive motion blur.
- **Boolean outlines, a tapered stroke, and the native typography shelf**
  (text on a path, inline spans and the fit modes).

If a comp of rigs, cameras and music renders empty, that is now a bug:
the motion census counts every node kind, so the tool's own reply says
what the comp holds instead of reporting nothing.

### The AUTO EDIT button

The button that turns a voiceover into a finished edit, stood on the
window, uses the editor's whole measured power now: fanned per-take
repair, transcription and signals; a derived tighten that cuts only the
dead air the reading classified; captions on the transcript's own words;
the CC0 music bed shopped and ducked; accents on the emphasis peaks; and
a verify that reads the timeline as it ended up. A run that can retry
resumes from the stages it completed, and a cancelled run's rolled-back
stages leave a record behind.

The runner behind the button repairs, gates, verifies and enriches the
edit it runs, and it names its whole judgment stack - the maestro and
clip-skills shelves are consulted, not merely decorated with, and a plan
whose signal log never names a craft module is warned about.

### The shelves shop for real

The live rights-cleared stock shelves - Pexels and Pixabay over their own
APIs - search by words and orientation, fetch one clip a round trip, and
import onto a lane with the licence in the reply. The Stock page shops
them beside the local index, the MCP trio (`stock_search`, `stock_fetch`,
`stock_import`) rides the director's surface, the JSON-RPC API carries
the three requests, and the AUTO EDIT run exports the keys a monetized
edit stands on.

### The gates that refuse

Several of these are the difference between an edit that looks fine and
an edit you can stand behind:

- **The licence gates are enforced, not advised** - a monetized brief
  refuses OpenVid stock and MobyGratis beds, and unknown music licence
  metadata fails closed.
- **The export's missing audio stream is named, not shipped** - a silent
  export is refused, and exports are refused outright when audio probing
  is unavailable.
- **The plan contract the executor keeps** - a short supplied visual
  description is refused rather than silently swapped for the narration,
  and the schema says the same in template, canonical and compact.
- **The run is idempotent** - every stage checks, every edit is keyed,
  the idempotency answers are bounded and durable, and a failed autosave
  is said rather than swallowed.

### The hardening

A long sweep of panic paths came out of the MCP tool surface and the
window: poisoned locks no longer take down the decode worker, the music
library, the runner or the effect loader; a raced timeline merge or bake
reports an error instead of panicking; a vanished critic target says so;
a panicking run thread still ends, so the button cannot wedge on
"running". Thirteen RustSec advisories were cleared with real upgrades,
and `zip` 9 either reads a dotLottie entry name or refuses the archive.
The AppImage survives a machine without `libfuse2`, the software renderer
is a Recommends so a GPU-less box can still preview, and `--help`, a
headless `--smoke` and the opt-in update check all behave.

## The one thing that held it back, and the fix

The Geneva golden-frame job - the lane that renders a committed board at
six fixed instants and compares each frame against a committed image
with DSSIM - went red on the day of the tag, with this:

```
the frame at 0.8s refused to render
  "error": "copying an asset into the render audio/whoosh-hard-cut.ogg:
            No such file or directory (os error 2)"
```

The cause was not the harness. It was a real defect in the product, and
it would have hit a person, not only CI:

1. A choreographed cut attaches its own sound to the entering scene's
   comp. The whip pan, the zoom-through and the rack focus all name a
   clip from the shelf.
2. The flatten drops the audio-reaction node the sound rides on, so the
   clip is **declared as an asset but nothing references it**.
3. The plan still tried to copy every declared asset into the render
   folder, and it aborted the whole render when one was not there - over
   a sound the timeline never played.

So any reel with a choreographed cut died at render if its shelf clip
was not already beside the project, and the golden board is the first
caller to have met it. The fix is in `concat-motion`'s sequence planner:
it now leaves a declared asset out of the timeline when its file is not
beside the comp, and names it in the warnings. A comp that carries a
sound nothing plays is a **dry cut** - it renders, quietly, and says so.

The fix is deliberately narrow. An asset a clip still *names* - a plate,
a font, a background - is still declared and still refused by the engine
when its file is missing, so a missing picture cannot be silently hidden
behind a warning. Only the orphaned asset is dropped. A regression test
pins the dry-cut behavior, and the golden lane passes against the real
Geneva 1.6.0 sidecar with the harness unchanged.

## What this release does not prove

A release note that reads as more checked than it was is worse than no
note. So, plainly:

- **No green `main` until this fix lands.** The Geneva engine job is red
  on `main` at the time of writing, for exactly the reason above. The
  release workflow builds a tag only after CI passes on it, so `.13` will
  not publish until this fix is merged and `main` is green again.
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
  variable-font axes, keyframes in px and % only. The guards are aligned
  to that binary, not to a wish.
- **The BGM library's FreePD shelf depends on archive.org.** If the
  Internet Archive is slow or reorganizes an item, `bgm_prepare` and
  `bgm_fetch` fail through no fault of the code.

`KNOWN_ISSUES.md` is the authoritative list; read it before you trust a
feature on your own machine.

## Getting it

Once `main` is green and the tag publishes, every architecture Concat
ships on gets a self-contained bundle:

| | x86_64 | arm64 |
|---|---|---|
| Linux | `linux-x86_64.deb`, `.rpm`, `x86_64.AppImage` | `linux-aarch64.deb`, `.rpm`, `aarch64.AppImage` |

`SHA256SUMS` lists each file's checksum. The `.deb` and `.rpm` carry the
FFmpeg runtime libraries beside the binary; the AppImage needs no
`libfuse2`.

## The ledger

The per-commit list is generated by `.github/release-notes.sh` from the
range `v0.2.8-funcut.12..v0.2.8-funcut.13`, and the release workflow
attaches it to the GitHub release when the tag builds. This letter is the
prose around that list: what the entries add up to, what held the release
back, and what the release still owes.

---

This letter was written by an AI agent (OpenHands) on behalf of the
maintainer, from the repository's own commit history, CI logs, release
records and `KNOWN_ISSUES.md`.
