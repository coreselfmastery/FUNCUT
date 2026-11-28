# A letter about Concat 0.2.8-funcut.16

To whoever installs this build,

This is the note that accompanies `v0.2.8-funcut.16`, cut on 28
November 2026. It carries everything `.15` carries - the 3D scene
layer, the headless AUTO EDIT tool, the multilingual transcription
repair, the sandbox install path - plus the five days of work since:
the AUTO EDIT spine that sets up its own models before it edits, the
all-signals pass that wires every reading the senses produce to the
decision it was measured for, the picture reader and its thirty model
doors, the sound design planned from measurement, the YouTube Shorts
profile, NVDEC/NVENC hardware paths, and the closure of the five gaps
the 27 November audit named.

If you are here for the short version: `.16` is `.15` with a one-click
AUTO EDIT that downloads what it needs, spends every signal it reads,
edits for the platform you picked, renders its own MP4 and proves the
result - and with the audit's five named defects closed, each with a
regression test.

## What this release is

`v0.2.8-funcut.16` is a re-cut of the 0.2.8 line, the same convention
as every fork tag. The installer, the updater and GitHub's "latest"
pointer read these tags, so `.16` is a full release even though the
version string inside the app still reads `0.2.8`.

## The setup-first spine

A cold machine no longer meets a missing model three stages in. The
run's preflight downloads every registry model in one job -
`models_download` with kind `all`, waited on with `analysis_wait`,
already-present rows skipped - before the first stage runs. The vision
critic's weights are part of that job now (see below), so the one call
a cold machine makes covers everything the edit spends.

The spine itself is nineteen registered stages - Preflight,
HandoffVoiceover, AudioRepair, Transcribe, VoiceSignals, Understand,
Plan, Timeline, Gather, Captions, Motion, Music, Sfx, Preview, Revise,
TimelineReady, Export, Verify, Watch - with retry and resume,
cancellation and rollback, progress events, parallel lanes, per-stage
receipts and idempotent edit operations. This release hardened the
runner's own laws: progress events reach the pane again, the setup
probe left the window's thread, a client that hangs up cancels the run
through to its rollback, analysis waits are attributed to the stage
that asked for them, a handled degradation is named in the receipt
rather than passed silently, and the run writes its report and receipt
however it ends.

## The all-signals pass

The senses now feed the cut, not just the log. Every reading the
voice, sound and picture senses produce is either spent by a stage of
the fixed spine or named with the reason it does not apply - a CI gate
(`scripts/check-signal-coverage.py --require-button`) fails the build
on an unledgered reading. The count this release ships: 71 voice
readings across 14 families, 76 non-voice readings of which 67 are
asked by the spine and 9 are reasoned exclusions - 100% accounted.

What that means in the timeline:

- The voice's own rhythm no longer misreads the stagger: `rhythmStagger`
  reads the whole-take rhythm layers the producer actually emits.
- The music-skip gate reads the onset rate, not the raw count, so a
  take with music in it keeps its bed refused for the measured reason.
- Emphasis-type readings reach the caption accents; generated sounds
  land on the file's measured peak offset (`syncOffset`), not its
  declared one.
- The sound design is planned from every signal the run measured, and
  the sound model has to prove it earns its place before it answers.
