# E2E-VIDEO — the one-take tour of everything FUNCUT is

This is the shootable script for the end-to-end demo video: one project,
taken from an empty window to a finished, exported film, touching every
layer of the app in the order a real edit would. Every step names what is
on screen, what to do, and what to say; nothing here is staged — every
feature named is in the tree, and where a thing needs a model or a
download first, the preparation section says so.

**Target length:** 12–15 minutes at a relaxed pace. **Chapter markers**
below are the cut points; each chapter is one shot list entry.

## Preparation (before recording)

- A Linux laptop (the target class: 4–6 GB RAM is the honest demo — the
  app is tuned for it, and the video should show it flying there).
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
- Screen recording at the display's native rate; 60 fps if the machine
  holds it, 30 otherwise — the compositor is GPU-driven either way.
- The Geneva sidecar installed (the installer brings it) so the motion
  passes run.

## Chapter 1 — the door (0:00)

**Show:** the start screen. Recents as poster cards, search, sort,
drop-to-start.

**Do:** drag the scratch folder's landscape clip onto the window. The
project opens on it without a wizard.

**Say:** *"Everything runs locally — no account, no upload, no
watermark. Drop footage on the door and you are editing."*

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

**Say:** *"The same engine a desktop suite has — masks, keys, tracking-
class stabilize, AI restore — on a laptop, because the compositor is the
GPU's."*

## Chapter 7 — stock, stickers, text, motion (9:00)

**Show:** the OpenVid page (search, filters, a clip fetched per use), the
Stickers page (1,285), the Text page (31 presets), and the **Motion page**
(the project's comps: name, frame, length, whether a bake is on disk).

**Do:** search OpenVid for a wide establishing shot and drop it at the
head; put one sticker on the footage lane; add a title from a preset and
retype it; open the Motion page, pick a baked comp, press **Add** — a
motion clip lands at the playhead on its own lane; undo it; redo it;
move it; save the project, close it, reopen it — the clip is still there,
bake and all.

**Say:** *"The stock shelf is the OpenVid research dataset — a research
licence, and the app says so before first use. Motion comps come from the
director or the motion tools; the page places them, and the timeline
treats them like any clip: move, trim, undo, save."*

## Chapter 8 — the AI director (10:30)

**Show:** the **Forge page**: the script box, the voice and look choices,
Start.

**Do:** paste a three-sentence script; pick the voiceover-in language;
Start. The log streams the director's twelve measured phases — research,
plan, footage slots, motion, the edit — and the timeline fills with the
director's clips as they land, every one of them an ordinary undoable
command.

**Say:** *"The director is an agent on the app's own MCP server — a
hundred and nine tools, senses for cuts, beats, dead air, and an emotion
reader. What it edits, you can undo; nothing it does is outside the
editor."*

## Chapter 9 — two cuts, one queue (12:00)

**Show:** the export sheet: the **timeline picker** (every timeline in the
project, its frame and clip count under it), the ladder, the codec rows
with their encodability notes, the **Encoder** row — Automatic, GPU
(VAAPI), CPU — the size estimate on the button.

**Do:** add a second timeline (a 9:16 cut of the phone clip, three
clips); queue the landscape edit as `demo-wide.mp4`; switch the picker to
the portrait timeline and queue `demo-tall.mp4` — the sheet stays up, the
first job's row runs with its own progress and cancel, the second waits
its turn; close the sheet on the running queue and reopen it — the queue
is where it was, and the app kept rendering. Cancel nothing; let them
finish; **Show in folder** on the finished row.

**Say:** *"One render at a time — the queue is the waiting room, and
every job is its own: its own timeline, its own progress, its own cancel.
The encoder can be the machine's own video hardware by name, with the
software encoder behind it as the fallback."*

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
- If the machine is the 4–6 GB class, say so when the export runs: the
  proxy system is what keeps the scrub smooth, and the queue renders on
  the same budget.
- The director's run needs the provider key it asks for; the video shows
  the key field, not the key.
- Nothing in this script requires a network after the two downloads —
  say it when the captions render offline.
