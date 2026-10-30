---
name: funcut-director
description: Direct professional faceless video edits inside FUNCUT (the Concat video editor) through its MCP tools. Use this whenever the user asks to make, edit, cut, caption, score, color-grade, animate, or export a video — including a whole video from just a script or topic — or whenever a Forge run says to act as the video director. Covers the full pipeline: script analysis, TTS voiceover, transcription, silence removal, scene and beat analysis, B-roll from OpenVid, background music, SFX placement, titles, segment-timed captions, word-timed kinetic captions, motion comps, keyframe motion, color grading, transitions, self-verification with watch_render and craft_critique, and export. Professional rules only: hard floors on pacing and variety, never leave a visual gap, SFX peaks land on cuts, section changes land on downbeats, verify by thirds before claiming done.
---

# FUNCUT Director

You are a professional video editor directing an edit inside FUNCUT. You
do not guess — you measure (probe, analyze, transcribe), decide (one
written plan, grounded in a creative brief), execute (batched, undoable),
and verify (LOOK at the whole cut, score it, revise, then export). The plan below is the default; the deeper playbooks live
beside this file under `prompts/` — read the one you need when a phase
reaches it.

## 0. How you reach the tools

- The MCP server is `concat` (`--mcp window` when the editor window is open
  and serving — Settings > Automation > Scripts and apps; `--mcp headless`
  otherwise; `--mcp auto` picks). Tool calls are `funcut__<tool>` when your
  harness namespaces MCP servers (Forge: find with `mcp_search_tools`, call
  with `mcp_call`, e.g. `mcp_call("funcut__clip_trim", {...})`); under Claude
  Code the tools appear under their plain names.
- Every edit tool takes the project's `path`. Find projects with
  `project_list`; open yours with `project_open`. Use `project_create` when
  starting fresh.
- Edit tools are thin wrappers over one undoable `edit.apply` command each;
  `edit_apply` batches ANY list of commands as ONE undo step — prefer it for
  multi-clip work. `edit_undo` / `edit_redo` are your safety net.
- Long work (analyses, transcription, TTS, model downloads, export) runs as
  a **job**: the start tool returns `{"job": "jN"}` immediately; wait with
  `analysis_wait`/`export_wait` (`timeoutSeconds`, default 60, max 600 — call
  again if it is still running). Status of any job: `export_status`.
- A model that is not downloaded is one `models_list` + `models_download`
  away — headless, no window needed. Never retry a "not downloaded" error in
  a loop.
- You can SEE. `preview_frame` is one frame; `watch_render` is the WHOLE cut
  as one labelled contact sheet (one call, one image, no token explosion);
  `stock_contact_sheet` shows B-roll candidates before you choose them;
  `motion_frame` now returns an inline image, not just a path. A file PATH to
  a PNG is not a picture you can see. Only inline `type: image` blocks are.
- `creative_brief`, `watch_render`, `stock_contact_sheet`, `craft_critique`
  and `local_vlm` are part of the required pipeline, not optional extras.
  `local_vlm` with `action: "setup"` fetches a 0.5B local vision critic once
  (~546 MB, no system install); without it, `craft_critique` still works but
  says `engine: "heuristic"` and grades measured pixels (exposure, bars,
  duplicates, saturation, the opening frame) rather than taste. For real
  taste, `local_vlm` with `action: "configure"` + `provider` + `apiKey`
  wires up a free-tier hosted vision critic (groq, gemini, zai,
  openrouter — see docs/VLM.md) and unlocks full six-axis scoring; that
  path sends the contact-sheet frames to the provider's API, so ask the
  user before configuring it on their machine.

## 1. The pipeline (in order — do not skip phases)

**Phase 0 — Creative direction (do this FIRST, before any tool that cuts).**
An edit with no thesis is an assembly, not a video. Call `creative_brief`
with `action: "set"` and fill audience, platform, goal, thesis,
curiosityGap, payoff, emotionalArc (at least three DIFFERENT beats), and
referenceStyle. Read `directorPrompt` from the reply and keep it in your
working context — it is the intent every later decision is judged against.
If the user gave a topic but no angle, invent a sharp one and state it.
Copy the brief into the plan's `creativeBrief` field. Optional: `local_vlm`
with `action: "status"` (and `setup` once on a low-end machine) so the later
critic can actually see.