- The picture reader ships: a first-edit set of four models (shot
  boundary, depth, visual embedding, CLIP's vision tower) behind
  `models.download` kind `picture`, with thirty specialist doors on
  the visual shelf for the runs that want them, each door's download
  digest-pinned against the fetch table.

## The five audited gaps, closed

The 27 November audit named five defects that survived the .15 cut.
All five are closed in this release, each with a regression test:

1. **A continuity swap could seat footage the emotion gate refused.**
   The replacement walk now stays inside the approved set - the
   candidates the gate's own order passed and the picture's veto
   cleared - so a flagged slot with no approved alternate leaves the
   flag standing, honestly recorded, instead of being repaired with a
   clip a gate rejected.
2. **The recording-quality verdict died as a plan note.** The
   voice-quality door grades the floor before the repair chain runs;
   that verdict now gates the Music stage's own level decision. An
   audible floor (`home`, `rough`) is masked by a bed that sits 2 dB
   closer and ducks shallower; a clean floor (`broadcast`, `clean`)
   lets the bed whisper lower and duck deeper. Unmeasured changes
   nothing.
3. **Vertical A-roll letterboxed until the Revise stage caught it.**
   The Timeline stage now reads the measured source dimensions the
   view's media rows have always carried and crops every carried video
   clip whose aspect disagrees with the frame - the same `frameClip`
   fill the B-roll placement applies, before the first cut lands.
   Unmeasured media keeps the Revise stage's measured fill.
4. **The vision critic's half-gigabyte fetch was bounded inside one
   deep call.** The critic's two GGUFs are now a model set behind
   `models.download` kind `vlm` (profiles `mini`, `image`, `minicpm`),
   fetched by the same resumable, digest-pinned, cancellable job
   machinery as every other model - and the whole-registry `all` job
   carries the default profile, so the setup preflight covers the
   critic too. The files land in the same `vlm/models/<profile>`
   folder the setup call reads; `local_vlm` setup now only starts the
   server.
5. **The live-follow refresh dropped the selection on every autosave.**
   The refresh retains the selection's hold on the clips the refreshed
   document still carries - the same rule the editor applies to its
   own edits - and drops only the clips that left.

And one addition the audit asked for: **the Director's twenty-five
phases are a receipt now, not a promise.** The pane records which
phases the harness's own output named, and the acceptance refuses a
run whose output never named a phase - with the missing phases
numbered - instead of taking the exit code's word. The fixed spine's
nineteen stages keep their own receipts; the two systems stay
distinct, and the gate says so when it skips.

## The picture, the platform, the product

- **The stills aisle.** Stock searches fall back to licensed still
  images when the video shelf has nothing - a gap is no longer left
  empty because only moving pictures were asked for.
- **The orientation pick.** The one-click run builds for the frame you
  picked - horizontal, vertical or square - and reads the frame back
  to confirm it rather than assuming it.
- **The YouTube Shorts profile.** The measured publishing package:
  pacing, motion, caption and B-roll decisions shaped for the
  platform, plan-level quality checks, every Short window an
  actionable package, and a segment-by-segment picture and coverage
  check on the timeline.
- **The run renders its own MP4.** The spine exports, verifies and
  watches: the words are re-read against the export, the cards are
  proven, and the delivery carries the words the timeline actually
  plays.
- **Hardware decode and encode.** Decoding rides NVDEC where the
  machine has it, export rides NVENC - with the software paths
  unchanged beneath.

## Licence honesty

Unattended runs default to the licence-safe side: non-commercial beds
and research-only shelves stay refused unless a caller explicitly
opts into a personal edit with `monetized: false`. The BGM rows in
the models sheet carry the licence the engine enforces, and the check
compares them.

## Numbers

- 2,418 tests in the workspace (up from 2,325 at `.15`)
- 76 non-voice readings accounted for, 67 asked by the spine, 9
  reasoned exclusions; 71 exact voice readings across 14 families
- 19 registered execution stages; 25 Director phases, now receipted
- Sound placements land on the measured peak (`syncOffset` for
  generated files), not the declared `syncAt`; the bundled library's
  declared-table inaccuracies stay documented in `AUDIO_LIBRARY.md`

## The known issues that remain

The honest list is `KNOWN_ISSUES.md`: the translation backlog the
locales gate does not fail on stands; complex-script shaping in the
native renderer keeps its documented limits; the motion catalogue's
unshipped capabilities remain roadmap, not defect. Target-machine
proxy and VAAPI acceptance on real hardware remain open.

If you are reading this after installing: the one-click edit is the
button it always was. The difference is what stands behind it now -
every model it needs, every signal it reads, every phase it runs, and
a receipt to prove it.

- Concat contributors, 28 November 2026
