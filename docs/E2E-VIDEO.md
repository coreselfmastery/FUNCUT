# E2E-VIDEO — release acceptance script and recording plan

This is a proposed end-to-end acceptance script, not yet a verified,
shootable demo. The Motion page (#97), export queue (#96), and VAAPI
encoder selection (#95) are still open pull requests, not features on
main. Chapters that depend on them are conditional and must not be
recorded as working until those changes are merged and manually accepted.
The AI Director flow and target-laptop performance also require real
project and hardware evidence; a script is not evidence that either works.

**Target length:** 12–15 minutes at a relaxed pace. **Chapter markers**
below are the cut points; each chapter is one shot list entry.

## Preparation (before recording)

- Use the intended Linux target laptop. Record its model, CPU, RAM,
  storage, display mode and OS. Do not claim 4–6 GB readiness or smooth
  performance until target-machine benchmarks and a real editing session
  are recorded.
- `./install-linux.sh` run once; the app opened once; in **Settings →
  General**: proxies on (H.264 default), GPU on. In **Settings →
  Transcriber** and **Speech**: a whisper model and two voices downloaded
  (they work offline after that).
- A scratch folder with: two or three 1080p clips of your own footage
  (30–60 s each, some dialogue for captions), one music-worthy sound-off
  landscape shot, and one portrait phone clip (to show a 9:16 edit).
- The OpenVid index present (`Settings → General` shows the library), and
  the network up for the two moments the stock/music shelves fetch.
- A second, empty timeline in the demo project (added live in Chapter 9).
- Screen recording at the display's native rate. Record the actual
  sustained capture rate and dropped frames; do not imply GPU acceleration
  or a frame-rate target without measuring it.
- The Geneva sidecar installed (the installer brings it) so the motion
  passes run.

## Chapter 1 — the door (0:00)

**Show:** the start screen. Recents as poster cards, search, sort,
drop-to-start.

**Do:** drag the scratch folder's landscape clip onto the window. The
project opens on it without a wizard.

**Say:** *"The project opens directly into the editor. We will identify
any networked services, provider calls and downloaded assets used in this
demo rather than making blanket privacy or offline claims."*

## Chapter 2 — the cut (0:45)

**Show:** the desktop workspace: the Blender-style dock — media, preview,
inspector, timeline, keyframes — drag a pane to another seat and watch
the swap.

**Do:** import the other clips; drag them onto lanes; make three cuts
with S (split), a trim by dragging an edge, one razor-band cut; turn
magnetic on and show a ripple delete; marquee-select two clips and move
them together; lock a lane and show edits refuse it; Ctrl+Z twice and
Ctrl+Y once — undo and redo everywhere.

**Say:** *"Multi-track, untyped lanes, magnetic edits, ripple, marquee,
lock — and every move is one undo away."*

## Chapter 3 — the sound of the cut (2:30)

**Show:** the Audio page (bundled 530 CC0 sounds), the Music page
(1,025 CC0 tracks), and the timeline's audio: waveforms, the draggable
volume line with unity snap, fade ramps on a clip's corners.

**Do:** drag one music track under the cut; duck it under the dialogue by
dragging the volume line; add a fade-out; audition two sounds from the
Hollywood trailer pack and drop one riser on a cut.

**Say:** *"The library is local and CC0 — the MobyGratis shelf is
non-commercial, and the app says so where you pick it."*

## Chapter 4 — captions and a voice (4:00)

**Show:** the captions sheet: transcribe the dialogue clip (the whisper
model already downloaded), then the speech sheet.

**Do:** transcribe; style all captions at once (batch title styling);
fix one line's words; then type one new sentence and render it as a voice
with an expression tag; show the voice picker and a saved voice.

**Say:** *"Captions and speech are models on your disk — they work with
the network off, and your audio never leaves the machine."*

## Chapter 5 — the look (6:00)

**Show:** the inspector's full panel for one clip: transform with snap
guides, the grading panel (wheels, curves), the Filters page with
audition-by-click, Effects (123), Transitions (122), and an adjustment
layer from the Adjustments page.

**Do:** one LUT import; one grade with the wheels; one filter auditioned
and applied; drag one transition onto a cut and set its length; add an
adjustment layer over the whole edit and put the grade there instead.

**Say:** *"Three hundred and twenty-seven packages, all local, all
keyframeable — and a look can live on the clip or over the whole cut."*

## Chapter 6 — the craft tools (7:30)

**Show:** the inspector's heavier tabs in one pass: chroma key, masks
with grips, crop, stabilize (with its measured zoom), background removal
(auto person, then one smart brush painted live), enhance with the
before/after split, speed with the curve editor and a preset ramp.

**Do:** stabilize the shakiest two seconds; remove the background of a
standing shot with one brush; reverse half a second; ramp speed on one
clip. Then open the keyframes pane: dopesheet, curves, bezier handles on
one key, an ease preset.

**Say:** *"These are the craft tools available in this build. We will
show their actual output and responsiveness on the recorded machine."*

## Chapter 7 — stock, stickers, text, motion (9:00)

**Show:** the OpenVid page (search, filters, a clip fetched per use), the
Stickers page, the Text page, and—only after PR #97 is merged and accepted—
the **Motion page** (the project's comps: name, frame, length, and bake
availability).

**Do:** search OpenVid for a wide establishing shot and drop it at the
head; put one sticker on the footage lane; add a title from a preset and
retype it. After PR #97 is merged and accepted, add a baked comp, verify
its placement, undo/redo it, move it, save, close and reopen the project,
then verify the clip and its rendered frames.

**Say:** *"The stock shelf is the OpenVid research dataset — a research
licence, and the app says so before first use. Motion comps come from the
director or the motion tools; the page places them, and the timeline
treats them like any clip: move, trim, undo, save."*

## Chapter 8 — the AI director (10:30)

**Show:** the **Forge page**: the script box, the voice and look choices,
Start.

**Do:** after the Director's real-project acceptance run is complete,
paste a three-sentence script, select the voiceover language and Start.
Capture the actual provider calls, phases, asset resolution, timeline
commands and failures. Verify audio, video, timing, captions and final
export; do not treat a phase log or a populated timeline as acceptance.

**Say:** *"The director is an agent on the app's own MCP server — a
hundred and nine tools, senses for cuts, beats, dead air, and an emotion
reader. What it edits, you can undo; nothing it does is outside the
editor."*

## Chapter 9 — two cuts, one queue (12:00)

**Show:** the export sheet and the choices actually present in the tested
build. Show the **timeline picker** and multi-job queue only after PR #96
is merged and accepted. Show the **Encoder** row (Automatic, GPU/VAAPI,
CPU) only after PR #95 is merged and VAAPI is tested on supported hardware.

**Do:** add a second timeline and export both cuts. If PR #96 is merged,
queue each named timeline and verify that each job renders the correct
clips, frame and rate, with independent progress, cancellation and
reopen behavior. If PR #95 is merged, test VAAPI on supported hardware
and verify the selected encoder and software fallback. Otherwise, show
only the export behavior that this build has actually passed.

**Say:** *"Describe queue isolation and hardware encoding only if those
specific paths have passed their merged CI and manual acceptance. Do not
present software fallback as proof that VAAPI itself was exercised."*

## Chapter 10 — the close (14:00)

**Show:** the finished files in the folder; the two timelines side by
side; Settings → About → copy system info.

**Do:** reveal both files; play ten seconds of each from the folder.

**Say:** *"Two cuts, one project, one machine, no cloud. That's the whole
point."*

## The honest notes for the narrator

- The stock shelf's licence (research dataset) and the MobyGratis
  non-commercial rule are **said on camera**, not footnoted — the app
  says both at the shelf itself, and the video should too.
- Report the target laptop's actual RAM and measured results. Describe
  proxy use, scrubbing and return-to-original-frame behavior only after
  verifying them with representative 1080p long-GOP footage. Do not claim
  speedup or smoothness from the presence of proxy code alone.
- The director's run needs the provider key it asks for; the video shows
  the key field, not the key.
- Do not claim the whole demo is offline: the Director may require a
  provider key and network access, and stock/music shelves may fetch
  remote assets. State which steps used the network and which completed
  offline.