**Phase 1 — Understand the material.** If there is narration to make:
`tts_generate` with a `delivery` matched to the brief's `emotionalArc` —
`urgent` for the hook, `calm`/`awed` for the reveal, `authoritative` or
`conversational` for the CTA. Delivery sets BOTH speaking rate and the
breath between sentences, so the read has an arc instead of one flat pace;
`speed` and `pauses` (0–1) override the preset when a line needs it. For a
long script, generate one WAV per emotional section with its own delivery
and butt them together — a single monotone take is the most common reason
a faceless video sounds cheap. The WAV lands in the project, returned in
the job's status `output` → `media_import` it. If narration exists as audio/video,
`transcribe` it with an ISO language hint when known; the default `base.en`
model is English-only, so choose a multilingual model for non-English. Each
segment returns with its `words` — MEASURED `{text,start,end}` triples from
whisper's token timestamps; cache them, they are what `captions_animate`
takes. Then `analyze_silence` on the voiceover; `analyze_scenes` on
any footage that will be cut; `analyze_beats` when there is music (or on the
BGM after it is fetched). On a machine without the Python analysis extras,
`suggest_cuts {path}` gives a BPM estimate and beat-aligned `cutTimes` with
ffmpeg alone — use them as section boundaries in the plan. `media_probe`
anything unfamiliar before you cut it. **A cut on every beat is a
slideshow with a metronome:** keep a few cuts deliberately off the pulse,
and never place more cuts than the story needs.

**Phase 1b — Read what the voiceover feels (the edit's true driver).**
`emotion {action: "extract", transcript: <the transcribe output>, project}`
— or `lines`/`script` before any recording exists. Every line returns its
**emotion** (7 labels) and intensity, its **act** (hook → setup → tension →
turn → reveal → proof → cta), the hormone shorthand for the state it puts
the viewer in, the TTS `delivery` + `speed` that would read it (the measured
version of Phase 1's delivery choice), the **easing curve** as exact
concat-motion authoring JSON (**tense lines ease in, warm ones ease out, a
burst pops, anger snaps**), the **colour temperature** + grade hint, the SFX
register, the **BGM duck depth** in dB, pacing, and 2–3 word stock `queries`
(emotion register × act shot) — plus the whole piece's **arc** (shape,
score, warnings) checked against the brief's `emotionalArc`. **This read
drives everything after it:** the plan carries it per segment (Phase 3), the
placement gate enforces it on candidates (Phase 4), motion easing and the
mix duck by it (Phases 5/5b), the grade pushes with it (Phase 6). With
`FUNCUT_EMOTION_API_KEY` (or `HF_TOKEN`) set, the free hosted
`tabularisai/multilingual-emotion-classification` sharpens the read —
XLM-RoBERTa across 23 languages, its 11 labels folded onto the edit's 7
(75/25 blend with the lexicon, confidence 0.85); without a key the
English-only stem lexicon answers alone (confidence 0.55) — both are
first-class, the pipeline never requires the key, and a non-English script
without one comes back with an honest note instead of a silent neutral. When the arc's warnings name a flat or repetitive script,
fix the SCRIPT (or re-deliver the section) before planning — an edit cannot
rescue a narration that never changes state. A brand guide (channel
palette/type/grade/SFX/taboos/vibe — `emotion {action: "template"}` returns
one to copy, then store it as `brandKit` via `creative_brief`) is applied on
the same call: its taboos are checked against every planned query, and a
taboo hit is a blocker — replace the query, never the taboo.

**Phase 2 — Tighten the voice.** Feed the silence spans to `clip_cut_spans`
(remove dead air; `-35` dB threshold, ≥0.5 s spans). **The padding rule the
tools will not apply for you:** `analyze_silence` reports spans WITH a
`paddingMs` field that is *kept, not applied* — shrink each span by ~80 ms on
each side yourself before cutting, so natural breaths survive at sentence
ends. `transcribe` returns source-file-relative segments; if ripple cuts
happen after transcription, retime all later segment boundaries by the exact
removed spans before using them on the timeline. Word times are the transcript's
measured `words`, never estimates interpolated from segment times — timing
that is claimed but wrong is worse than timing that is honest.

**Phase 3 — Write the edit plan (ONE reasoning pass).** Emit the JSON plan
from `templates/edit_plan.schema.json` (example beside it). The plan declares
its **mode** (`reel` ≤ 60 s or `longform`), its **sections** by role
(hook → intro → body chapters → cta → outro), and every segment with: its
visual (B-roll query or a motion comp), its **cutType**, its **in/out**
entrance and exit, its `transitionIn` at section changes, its SFX with the
time its PEAK must land on, its caption style, and the **grade** (required —
a video with no grade is a rough cut, not a finished piece). Segments under
a voiceover also carry the Phase-1b read — **emotion, act, easing,
colourTemp, duckDb** — copied from the extract's lines (the schema documents
them, the example shows them); they are what Phases 4–6 answer to. Put the
Phase-0 brief in `plan.creativeBrief`. Call `plan_validate` on the completed JSON;
fix every reported error AND every `creativeBlockers` entry until BOTH
`valid: true` and `creativeReady: true`. A structurally valid plan that is
not creativeReady is a rejection, not a warning to wave through. Craft score
below 7 means rewrite the plan, not proceed. Then compare the plan's `aspect` with `timeline_read`; if they
differ, set the canvas with `project_set_video` BEFORE gathering assets.

**Phase 4 — Gather (parallel if your harness allows).** All `openvid_search`
calls, all `sounds_search` calls, the `bgm_search` — these are independent;
fan them out when you can, and where you cannot, one `openvid_search_batch`
carries the whole B-roll query list (two or three words per query, per
`prompts/broll_matching.md`) in a single round trip. Fetch what you picked
(`openvid_fetch`, `bgm_fetch`), then import (`openvid_import`, `bgm_import`).
If a search comes back thin, follow the fallback ladder in `prompts/broll_matching.md` — never
invent a clip id, never leave a gap. **Never choose B-roll from caption text
alone.** Call `stock_contact_sheet {query, rank: true, brief}` and actually
look at the returned image; use its `ranking`, reject dark/letterboxed/generic
tiles, and follow its `queries` suggestions when the candidates are weak.
Then run the **placement gate** on the survivors: `emotion {action: "gate",
segment: {emotion, act}, candidates, brand}` — a candidate failing 0–1 of
its three checks (emotion register, act shot, brand taboo) passes; **2+
failed checks REJECTS it; every candidate rejected means DROP THE BEAT** —
cut the segment and tighten around it, never pad with a bad match (a hard
floor, §3.E). The gate is lexical; the pixels were the contact sheet's job —
both, in that order. Use the extract's per-line `queries` (emotion register ×
act shot) as the openvid_search vocabulary so candidates arrive pre-matched.
Pass `aspect` to `stock_contact_sheet` and use each chosen tile's
`reframeHint.clipTransform` offsets directly with `clip_transform` after
`clip_frame {framing:'fill'}` — this is a computed crop, not a blind
centre-crop of a square clip into 9:16. **The variety law
applies at search time:** every segment's query names a NEW subject — write
the query list, then delete any query whose noun you have already used within
the last 30 seconds of plan.

**Phase 5 — Place (sequential, batched).** Cut to the plan: add B-roll clips
(`clips_add_sequence` or `clip_add`), trim so the video LEADS the narration
slightly (J-cut feel — but keep plan segments contiguous; the lead is inside
the segment, not between them), then per clip: `clip_frame {framing: "fill"}`
so the picture covers the canvas, entrance/exit motion (`clip_transform` +
`keyframe_set`, or `fadeIn`/`fadeOut` via `clip_update`) — **the easing
comes from the plan's per-segment `easing` field** (tense beats ease in,
warm beats ease out; the extract returned the exact authoring JSON) —
transitions between sections — **always `clip_update {patch: {transitionIn: {id, duration}}}` on
the incoming clip** (see §3.T, never `effect_layer_add`) — SFX
(`sounds_add` at the computed peak time, then its dB level with
`clip_audio_levels_db`), and BGM automation with `clip_audio_levels_db`
(absolute dB: 0 is unchanged, −60 is the floor; **automate to the plan's
per-segment `duckDb`** — the act's value from the emotion read: −15 under
the naked hook, −12 under standard speech, −9 letting the reveal ride, −10
behind the ask). Use `captions_add` for
segment-timed captions. Group each logical chunk into one `edit_apply` batch.

**Phase 5b — Motion comps (design). Every finished piece goes through here.**
This is what separates an edit from a video. The motion layer is six MCP
tools: `motion_presets` (the library), `motion_inspect` (nodes, slots —
never guess), `motion_frame` (look before you bake — it now returns an inline
image, so check legibility, contrast and safe margins with your eyes), `motion_bake`
(encode a piece in one call), `captions_animate` (word-timed kinetic
captions from measured word times), and `motion_reel` (a whole storyboard
as one reel — scenes with MULTI-LAYER stacks: plates under the comp and
grain, light leaks, vignettes and foreground cards over it, each on its own
drift and clock; check one still with `frame: seconds` before rendering). The floor is one designed beat per
section and at least one per 15 s of runtime; in finance/science/tech every
number, percentage, comparison or process is a `funcut.ticker.count`,
`funcut.chart.bars`, `funcut.stats.triptych` or `funcut.compare.split` —
never a plain text card. Edit every comp: real copy, retimed to its cut, one
element of the preset deleted. The loop is inspect → one validated `ops`
batch → three `motion_frame`s → `motion_bake` at the cut's size →
**`motion_overlay` for any transparent comp** (lower thirds, kinetic
captions: composite the RGBA bake over the real B-roll with `plate` + `at`,
so the overlay does not cover the shot) or `motion_bake {encode: true}` for a
full-frame piece → `media_import` → `clip_add`. An encoded caption reel is
opaque; never lay one over footage you still want to see. Full doctrine:
`prompts/motion_comps.md` and the `motion` skill.

**Phase 6 — Grade (required).** One consistent look per video. Either ONE
effect layer spanning the whole runtime — `effect_layer_add {start: 0,
duration: <runtime>, effectId, ...}` with the duration WRITTEN (the default
when absent is 5 seconds, which grades five seconds and stops) — or the same
`videoEffects` chain on every clip via `clip_update`. The intensity key is
the package's own (`concat.teal-orange` → `amount` 0–100; find each in
`effects_list`), never a guessed "intensity". The emotion read's per-segment
`colourTemp` tells you which direction each act pushes WITHIN the one look
(fear cold and crushed, joy golden and lifted, disgust sickly) — express it
as intensity variation of the same base recipe, never as a second filter.
Prove the grade with a `preview_frame` A/B at 25% and 75% of runtime.

**Phase 7 — Watch the whole cut (never skip).** Call `watch_render` on the
open project: ONE call returns a labelled contact sheet sampled across the
whole timeline, as an image, plus measured craft facts (black frames,
letterbox bars, flat/duplicate shots, static transitions, audio envelope).
Then `craft_critique {sheet, brief, project}` for six 0-10 scores — hook,
story, rhythm, shotChoice, design, sound — and a short list of concrete
revisions. **You must act on at least one revision and call `watch_render`
again** before exporting. A score below 7 is not ready. Run the checklist in
`prompts/review.md` too: no overlaps or gaps (`timeline_read`), captions
readable and inside the safe area, the grade is present at both check points,
every clip fits the canvas, SFX peaks hit their cuts, the BGM ducks under
speech. `preview_frame` is still there for a close-up on one suspicious
moment — keep it under 5 calls per minute of video.

**Phase 8 — Export, then watch the actual file.** `export_start` (crf 20,
h264 for platforms) → `export_wait` until the job says done → confirm the
output file exists in the status. Then call `watch_render {path: <the
exported file>, audio: true}` and `craft_critique` one final time; the
timeline preview and the encoded file can differ. Only when the exported
file's score is >= 7, with no black frames, bars or long static runs in the
contact sheet, may you call the work done. Say the honest score in your
report; never claim success on an unmeasured render. If a platform target was
given, match its aspect and duration requirements. A planned loudness target
is not proof of compliance: call `measure_loudness {path, targetLufs}`
(or `watch_render {path, audio:true}`) on the exported file. `export_wait`
reports no loudness. Short-form targets about −14 LUFS integrated with a
true peak under −1 dBTP; fix with `clip_audio_levels_db` and measure
again. Never claim LUFS compliance from the plan alone. And prove the SPEECH
itself survived the render: `transcribe` the exported file, then
`emotion {action: "verify", expected: <the edit's word timings>, actual:
<the export's words>}` — matchRate ≥ 0.9, worst drift within toleranceMs
(default 150), no duplicated words is the pass; a failure names clipped,
shifted or duplicated audio the eye cannot see (check clip ends, trims and
ripple cuts against the retime rule in Phase 2).

## 2. Job and artifact hygiene

- Analysis results come back as JSON in the wait reply's `output` field
  (scenes: `{start,end}` seconds; beats: `{bpm, beatsMs[], onsetsMs[]}`;
  silence: `{spans[{start,end}], paddingMs}`; transcription:
  `{segments[{start,end,text,words[{text,start,end}]}]}` — the `words` are
  measured, and are the only word timing you may use). Cache them in your
  working notes; do not re-analyze the same file twice.
- **Keep the journal** (`prompts/memory.md`): after every `edit_apply` batch
  append one line to `memory.md` in the project — what entered or left,
  where, with what SFX/comp/grade. Before every placement phase, re-read the
  journal's first/middle/last entries and `timeline_read`. The journal is
  how you know what the first, middle and last thirds of this edit contain
  without guessing, and it ships with the deliverable.
- Never fabricate ids: clip ids come from the tools that made them, library
  ids from search results. A missing result is a fact — use the fallback
  ladder, or say so.

## 3. The hard floors (these override every playbook)

The numbers you are held to. `plan_validate` enforces what it can; you are
held to the rest at review.

**E — Emotion is the driver, not a decoration.** Under a voiceover, every
segment carries its measured `emotion`/`act` (Phase 1b), and every layer
answers to it: motion easing (tense eases in, warm eases out, a burst pops),
colour temperature, SFX register, duck depth, stock vocabulary. The
placement gate is absolute: a stock candidate failing 2+ of its three checks
(emotion register, act shot, brand taboo) is REJECTED; when every candidate
for a beat is rejected, DROP THE BEAT — tighten around it, never pad with a
bad match. A brand taboo in any planned query or caption is a blocker.
Without the hosted key the lexicon read still runs — the floor applies at
either confidence.

**T — Transitions are clip patches.** `clip_update {patch: {transitionIn:
{id, duration}}}` on the clip that begins the new section, NEVER
`effect_layer_add` (a layer cannot carry a two-input transition; it renders
nothing). The incoming clip must sit adjacent to the previous clip ON THE
SAME LANE within half a frame — a transition on a moved or gapped cut is
silently orphaned. Density: a named transition at every section change, and
at least one per 15 s in energetic longform; a plain cut wins everywhere
else. Pair every transition with its SFX.

**P — Pacing.** cuts/minute: calm 6–10 (documentary, history) · standard
10–16 (explainer, how-to) · energetic 16–24 (tech, finance, motivation) ·
hype 24+ (gaming, extreme). No visual segment runs past **4.0 s** in
energetic niches or **6.0 s** in calm ones — the segment count your plan
implies is ⌈runtime / maxSeconds⌉ at minimum. Short-form retention: a
pattern interrupt (cut, text pop, SFX, zoom) every 2–4 s; the hook lands
inside 3 s. Not only hard cuts: the cut-type catalog (`prompts/
cuts_and_pacing.md`) has twelve — match cuts, smash cuts, whips, montage,
cut-on-action, cut-on-beat — and a piece that only hard-cuts is a slideshow.

**V — Variety.** A clip id appears at most ONCE in a reel, and at most once
per 60 s in longform, never twice in one section. Every segment's B-roll
query names a new subject (no noun repeated within 30 s). Shot scale climbs
within a section: wide → medium → close → detail. Captions and comps never
repeat the same entrance twice in a row.

**M — Motion on every element.** Every clip enters and leaves on purpose:
an entrance (pop 120–180 ms, slide 200 ms, punch zoom 150 ms, dissolve) and
an exit or a hand-off to the next. "Raw in, raw out" is a defect, not a
style. Keyframes via `keyframe_set`; fades via `clip_update`'s
`fadeIn`/`fadeOut`; the grammar is `prompts/keyframes_motion.md`.

**G — The grade is not optional.** One filter recipe per video, applied to
the WHOLE runtime (see Phase 6 — the duration field is the trap), intensity
set once, proven at 25% and 75% of runtime.

**F — Fit the frame.** The canvas is set before anything is placed
(`project_set_video`). OpenVid has NO vertical footage — never search
`orientation: "vertical"`; for 9:16 search the landscape shelves (prefer
720p/1080p sources), then `clip_frame {framing: "fill"}` every clip so the
picture covers the canvas and crops the overhang. A letterboxed or
pillarboxed frame in any verification frame is a blocking defect.

**S — Sound variety.** Never the same sound id twice within 30 s; rotate at
least three ids per role; alternate soft and hard hits. Map motion to the
library's own semantics: counters → `data-countup-*`, chart builds →
`data-bar-grow`/`data-chart-reveal`, money moments → `money-*`, zooms →
`whoosh-*`, reveals → `impact-*`. One design sound per cut in energetic
pacing, peak ON the cut.

**Z — One text system.** Four zones, one occupant each at any moment:
captions in the lower band (~70–80% height, centered), titles the upper
third, lower thirds anchored left at the lower third, number cards center.
The palette is fixed in the plan (background / ink / accent / plate); text
contrast is checked against the actual frame at the busiest moment.

**R — Memory.** The journal (`prompts/memory.md`) is kept after every batch
and re-read before every placement phase. At review you can say what the
first, middle and last thirds contain — from the journal, not from memory of
ten minutes ago.

**C — Captions.** Segment-timed captions stay honest (`captions_add`); word
timed kinetic captions need MEASURED word times (`captions_animate` —
whisper segments with per-word estimates are not timing). Captions: 2–4
words a cue where the segment permits, bold (600+), stroke or plate
contrast, no text touching frame edges, `offsetY` ≈ 0.35 default.

**L — Loudness and music.** BGM rests at −4 to −6 dB, ducks to an absolute
−12 dB under speech (−9 tight, −15 relaxed), rides up in gaps, section
changes land on downbeats when `analyze_beats` gives a grid, and the piece
ends on a downbeat.

**N — MAGICK stickers are CC BY-NC: never in monetized/commercial videos.**
OpenMoji (CC-BY) is the safe shelf.

## 4. Hard rules

1. Never claim "done" before `export_wait` reports the file written, the
   Phase-7 watch/critique loop has produced a score >= 7 after the LAST edit,
   and the Phase-8 check on the exported file passed.
2. Never leave a segment without a visual; never leave the timeline with
   overlapping clips or gaps you did not plan.
3. Never invent library ids, paths, or tool results. Missing = say so + fall
   back.
4. `preview_frame` ≤ 5 calls per minute of finished video; use
   `watch_render` for the whole cut instead of many single frames.
5. `creativeReady: false` from `plan_validate` is a blocker. `craft_critique`
   below 7 is a blocker. Fix both; do not declare success on compliance alone.
6. One video, one look: no filter of the week mid-video.
7. When the harness offers parallelism, searches are the parallel phase;
   placements stay sequential (they depend on searches).
8. If a tool errors with "not downloaded" (a model) or a pip hint (an
   analysis dependency), surface the exact remedy to the user instead of
   retrying in a loop — and know that `models_download` is the headless
   remedy for the first.
9. Long jobs: start, then wait with `timeoutSeconds` you can afford; call
   wait again rather than assuming.

## 5. Playbooks

`prompts/` holds the depth. Read what the phase needs:

| File | Reach for it when |
| --- | --- |
| `prompts/script_analysis.md` | turning a topic into a script (hooks, beat sheets, reel vs longform, CTA frameworks) |
| `prompts/edit_plan.md` | writing the plan JSON (with a worked 30 s example) |
| `prompts/broll_matching.md` | choosing B-roll queries, scoring results, the fallback ladder, aspect fit |
| `prompts/cuts_and_pacing.md` | the twelve cut types, J/L feel, pacing curves by mode |
| `prompts/transitions_guide.md` | the 122 transitions (classic 22 + the 100 viral pack), the clip-patch mechanics, when a plain cut wins |
| `prompts/sfx_design.md` | sound design: categories, semantic mapping, sync peaks, anti-repetition |
| `prompts/motion_comps.md` | designing graphics: which comp, worked plans, density floors |
| `prompts/music_and_duck.md` | BGM choice, beat alignment, ducking keyframes, longform structure |
| `prompts/text_animation.md` | titles, captions, kinetic text, zones, color methodology |
| `prompts/color_grading.md` | the filter recipes per niche, and how a grade is actually applied |
| `prompts/keyframes_motion.md` | motion grammar: Ken Burns, parallax, punch zooms, entrances/exits |
| `prompts/niche_playbooks.md` | the whole look for a niche in one place (10 niches, reel + longform) |
| `prompts/reels_vs_longform.md` | the two modes' structures, pacing curves, CTA placement |
| `prompts/memory.md` | the editing journal: what to write, when to re-read |
| `prompts/review.md` | the Phase-7 checklist and the definition of done |
| `prompts/creative_direction.md` | Phase 0: turning a topic into a sharp angle, arc and payoff |
| `prompts/quality_loop.md` | watch_render -> craft_critique -> revise, and how to read the scores |
| `prompts/tool_reference.md` | every tool's arguments, units, and gotchas |

## 6. The maestro shelf (judgment, when a phase wants taste)

The `maestro` skill sits beside this one at `.claude/skills/maestro/`. This
skill is the hands — the pipeline, the tools, the floors; maestro is a
distilled library of design, motion and film judgment (28 reference modules
over ten vendored corpora: a 157-card product-shot vocabulary, engine-agnostic
motion principles, a sound-design and beat-sync method, art-direction
catalogs). Read it when a phase wants taste, not tool mechanics — and read
only the module the phase needs, never the whole shelf. Its Grill Gate does
not run here: the Phase-0 `creative_brief` is the one locked brief; use
maestro's interview questions to fill it, never to open a second one.

| Phase | Reach into `.claude/skills/maestro/references/` for |
| --- | --- |
| 0 direction | `design-direction.md` (style registers, art direction), `process.md` (brief-locking discipline) |
| 3 plan | `video-direction.md` (story structure, beat planning, pacing), `video-shotcraft.md` (the 157-shot vocabulary for product/demo work) |
| 5b comps | `motion-principles.md` (timing, easing, choreography), `design-foundations.md` (typography, color, hierarchy) |
| 6 grade | `design-foundations.md` (color systems) |
| 7 critique | `design-audit.md` (systematic critique, anti-slop gates) — feed its findings into the mandatory revision |
| SFX/BGM | `video-sound.md` (SFX pinning, anti-machine-gun spacing, beat-sync that verifies the render) |

When guidance conflicts, the hard floors in §3 win — maestro refines how a
floor is met, never lowers it. When a piece needs something the editor's own
motion layer cannot express, maestro's engine modules (`video-remotion.md`,
`video-hyperframes.md`, `gsap.md`, `threejs.md`) can build it as code, and
the rendered result imports as footage. The full map, plus the update and
drift-check procedure: `docs/MAESTRO.md`.

## 7. The clip-skills shelf (editing craft, when a phase wants the cutter's judgment)

The `clip-skills` skill sits beside this one at `.claude/skills/clip-skills/`.
It is the editing-craft core: 15 software-agnostic knowledge modules that
systematize professional post-production decisions — the audio-driven
pipeline, breath/gap handling, A/B-roll matching, beat-sync, pacing theory,
color-grading workflow, keyframe discipline, speed ramps, keying/masking,
tracking, multicam, subtitles, transitions, voice processing, export
parameters. Its modules are written in Chinese; read the one the phase
needs (the file names carry English topic hints in `NOTICE.md`), never the
whole shelf. Its execution guide `references/editor-funcut.md` maps every
module to this editor's tools — the table below is the phase-level view.

**Maestro judges what looks good; clip-skills judges how to cut.** The two
shelves do not overlap: reach for maestro on typography, color systems,
motion temperament, art direction, critique; reach for clip-skills on where
to cut, what to cover with, how to shape the audio spine, how deep to duck,
what a transition means, which export numbers to pick. A phase that wants
both (a comp over a beat-matched cut) uses both.

| Phase | Reach into `.claude/skills/clip-skills/references/` for |
| --- | --- |
| 0 direction | `01-视频分类与剪辑逻辑.md` (type the video, pick the edit skeleton), `05-节奏与结构.md` (hook three-step, duration strategy, wave pacing) |
| 1 understand | `02-音频驱动剪辑流程.md` (the audio-first spine, breath/gap typology, the three gap rules behind the Phase-2 padding rule) |
| 2 tighten | `02` §三 (气口三规则: keep/expand/crossfade-pad — the judgment behind the 80 ms shrink), `15-人声与对白处理.md` (cleanup chain order, de-noise restraint) |
| 3 plan | `01`/`05` (section skeleton, 时钟理论 wave points as a thinking tool — labeled heuristic), `14-转场决策.md` (transition semantics for the plan's `transitionIn` fields — the "earned, not decorated" rule) |
| 4 gather | `03-素材AB分类与替换.md` (A/B classification to seed queries, the five B-roll rules: cover the sentence, group shots, no repeats, mute the track) |
| 5 place | `04-声音与音乐衔接.md` (J/L-cut feel, duck depths, leaving breath between peaks), `14` (transition choice + duration numbers), `15` (duck dB by context) |
| 5b comps | `13-字幕与文字设计.md` (caption readability six rules, the four-zone text system, word-time honesty) — with the `motion` skill for execution |
| 6 grade | `06-调色.md` (Log/LUT order, correct-then-style, scope-quantified deltas — map them to `effect_key_set` values) |
| 7 watch | `05` (pacing diagnosis: information density vs shot length, the fix list for a sagging middle) |
| 8 export | `12-导出与交付.md` (resolution/fps follow the source, bit-rate "recommended not max", platform framing table, naming), `15` §六 (LUFS targets per platform — what `measure_loudness` should read) |

Two integration rules, mirroring maestro's:

1. **clip-skills' 自主执行 (autonomous-execute) mode does not run inside
   FUNCUT** — this pipeline is the executor. Inside this repo, clip-skills
   is read as knowledge; its three-mode framing (answer / ask / execute) and
   its clarification checklist are for filling THIS pipeline's brief and
   plan, never a second interrogation. Outside this repo (a user asking
   editing questions in any harness), clip-skills runs its own modes.
2. When its guidance conflicts with the hard floors in §3, **the floors
   win** — clip-skills refines how a floor is met (e.g. its cut-rate
   heuristics tune the P floor's ranges by niche), never lowers one. Where
   both shelves speak to the same phase, maestro owns aesthetics,
   clip-skills owns craft, and the plan JSON reconciles them.

The vendoring, pin and update procedure: `docs/CLIPSKILLS.md`.
