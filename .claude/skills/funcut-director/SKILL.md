---
name: funcut-director
description: >-
  Direct professional faceless video edits inside FUNCUT (the Concat video editor) through its
  MCP tools. Use this whenever the user asks to make, edit, cut, caption, score, color-grade,
  animate, or export a video — including a whole video from just a script or topic — or whenever
  a Forge run says to act as the video director. Covers the full pipeline: script analysis, TTS
  voiceover, transcription, silence removal, scene and beat analysis, B-roll from OpenVid planned
  segment by segment (a description-ranked search, a continuity gate, and a revision loop that
  re-shops only what the critic named), background music, SFX placement, titles, segment-timed
  captions, word-timed kinetic captions, motion comps, keyframe motion, color grading,
  transitions, self-verification with watch_render and craft_critique, and a timeline-ready handoff (export only if asked). Professional
  rules only: hard floors on pacing and variety, never leave a visual gap, SFX peaks land on
  cuts, section changes land on downbeats, verify by thirds before claiming done.
---

# FUNCUT Director

You are a professional video editor directing an edit inside FUNCUT. You
do not guess — you measure (probe, analyze, transcribe), decide (one
written plan, grounded in a creative brief), execute (batched, undoable),
and verify (LOOK at the whole cut, score it, revise, then hand off a customizable timeline; export only if asked). The plan below is the default; the deeper playbooks live
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
  `analysis_wait`/`export_wait` (`timeoutSeconds`, default 60, max 590 — call
  again if it is still running). Status of any job: `export_status`.
- A model that is not downloaded is one `models_list` + `models_download`
  away — headless, no window needed. Never retry a "not downloaded" error in
  a loop.
- You can SEE. `preview_frame` is one frame; `watch_render` is the WHOLE cut
  as one labelled contact sheet (one call, one image, no token explosion);
  `stock_contact_sheet` shows B-roll candidates before you choose them;
  `motion_frame` now returns an inline image, not just a path. A file PATH to
  a PNG is not a picture you can see. Only inline `type: image` blocks are.
- The default Director surface is the 108-tool creative surface. The
  composition reads `scene_analysis`, `scene_timeline`, `motion_preview` and
  `geneva_explain` are opt-in (the farm pair and the two destructive tools are
  off by design); if one is not served, use the served equivalent
  (`analyze_scenes`, `motion_frame`, `motion_reel`, the `motion` skill) and say
  so. A specialist run may serve the whole catalogue with
  `CONCAT_MCP_ALLOW=all`; never invent a tool that is not on the surface.
- `prompts/power_moves.md` is the 100-move checklist across the 25 phases:
  thesis, hook, pacing, footage, motion, captions, sound, grade and timeline
  acceptance. Use the moves the story earns; record the ones you leave out.
- `prompts/signal_matrix.md` names the decision every `voice_signals`
  reading informs (14 families, 70 named readings). Spend the readings, not just
  the family names; a reading that is absent is named unmeasured.
- `prompts/signal_recipes.md` turns those readings into a cut score, a cut
  density, a cut type and a confidence fallback. Read it at the plan phase.
- `prompts/emotion_to_edit.md` is the seven-label recipe: pace, cut types,
  captions, sound, grade, motion and duck for each emotion and intensity.
- `prompts/token_budget.md` is the 10-minute budget and the reduction rules
  that protect the hook, the turn, the contact sheet and the whole-cut watch.
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

## 0a. Multi-agent fan-out — when the harness exposes it

Forge can expose `spawn_agents` and `/workflow` (`agent()`, `parallel()`,
`pipeline()`). Use them when they are available; the run is faster and the
judgment is better when independent work is done independently. The rules:

- **One writer.** Only the Director thread calls `edit_apply`, `edit_undo`,
  `project_save`, or any other mutating tool. Subagents are read-only
  researchers, designers and critics; they return JSON, not edits. Two
  writers on one timeline interleave and lose each other's work.
- **Fan out the independent reads.** One agent per section or stock slot for
  `stock_search` / `stock_fetch` / `stock_contact_sheet` / `footage_continuity`;
  one per motion beat for `motion_inspect` / `motion_frame` / comp authoring;
  one for sound design; one for the script/story map. Ask each for a small
  artifact with ids, paths, reasons and confidence — never a chat essay.
- **Keep the dependent work in the main thread.** The plan depends on the
  story map; placement depends on the chosen footage; the grade depends on
  the placed cut; the final acceptance depends on the saved timeline. Do not
  fan out a dependency.
- **Independent critic.** When subagents exist, give the rendered contact
  sheet, the brief and the plan to a fresh-context critic that has not seen
  the build conversation. It returns findings only; the Director decides
  and executes the revision. If no subagent exists, run the same critique
  inline with `craft_critique` and say so in the report.
- **Fallback.** Without `spawn_agents`/`/workflow`, issue independent tool
  calls in parallel where the harness allows it, and keep every write
  sequential. Never pretend a fan-out happened.

## 1. The pipeline (in order — do not skip phases)

**Phase 1 — Creative direction (do this FIRST, before any tool that cuts).**
An edit with no thesis is an assembly, not a video. Call `creative_brief`
with `{action: "set", path: <the project folder>, brief: {audience, platform,
goal, thesis, curiosityGap, payoff, emotionalArc, referenceStyle}}` - `path`
is required, the fields live NESTED in the `brief` object (a top-level
`audience` is an unknown-field refusal), and emotionalArc needs at least
three DIFFERENT beats. Read `directorPrompt` from the reply and keep it in your
working context — it is the intent every later decision is judged against.
If the user gave a topic but no angle, invent a sharp one and state it.
Before choosing one direction, run a three-concept bake-off: write three
different films that could answer the brief (different subject, structure,
visual language and sound), say in one sentence what each would make the
viewer feel, and pick one for a stated reason. A single first idea is how
every AI edit becomes the same edit. Then write the **style bible**
(`prompts/style_bible.md`) and the **retention contract** (`retention` in
the plan: hook, turnAt, payoffAt, patternBreaks, openLoops). Both are
`creativeReady` blockers when absent, because a cut with no one look and no
reason to keep watching is not a finished edit.
Read `prompts/creative_visual_direction.md` before planning visuals. It is
a decision framework, not a template: invent a film-specific visual language
and choose footage, original motion, data graphics, typography, generated
media, holds, or hybrids by each beat's communication job. Do not impose
fixed media ratios or repeat a preset treatment across videos. Record a
concise rationale for major choices and execute/verify every chosen visual;
a rendered motion asset not imported and placed is not complete.
Copy the brief into the plan's `creativeBrief` field. Optional: `local_vlm`
with `action: "status"` (and `setup` once on a low-end machine) so the later
critic can actually see.

**Phase 2 — Understand the material.** If there is narration to make:
`tts_generate` with a `delivery` matched to the brief's `emotionalArc` —
`urgent` for the hook, `calm`/`awed` for the reveal, `authoritative` or
`conversational` for the CTA. Delivery sets BOTH speaking rate and the
breath between sentences, so the read has an arc instead of one flat pace;
`speed` and `pauses` (0–1) override the preset when a line needs it. For a
long script, generate one WAV per emotional section with its own delivery
and butt them together — a single monotone take is the most common reason
a faceless video sounds cheap. The WAV lands in the project, returned in
the job's status `output` → `media_import` it.
**Repair a rough take before you analyze it.** A recorded or imported
voiceover is graded before anything else reads it:
`sense_voice_quality {action: "classify_recording_quality", audio}`. A
verdict of `home` or `rough` (a high noise floor, SNR under ~12 dB, hum,
clipping) is repaired FIRST — `sense_audio_repair {action:
"enhance_speech", audio, out: <an enhanced copy beside the take>}`,
then `sense_audio_repair {action: "remove_hiss_hum_rumble", audio:
<the enhanced copy>, out: <the take itself>, overwrite: true,
force: true}` (its `findings` name the chain; `targetLufs` picks the
platform landing). The order is enhance-then-clean, and the chain lands
**in place**: the second call writes the cleaned, enhanced audio onto
the take the timeline plays, so the timeline, the transcription and
every later analysis (`analyze_silence`, `voice_signals`, beats) read
the repaired take, never the raw one — do NOT import or re-place the
narration clips to point at a new file. An un-repaired rough take puts
its noise floor into every downstream signal; the enhance chain filters
and never cuts, so timings survive the repair. A TTS take is **not**
clean by construction: the server's default voice model is the int8
Kokoro, and its quantisation adds a hiss above the noise floor — the
same grading and repair chain runs on a synthesized take (a take from
the project's own `audio/` folder is synthesized; the runner records
the chain it ran), and only a plan that names a full-precision model
may skip it.
If narration exists as audio/video,
`transcribe` it with an ISO language hint when known; the default `base` is
multilingual and detects the language, and an English-only `.en` model is never
used for a non-English take. Each segment returns with its `words` — MEASURED
`{text,start,end,confidence?}` entries from whisper's token timestamps; cache
them, they are what `captions_animate` takes. The reply's `languageDetected`,
`wordConfidence`, `uncertainWords`, `coverage` and `glossaryMisses` say how far
the reading can be trusted. Then `analyze_silence` on the voiceover; `analyze_scenes` on
any footage that will be cut; `analyze_beats` when there is music (or on the
BGM after it is fetched). On a machine without the Python analysis extras,
`suggest_cuts {path}` gives a BPM estimate and beat-aligned `cutTimes` with
ffmpeg alone — use them as section boundaries in the plan. `media_probe`
anything unfamiliar before you cut it. **A cut on every beat is a
slideshow with a metronome:** keep a few cuts deliberately off the pulse,
and never place more cuts than the story needs.

**Phase 3 — Read what the voiceover feels (the edit's true driver).**
`emotion {action: "extract", transcript: <the transcribe output>, project}`
— or `lines`/`script` before any recording exists. Every line returns its
**emotion** (7 labels) and intensity, its **act** (hook → setup → tension →
turn → reveal → proof → cta), the hormone shorthand for the state it puts
the viewer in, the TTS `delivery` + `speed` that would read it (the measured
version of Phase 2's delivery choice), the **easing curve** as exact
concat-motion authoring JSON (**tense lines ease in, warm ones ease out, a
burst pops, anger snaps**), the **colour temperature** + grade hint, the SFX
register, the **BGM duck depth** in dB, pacing, and 2–3 word stock `queries`
(emotion register × act shot) — plus the whole piece's **arc** (shape,
score, warnings) checked against the brief's `emotionalArc`. **Spend it with `prompts/emotion_to_edit.md`:** each of the seven labels
sets pace, cut types, captions, sound, grade, motion and duck, and intensity
scales the recipe. **This read
drives everything after it:** the plan carries it per segment (Phase 9), the
placement gate enforces it on candidates (Phase 13), motion easing and the
mix duck by it (Phases 15/16), the grade pushes with it (Phase 19). With
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

**Phase 4 — Read what the voiceover IS (the acoustic score).**
`voice_signals {audio: <the voiceover>, transcript: <the transcribe output>,
script: <the author's text when a script exists>, emotion: <the Phase 3
extract>, language: <the hint used>}` — one call,
the whole reading. The script, when there is one, is the wording's
authority: the call aligns it against the transcript (misheard words
corrected to the author's spelling, measured timings kept, a
`scriptAlignment` verdict in the reply), so captions and queries quote
the script, never the recogniser's guesses. Every line returns its **momentary loudness relative to
the take** (a line 3 LU over the mean is a punch line; 3 LU under is a
confession), its **speech rate** (words-per-minute and syllables-per-second
with a pace class: punchy / conversational / editorial / slow), its
**sentence type** (? ! . or fragment — cross-checked against the pitch's
terminal rise, so a statement that ends rising is marked), its **emphasis
words** (where the energy peaks land — the SFX peaks and the caption
highlights belong there), its **fillers** (um/uh are safe removals; like /
you know / basically are context calls), per-word **confidence** (gated on
measured timing, silence overlap and energy — or WhisperX scores when
installed), its **speaker**, its formants and voice quality, and the
**entities** it names: `$10,450` → a stat card, a name → a lower-third, a
date → a timeline, PII → redact. Around the lines: the **pauses
CLASSIFIED** (breath / beat / dramatic hold / scene turn / dead air), the
**motifs** (a repeated phrase gets its visual back), the take's **rhythm
layers** and phrase boundaries, the **noise floor** (the BGM decision), the
**diarization** with change times, and `directorHints` — the cut style
(fast / mixed / hold / building), the caption gate, the protected pauses,
the font and music-genre hints for the detected script (Bengali gets a
Bengali font). The reading also carries **`signalConfidence`** — one
confidence and basis per signal family (formants on a phone-band take,
reverb ±20% on a single channel, emphasis reliable): lean less on a signal
whose confidence is low, and never treat an absent ear's silence as a
neutral reading — and **`decisionPolicy`** — the table that says which
signals drive each decision (cut: pause class, emphasis, pace; caption:
emphasis, word confidence, script; graphic: entities, sentence type; BGM:
noise floor, music segments; transition: speaker change, topic,
discourse). The Rust ears (prosody, forensics, speakers, NER) need the
`voice-signals` binary built (`cargo build -p voice-signals`); without it
the Python signals still answer and the reply says which ear was absent —
never guess the missing ones. **Spend every reading, not just the family:**
`prompts/signal_matrix.md` maps all 14 families and 70 named readings
to the decision each one informs. Every family and every reading you spend must
appear in the plan's `signalLog` with its confidence and one-sentence basis;
a reading that was absent is named unmeasured, never cited at confidence 0.
**Turn the reading into cuts with `prompts/signal_recipes.md`:** score the
seam, set the density, choose the cut type, and use the conservative fallback
when confidence is low. The reading is not a dashboard; it is the cut.
**This reading is what the edit cuts by:**
Phase 7 cuts only the dead air and never a dramatic hold, Phase 9's pacing
takes the cut style, Phase 15's SFX peaks land on the emphasis words and
the BGM obeys the noise-floor decision, the captions drop or re-check the
gated words, and a speaker change is staged like a scene change. The
deeper consumption readings — script quality, restarts, fillers, rhythm
and meter, music structure, caption alignment, what a sound file IS and
what it MEANS — live behind the seven family doors (`sense_discourse`,
`sense_voice_quality`, `sense_prosody_style`, `sense_audio_repair`,
`sense_captions`, `sense_sound`, `sense_sound_model`), each `action`
naming one reading. The two sound doors are Phase 18's instruments:
`sense_sound` measures the file (its true `peakAt`, attack, the six
role fits) and proves a placed accent with `check_placement`;
`sense_sound_model` adds what it MEANS (YAMNet labels, CLAP similarity)
and `plan`s the whole accent design from the edit's own beats, arc,
visual events and bed structure. Both degrade loudly to
measurement-only answers when the models are absent — never silently. Three of them are wired into the pipeline,
not left to judgment: `sense_voice_quality {action:
"classify_recording_quality"}` + `sense_audio_repair {action:
"enhance_speech"}` on a rough take BEFORE it is analyzed (Phase 2),
`sense_audio_repair {action: "detect_music_segments"}` before any BGM is
chosen (Phase 13), and `sense_audio_repair {action: "identify_music"}` as
the copyright check on a take that already carries music (Phase 13). The
rest are called where the edit needs them, each as its door's `action`
(`sense_discourse {action: "score_script_quality"}` before the read,
`sense_voice_quality {action: "detect_restarts"}` + `sense_voice_quality
{action: "classify_filler_type"}` for the punch-up, `sense_audio_repair
{action: "analyze_music_structure"}` when cutting to music,
`sense_captions {action: "align_captions"}` + `sense_captions {action:
"clean_subtitles"}` before burning captions). A bare reading name is
never a tool - always the door with its action.

**Phase 5 — Story map and shot budget.** Before the plan, write the
film's argument as a map the plan can execute: the one-sentence thesis, the
hook's curiosity gap, the sections and their jobs, every claim and the
evidence that proves it, the turn, the payoff, and the CTA. Give each
section a **shot budget** (how many visual beats it earns) and a **sound
job** (bed, accent, silence, or none). Read `analyze_scenes` for real shot
boundaries and `suggest_cuts` for cut candidates; hold the map against
`footage_plan_suggest` so a planned B-roll slot answers a real dialogue gap
and a real gap is not left uncovered. The map is an artifact, not a mood
board: if a claim has no evidence, cut the claim or find the evidence before
you write the plan. No edit tool is called in this phase.

Run the **retention pass** on the map before the plan: name the long
low-energy stretches (where a viewer would leave), the explanations the
script repeats (the same idea told twice without a new image) and the cuts
that keep the same act, mood and angle (weak transitions). Put them in
`retention.analysis` with their times, and move a pattern break, a cut or a
new visual to each finding. Then run the **variety pass**: no B-roll style,
camera movement, caption animation or transition repeats twice running;
rotate shot scale, camera move, B-roll category and caption entrance, and
record what the cut actually uses in `visualVariety`. A repeat is only
allowed when the motif is the point, and the plan says why. Match the cuts,
motion and sound to meaningful speech beats (emphasis peaks, sentence ends,
topic boundaries, music downbeats), not to arbitrary seconds; name the beat
each decision spent in `signalLog`.

**Phase 6 — Scene, beat and transition map.** Turn the story map into a
technical grid: `analyze_scenes` for real shot boundaries, `suggest_cuts`
for voice/beat candidates, `analyze_silence` for the pauses that can carry a
turn, and `emotion`/`analyze_beats` for the beat and energy curve. Mark
where a cut, a transition, a reframe, a mask or a held shot belongs, and
name the reason for each. A transition is a change of place, time or
argument, not decoration. `scene_timeline` and `scene_analysis` are opt-in
reads; if they are not served, use the served scene and cut tools and say
so. No edit tool is called in this phase.

**Phase 7 — Tighten the voice.** Feed the silence spans to `clip_cut_spans`
(remove dead air; `-35` dB threshold, ≥0.5 s spans) — but the Phase 4
reading already classified them: cut ONLY the `dead_air` pauses, keep every
`breath`, `beat` and `dramatic` hold (a hold is the performance — removing
it is the classic auto-edit failure), and treat each `scene_turn` as a
placement cue, not a cut candidate. **The padding rule the
tools will not apply for you:** `analyze_silence` reports spans WITH a
`paddingMs` field that is *kept, not applied* — shrink each span by ~80 ms on
each side yourself before cutting, so natural breaths survive at sentence
ends. `transcribe` returns source-file-relative segments; if ripple cuts
happen after transcription, retime all later segment boundaries by the exact
removed spans before using them on the timeline. Word times are the transcript's
measured `words`, never estimates interpolated from segment times — timing
that is claimed but wrong is worse than timing that is honest.

**Phase 8 — Voice edit proof.** Read the timeline back with
`timeline_read` after the tighten. The runtime must equal the original
runtime minus exactly the spans you removed; every later transcript time
must be shifted by the same amount; no word may be clipped, duplicated, or
moved onto a neighbour. If the reading disagrees, `edit_undo` the tighten
and redo it before planning — a plan built on a timeline that does not match
its transcript is a plan built on a lie. Keep the before/after runtime in
the run report.

**Phase 9 — Write the edit plan (ONE reasoning pass).** Read
`prompts/token_budget.md` first: a 10-minute cut is budgeted at 250–500k
tokens (800k hard stop), and the plan is the one source of truth that keeps
the run from re-reading its own tool output. Emit the JSON plan
from `templates/edit_plan.schema.json` (example beside it). The plan declares
its **mode** (`reel` ≤ 60 s or `longform`), its **sections** by role
(hook → intro → body chapters → cta → outro), and every segment with: its
visual (B-roll query or a motion comp), its **cutType**, its **in/out**
entrance and exit, its `transitionIn` at section changes, its SFX with the
time its PEAK must land on, its caption style, and the **grade** (required —
a video with no grade is a rough cut, not a finished piece). Consult
clip-skills' audio-driven cutting module and maestro's shot cards for
scale, motion and register - and NAME the module each decision used in
the plan's signalLog notes (the craft warning asks for exactly that).
The cut lands on the emphasis
word and the beat, not the line boundary. Segments under
a voiceover also carry the Phase 3 read — **emotion, act, easing,
colourTemp, duckDb** — copied from the extract's lines (the schema documents
them, the example shows them); they are what Phases 13–19 answer to. Consult
maestro's shot cards for scale, motion, and register. Pick the card that
matches the beat's emotion and act. Put the
Phase 1 brief in `plan.creativeBrief`, the **style bible** in
`plan.style` (palette, grade, caption system, motion hand, sound world,
footage treatment, continuity rules) and the **retention contract** in
`plan.retention` (hook, turnAt, payoffAt, patternBreaks, openLoops). **The plan also carries the
`signalLog` — the decision log**: one entry per cut, caption highlight,
graphic, BGM choice and transition, naming the Phase 4 signals that drove
it (`{"at": 4.2, "decision": "cut", "signals": ["pause.class=dead_air",
"emphasisWords"], "confidence": 0.8, "note": "2.1s of dead air before the emphasis word"}`).
Every entry carries the confidence the profile rated its signals at and a
one-sentence basis; an entry with no note, no confidence, or confidence 0
is a claim the validator refuses, and a `cut` entry must land on a real
segment boundary or it did not drive a cut.
The profile's `decisionPolicy` says which signals each decision reads; a
plan with a voiceover and no signalLog is NOT creativeReady — the reading
exists to be consumed, not admired. Before validation, persist the exact canonical plan object (not a wrapper) as `<project>/edit-plan.json`; the acceptance check re-reads and validates this artifact after the save, so a plan that exists only in chat is not verifiable. Then call `plan_validate` on the completed JSON;
fix every reported error AND every `creativeBlockers` entry until BOTH
`valid: true` and `creativeReady: true` — including `signalLog.driven:
true`, whose `problems` name any entry that cites a signal the profile does
not answer. A structurally valid plan that is
not creativeReady is a rejection, not a warning to wave through. Craft score
below 7 means rewrite the plan, not proceed. Then compare the plan's `aspect` with `timeline_read`; if they
differ, set the canvas with `project_set_video` BEFORE gathering assets.

**The canonical plan is segment-based.** `segments[]` is the timeline
contract the validator and `scripts/auto_director.py` execute; for every
timed segment, the `visual` object says what actually plays. A `broll`
visual carries `query`, a 40+ character semantic `description`, optional
fallback `queries`, `hero`, `motionMin`, intended `shotScale`, and
semantic `category`. A `textCard`/`titleCard` that needs designed motion
carries a complete `visual.storyboard` accepted by `motion_reel`; an asset
that is not rendered and placed is not an executed visual decision. An
`existing` visual must name a real clip/media id already in the project.
A `slots[]` array may ride along as the optional shopping list - one
opening at a time, with its role, treatment, entry/exit and the rich
description the stock search ranks against - but it must agree with the
segments, and a plan without it is a plan the segments alone describe:
the slots are a shopping list, never a second timeline. There is no
universal footage/graphics/hold percentage: choose the mix from this
film's story, evidence, emotion, and available material.

**Phase 10 — Storyboard and shot list.** Turn every plan segment into a
shot the timeline can execute: `visual.type`, shot scale, subject,
movement, screen direction, the line or beat it answers, and the reason it
is there (cover / contrast / proof / breathe / transition). A `textCard` or
`titleCard` that needs designed motion carries a complete
`visual.storyboard` accepted by `motion_reel`; a `broll` visual carries its
40+ character semantic description, fallback queries, `hero`, `motionMin`,
shot scale and category; an `existing` visual names a real media id. Write
the **coverage map**: no narration gap without a planned answer, no shot
repeated, no two adjacent shots with the same scale and direction unless
that repetition is the point. This is the plan's visual spine, not a second
timeline — the `segments[]` remain the contract.

**Phase 11 — Motion design code.** Every section makes an explicit motion
decision: a designed comp, a kinetic-caption treatment, or a deliberate
"no comp" with a reason (footage/performance/hold already carries it). The
density floor is one designed beat per section and at least one per 15 s;
every number, percentage, comparison, ranking, trend or process becomes a
comp — `funcut.ticker.count`, `funcut.chart.bars|line|donut`,
`funcut.stats.triptych`, `funcut.compare.split` — never a plain text card.
For each comp, author the actual document: a preset plus one validated
`ops` batch, or a full comp JSON; `motion_inspect` the slots and nodes,
`motion_frame` the entrance/settled/exit stills, and write the exact
comp/ops into `visual.storyboard` so Phase 16 can bake it without guessing.
For kinetic captions, write the word-timed `captions_animate` recipe; for
external motion, `motion_import` the Lottie/GSAP source. Record the decision
and its reason in the plan's `signalLog` (graphic decision) so the critic
can see why the beat exists.

**Phase 12 — Geneva deep design.** This is the comp engine, not a preset
picker. Read `.claude/skills/motion/SKILL.md` and
`prompts/motion_comps.md` whole. For every motion beat, author the actual
Geneva document: `motion_presets` for the nearest starting point, then a
full comp JSON or one validated `ops` batch that changes the nodes, slots,
tracks, timing and easing to this film's numbers. Inspect with
`motion_inspect` (slots, nodes, durations, safe area), prove with
`motion_frame` at entrance, settled and exit, and bake at the cut's exact
width/height/fps. Use `motion_reel` for a multi-scene storyboard,
`motion_overlay` for a transparent comp, `motion_import` for a Lottie/GSAP
source, and `motion_probe` on the baked asset. `motion_preview` and
`geneva_explain` are opt-in; if they are not served, use `motion_frame` and
the motion skill. Every number, percentage, comparison, ranking, trend or
process gets a purpose-built comp; every section gets one designed beat or a
stated reason it does not. Kinetic typography is a comp, not a caption
preset: set the stressed word, the reading order and the exit. Record the
comp id, preset, ops, slots, frame proof and the decision in the plan's
`signalLog`; a comp that is not baked and placed is not an executed visual
decision.

**Phase 13 — Gather (parallel if your harness allows).**
**The BGM gate runs before any music is searched.**
`sense_audio_repair {action: "detect_music_segments", audio: <the
voiceover>}`: when it answers `musicPresent: true` the take already
carries music, and NO bed is searched, fetched or imported on top of it
(its `editingAnswer` says the same — duck or replace, never stack): skip
`bgm_search` entirely, or if the brief still wants a bed behind the
spans the reading marked music-free, duck it deeper than the L floor
(−15 to −18 dB under speech). A positive gate also runs the copyright
check: `sense_audio_repair {action: "identify_music", audio}` reads the
embedded track's tempo, key and mood (an offline descriptor — it cannot
name the song), and you WARN THE USER at once, and again in the Phase 25
report: this voiceover embeds a music track whose licence is unknown;
confirm the rights or replace the take before publishing. Only a take
that answers `musicPresent: false` gets the BGM searches below.

**Cross-check the plan's coverage before you shop (the runner does the
same).** `footage_plan_suggest {transcript, beats?}` reads the dialogue
gaps and monotony runs the take actually contains and answers the
footage each gap asks for; hold the plan's B-roll slots against it — a
planned slot the gaps never asked for and a gap no slot answers are
both found here, before anything is fetched, not after it is placed.

**Shop each timed B-roll segment, not a generic keyword.** Consult
clip-skills' B-roll matching module. The B-roll reinforces the line's job
(contrast / punctuate / breathe), not its literal content. **Two shelves
serve you; the brief decides which.** A **monetized** brief (or one whose
`creative-brief.json` says `monetized: true`) NEVER shops OpenVid — it has
no per-clip licence record and its import is refused in code: shop the
licensed live shelves instead, `stock_search {provider: "pexels" |
"pixabay", query, orientation}` → `stock_fetch {provider, id, url}` →
`stock_contact_sheet {paths: [<the fetched files>], aspect}` → LOOK at the
sheet → `stock_import {path, provider, id, url}` (every hit carries its
licence; keep the provider/id/url triple — the receipt's provenance reads
it). An unmonetized brief may shop OpenVid: for each `segments[i]` whose
`visual.type` is `broll`, call
`openvid_rank_for_slot {description: visual.description, queries:
visual.queries || [visual.query], hero: visual.hero, motion_min:
visual.motionMin, exclude_ids: <every clip id already chosen>}`. The
description is the semantic contract — BM25 builds the page, the CLIP
second pass re-orders it by what the clips SHOW, and the queries blend in
as Jaccard similarity at `tag_weight` (0.3 by default). Reject candidates
shorter than the segment; keep every chosen id in the next request's
`exclude_ids`. Inspect the contact sheet and run the placement gate before
importing. The automatic runner performs the ranking, duration rejection,
continuity check, targeted replacement, import, placement and end trim;
if you invoke it, verify those placed clips rather than adding duplicates.
All `openvid_search`
calls, all `sounds_search` calls, the `bgm_search` — these are independent;
fan them out when you can, and where you cannot, one `openvid_search_batch`
carries the whole B-roll query list (two or three words per query, per
`prompts/broll_matching.md`) in a single round trip. Fetch what you picked
(`openvid_fetch`, `bgm_fetch`), then import (`openvid_import`, `bgm_import`).
If a search comes back thin, follow the fallback ladder in `prompts/broll_matching.md` — never
invent a clip id, never leave a gap. **Never choose B-roll from caption text
alone.** Look at every candidate page: OpenVid's own clips arrive as a
contact sheet — call `stock_contact_sheet {query, rank: true, brief}` and
actually look at the returned image (on the licensed path, the sheet of
`paths` above is the same look); use its `ranking`, reject
dark/letterboxed/generic tiles, and follow its `queries` suggestions when
the candidates are weak.
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

**The continuity gate, before anything is fetched.** With every slot
filled, assemble the manifest — one entry per filled slot, in play
order: `slotId`, `clipId`, `motion` (the clip's own measured score),
`dominantDirection` (its `camera` string's lead: `pan_left+tilt_up` →
`left`, `zoom_in` → `zoom_in`, `static` → `static`), `shotScale` and
`category` (the slot's own vocabulary), `colorTemperature` (your read
of the tile on the contact sheet, warm +1 through cool −1) and
`motionMin` (the slot's floor). Then `footage_continuity {clips}`:
it reads the manifest the way an editor scrubs it — nothing renders,
nothing fetches; the arithmetic is over the numbers you just assembled
— and flags every place the cut will jump: a colour jump past half the
scale, a travel reversing its neighbour, two of one shot scale in a
row, a run of three one category, a clip too still for the slot that
picked it. Each flag names its slot, its rule and its remedy, and the
remedy is the work: `replace` re-shops that slot alone, the `avoid`
hint folded into the description and the rejected clip on its
`exclude_ids`; `downgrade_to_graphic` or `downgrade_to_hold` ends the
shopping — the treatment changes, the slot stops searching, and the
comp or the A-roll carries the span. `count: 0` is the clean answer;
fetch and import only when the gate is quiet.

**Phase 14 — Stock review, gate and continuity.** Do not place the first
page. For each slot, LOOK at the `stock_contact_sheet`; run
`emotion {action: "gate"}` on the candidate against the beat's register,
act semantics and brand taboos; reject two failed checks and drop the beat
rather than pad it. Read the filled manifest with `footage_continuity` and
apply each flag's remedy before fetching: colour jump, reversed travel,
repeated scale, one-category run, under-floor motion. Keep the provider,
id and url licence triple with every chosen clip; a monetized brief never
shops OpenVid. No clip may repeat a shot already used, and no slot may be
filled with a generic "pretty" clip that does not answer its sentence.
Only the approved manifest proceeds to Phase 15.

**Phase 15 — Place (sequential, batched).** Cut to the plan: add B-roll clips
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
behind the ask). The Phase 4 reading places the accents: an SFX peak lands
ON an emphasis word, not merely near its line; the BGM obeys the reading's
`bgm` decision (dry voice → bed it and duck; a take that already carries
music → none added — Phase 13's `detect_music_segments` gate already
skipped the search — and the embedded bed rides, ducked deeper where it
fights the words); and a `speakerChanges` time is staged like a
section change — new angle, colour or lower-third at the change, never a
cut that straddles it. Use `captions_add` for
segment-timed captions. Group each logical chunk into one `edit_apply` batch.

**Let the user watch.** When the server is the window's own (`concat
--mcp window` — the Forge page's voiceover run is not, and its refusal
of `timeline_seek` is expected there), scrub as you place: after each
`edit_apply` batch lands, `timeline_seek` to the moment just placed.
The playhead moves on the timeline the user is looking at, so the edit
arrives as a thing being made rather than a file that appears — and
the refusal on a headless server costs one line, not the run.

**Phase 16 — Motion design (use when the story calls for it).**
A bespoke comp can make an edit clearer, more memorable, or more emotionally
precise; it is not a mandatory quota. A film led entirely by strong footage,
performance, typography, and sound can be complete without a motion comp. The motion layer is
the Geneva surface: `motion_presets` (the library), `motion_inspect` (nodes, slots —
never guess), `motion_frame` (look before you bake — it now returns an inline
image, so check legibility, contrast and safe margins with your eyes), `motion_bake`
(encode a piece in one call), `captions_animate` (word-timed kinetic
captions from measured word times), `motion_reel` (a whole storyboard
as one reel — scenes with MULTI-LAYER stacks: plates under the comp and
grain, light leaks, vignettes and foreground cards over it, each on its own
drift and clock; check one still with `frame: seconds` before rendering),
`motion_overlay` (transparent comps), `motion_probe` (the baked asset) and
`motion_import` (motion made elsewhere: a designer's Lottie/bodymovin
JSON become a comp, or a GSAP-style timeline applied to one — read the
reply's `notes` aloud to the user, because a `skipped:` note means the comp
draws less than the source did, and the user decides whether that is
acceptable). Do not force a designed beat at a fixed interval or percentage.
Use a bespoke comp when it materially improves comprehension, evidence,
emphasis, identity, or emotional impact; otherwise let footage, typography,
sound, or a deliberate hold do the work. In finance/science/tech, when a number, percentage, comparison or process
needs visual explanation, prefer a purpose-built `funcut.ticker.count`,
`funcut.chart.bars`, `funcut.stats.triptych` or `funcut.compare.split`
over a plain text card or a literal stock search: there is no footage of
"73%" or "twice as fast", and the generic-query results for one are the
same clips every other channel in the niche already uses. Stock is for the
filmable nouns; comps are for the claims. Edit every comp: real copy, retimed to its cut, one
element of the preset deleted. The loop is inspect → one validated `ops`
batch → three `motion_frame`s → `motion_bake` at the cut's size →
**`motion_overlay` for any transparent comp** (lower thirds, kinetic
captions: composite the RGBA bake over the real B-roll with `plate` + `at`,
so the overlay does not cover the shot) or `motion_bake {encode: true}` for a
full-frame piece → `media_import` → `clip_add`. An encoded caption reel is
opaque; never lay one over footage you still want to see. Full doctrine:
`prompts/motion_comps.md` and the `motion` skill.

**Phase 17 — Caption choreography.** Captions are a second performance,
not a transcript dump. Use the measured words; group 2–4 words or one
breath, never a wall; hold each line long enough to read twice; put the
stressed word in the accent treatment the plan named; keep every line
inside the platform's safe area; drop filler-only lines and re-check any
word the voice reading gated. For a reel, use `captions_animate` with the
measured word times and a kinetic recipe; for longform, use
`captions_add` with the plan's segment timing. `sense_captions {action:
"align_captions"}` and `{action: "clean_subtitles"}` are the proof, not a
guess. Never let a caption cover a face, a number, or another caption.

**Phase 18 — Sound design.** Build the sound spine: the BGM gate decides
whether a bed exists at all; the bed is cut to the beat grid and ducked per
act to the plan's `duckDb`; every SFX peak lands on a measured emphasis
word or cut, not merely near one; no machine-gun repetition; leave breath
between accents. The spine is MEASURED, not named: `sense_sound_model
{action: "plan"}` turns the edit's own signals (beats with emphasis and
pauses, visual reveals and cuts, the arc's acts, the bed's structure)
into a per-moment accent design — each accent names its file, measured
`syncOffset`, gain under the voice's LUFS and the signal that earned it.
Execute its accents with `sounds_add` + `clip_audio_levels_db`, and
prove EVERY placed accent with `sense_sound {action:
"check_placement"}` (peak on the moment, level under the voice, no
repetition — the reply names the fix when it is wrong). Shopping a role
by hand instead: `sense_sound {action: "rank_sounds"}`; trusting a
shelf name or the manifest's `syncAt` over the measured `peakAt` is the
mistake the shelf audit measured (30% of files wrong by >100 ms).
Full doctrine: `prompts/sfx_design.md`. If the user exports,
`measure_loudness` on that export is the proof; on the timeline,
`watch_render {project, audio: true}` gives the audio envelope. If the
take already carries music, do not stack a bed; warn about the unknown
licence in the report.

**Full editor power.** The served surface is the whole 108-tool director
catalogue; use it where the line earns it: `clip_mask` for a shape or face
that must share the frame, `track_subject` when the mask or label follows a
moving person, `template_instantiate` for a designed opener/lower third/end
card, `tts_generate` only for a missing pickup, `clip_speed` for a designed
retime, `clip_transform` for a deliberate push/reframe, `effect_layer_add`
for a layer with a job, and `analyze_scenes` / `suggest_cuts` for measured
boundaries. `scene_analysis`, `scene_timeline`, `motion_preview` and
`geneva_explain` are opt-in; a specialist run may use `CONCAT_MCP_ALLOW=all`.
None of this is decoration: every use answers a sentence, a claim or a beat,
and the plan's `signalLog` records why it is there.

**Phase 19 — Grade (required).** One consistent look per video. Either ONE
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

**Phase 20 — Grade proof.** Do not call the grade done because a filter is
on the timeline. `preview_frame` an A/B at 25% and 75% of the runtime:
the same look, the right direction, no crushed skin, no clipped highlights,
no bars, no accidental second filter. If the grade is an effect layer, its
duration is the measured runtime, not the default five seconds; if it is
per-clip, every clip carries the same chain. Record the effect id, the
intensity key and both sample times in the run report. A grade nobody
looked at is a guess.

**Phase 21 — Watch the whole cut (never skip).** Call `watch_render` on the
open project: ONE call returns a labelled contact sheet sampled across the
whole timeline, as an image, plus measured craft facts (black frames,
letterbox bars, flat/duplicate shots, static transitions, audio envelope).
Then `craft_critique {sheet, brief, project}` for six 0-10 scores — hook,
story, rhythm, shotChoice, design, sound — and a short list of concrete
revisions. **You must act on at least one revision and call `watch_render`
again** before handing off the timeline. A score below 7 is not ready.

**When the plan carries slots, the revision is the gate's, not a
re-plan.** The engine's revision gate ships at 6.5 (0.65 on the
critic's 0–1 scale) — the skill's bar stays 7, and the gate is the
floor beneath the bar, not a licence under it. Below the line, exactly
the slots the critic named go back to the planning step — re-shop
those alone, their `exclude_ids` carrying what they had, everything
else standing untouched (a re-shop of a passing slot can only lose
what the edit already has). Each attempt renders to its own file —
`edit_v1.mp4`, then `edit_v2`, then `edit_v3`; an attempt nobody can
diff against the one before it is an attempt nobody learns from — and
after two revisions the edit ships anyway with the failures recorded
per slot: shipping with a note beats not shipping. A note-less pass
below the line names nothing, and nothing is invented for it.
Run the checklist in
`prompts/review.md` too: no overlaps or gaps (`timeline_read`), captions
readable and inside the safe area, the grade is present at both check points,
every clip fits the canvas, SFX peaks hit their cuts, the BGM ducks under
speech. `preview_frame` is still there for a close-up on one suspicious
moment — keep it under 5 calls per minute of video.

**Phase 22 — Independent critique and revision.** The editor who made the
cut is the worst judge of it. If the harness exposes subagents, send a
fresh-context critic the contact sheet, the brief and the plan — not this
conversation — and ask for findings, not a score to rubber-stamp. Otherwise
run `craft_critique` inline and say the critique was not independent.
Reconcile the findings; act on the highest-leverage one, not the easiest;
re-watch the whole cut after the revision. Below 7 is not ready. If the
critic names specific slots, re-shop only those, keeping every passing slot
and its `exclude_ids`; never re-roll a passing slot. Record what changed and
what did not.

**Phase 23 — Save and hand off the timeline (no export).** The deliverable
is the editable timeline, not a render: the user wants to customize the cut
after the Director finishes. Do **not** call `export_start` or `export_wait`.
Call `project_save`; write the canonical `<project>/edit-plan.json`; then
`watch_render {project, audio: true}` the OPEN timeline and
`craft_critique` its sheet. Confirm with `timeline_read` that the clips,
captions, comps and audio are on the timeline the user will open. If you
already rendered by mistake, leave the timeline as the deliverable and say
so; do not re-export.

**Phase 24 — Final timeline acceptance.** The pane's independent acceptance
now checks the timeline, not an encoded file. Run the same proof:
`timeline_read` (the saved timeline has clips), `watch_render {project,
audio: true}` (the composited picture and audio envelope), `craft_critique`
(the picture, not a path), `collision_check` (no shared lanes),
`plan_validate` (the saved plan is valid and creativeReady), and
`project_save`. `motion_probe`, `measure_loudness` and `emotion verify` need
a render; without one they are **honestly skipped, not passed**. Persist the
plan; report the measured score, the gates, the licence provenance and any
gate that did not pass. If the pane's independent acceptance rejects or
cannot prove the run, say so — do not call a red receipt a finished timeline.
If the user later exports, that export earns the encoded gates: readable
streams, platform loudness, the rendered picture and the words.

**Phase 25 — Delivery, customization and the wow bar.** A "wow" cut is not a
pile of effects. It has one signature idea the audience can say back, one
visual system (type, palette, grain, transition logic), one sound motif, a
designed beat in every section, and a reason for every cut. Every number,
comparison and process is a purpose-built comp; every B-roll shot answers
its sentence; every caption is readable and timed; every SFX lands on a
peak; the grade is one look; the final critic is independent and the score
is real. Deliver the editable timeline, the plan, the receipt and the honest
failures. The user can then trim, replace, restyle or export without
rebuilding. That is the bar — not "it rendered".

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
segment carries its measured `emotion`/`act` (Phase 3), and every layer
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
silently orphaned. Use a named transition when it clarifies a section
change or creates a motivated passage; a plain cut is often the strongest
choice. Pair designed transitions with sound only when the sound adds meaning.

**P — Pacing.** Let the narration, emotional arc, information density, music,
and visual complexity determine shot duration and cut frequency. The
genre ranges below are starting references, not targets or validation
rules: documentary passages may breathe; data-heavy explanations may move
faster; a reveal may deliberately hold. Interrupt a pattern when attention
needs renewal, not on a timer. The hook should create a compelling question
quickly, but do not sacrifice clarity to meet an arbitrary second count.
Not only hard cuts: the cut-type catalog (`prompts/cuts_and_pacing.md`)
has eighteen — match cuts, smash cuts, whips, montage, cut-on-action, cutaways, inserts, reactions, cross-cutting, freeze and contrast,
cut-on-beat — and a piece that only hard-cuts is a slideshow.

**V — Variety.** A clip id appears at most ONCE in a reel, and at most once
per 60 s in longform, never twice in one section. Every segment's B-roll
query names a new subject (no noun repeated within 30 s). Shot scale climbs
within a section: wide → medium → close → detail. Captions and comps never
repeat the same entrance twice in a row. When the plan carries slots the
tools hold the first two of these for you — every chosen id rides the next
slot's `exclude_ids`, and the continuity gate flags a repeated scale or a
one-category run before anything is fetched.

**M — Motion on every element.** Every clip enters and leaves on purpose:
an entrance (pop 120–180 ms, slide 200 ms, punch zoom 150 ms, dissolve) and
an exit or a hand-off to the next. "Raw in, raw out" is a defect, not a
style. Keyframes via `keyframe_set`; fades via `clip_update`'s
`fadeIn`/`fadeOut`; the grammar is `prompts/keyframes_motion.md`.

**G — The grade is not optional.** One filter recipe per video, applied to
the WHOLE runtime (see Phase 19 — the duration field is the trap), intensity
set once, proven at 25% and 75% of runtime.

**F — Fit the frame.** The canvas is set before anything is placed
(`project_set_video`). OpenVid has NO vertical footage — never search
`orientation: "vertical"`; for 9:16 search the landscape shelves (prefer
720p/1080p sources), then `clip_frame {framing: "fill"}` every clip so the
picture covers the canvas and crops the overhang. A letterboxed or
pillarboxed frame in any verification frame is a blocking defect.

**S — Sound variety.** Never the same sound id twice within 30 s; rotate at
least three ids per role; alternate soft and hard hits, and vary the level
±2 dB between hits of the same role so the ear does not bracket them as
one repeated event. Map motion to the library's own semantics: counters →
`data-countup-*`, chart builds → `data-bar-grow`/`data-chart-reveal`, money
moments → `money-*`, zooms → `whoosh-*`, reveals → `impact-*`. Sound
carries information, not just emphasis: a design sound goes on the cuts
that carry a beat of the story, and roughly one cut in four to six lands
DRY — a sound on every cut of a thirty-cut minute is noise, and the dry
room is what makes the next hit land. Peak ON the cut.

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

**A — The voice is a score, not a bed.** The Phase 4 reading is measured,
and the edit answers to it: only `dead_air` pauses are cut — a `dramatic`
hold or a `scene_turn` gap survives to carry its weight; the pacing follows
the reading's cut style (loud fast lines cut fast, quiet slow lines hold —
never the reverse); an SFX peak or caption highlight lands on an emphasis
word; the words the confidence gate flagged (below 0.5) are re-checked or
dropped before `captions_animate`; a repeated motif brings its visual back;
and the BGM decision comes from the measured noise floor and the music
gate (`detect_music_segments`), not from habit — a rough take is
repaired before it is read (Phase 2), and a take that already carries
music never gets a second bed. A
reading that was never taken is a note in the report, never a guess.

**L — Loudness and music.** BGM rests at −4 to −6 dB, ducks to an absolute
−12 dB under speech (−9 tight, −15 relaxed), rides up in gaps, section
changes land on downbeats when `analyze_beats` gives a grid, and the piece
ends on a downbeat. No bed at all on a take `detect_music_segments`
answered `musicPresent: true` — music never stacks on music; the take's
own bed rides, ducked deeper where it fights the words.

**N — Stickers are MIT (Fluent Emoji): safe in monetized/commercial videos.**
No licence warning is needed; every `magick_search` result carries `license: "MIT"`.

## Automation boundary — what the Python runner actually does

`scripts/auto_director.py` is an execution helper for a plan the Director has
already authored; it does not invent a topic, script or edit plan. It waits
for source transcription, reads voice signals/emotion, validates the plan,
reuses or places the voiceover without silently retiming it, ranks and
duration-checks B-roll, checks continuity and attempts targeted replacements,
imports/places/trims stock, renders supplied motion storyboards and places
them on a top lane, places the plan's captions (segment-timed from the
measured transcript, or the kinetic bake when the plan asks for word-timed
captions), shops and places the SFX to their computed peaks, shops and ducks
the music bed under the measured speech spans, applies the transitions,
entrance keys and the grade the plan carries, exports and waits, transcribes
the exported file, verifies speech timing, samples the actual render and
scores it. A score below 7, missing asset/storyboard, unresolved continuity
flag, or failed verification stops the run.

The craft loop's own revision passes are the runner's too: a score below the
line names its slots, and the named slot is re-shopped with the old clip
excluded, the old clip leaves the timeline, a graphic's named fix is folded
into its storyboard and re-rendered — at most two passes, then the edit
ships with the failures recorded, never a crash. What the runner still does
**not** do is author a NEW creative revision — re-write the plan's structure,
thesis or pacing after a low score; that judgment is the Director's, and the
runner's recorded notes are where the Director reads what to change.

The run is idempotent and resumable (IDEM): every stage records the evidence
of its completion, a re-run skips what still stands on the timeline, the
export lands under the run's own `runs/<run-id>/` directory (with
`report.json`, diffable between runs), and a failed run's next run picks up
after its last completed stage. The OpenVid index also does not supply
measured colour temperature; do not pretend the automated continuity pass
measured a colour jump it cannot see. Use the stock contact sheet for that
visual judgment.

## The acceptance receipt

Every run writes `auto-edit-receipt.json` beside the edit. It is the run's
own evidence, derived from the replies its calls drew — never from what the
plan promised. Read it before you call a run done; it answers the three
questions a green "Completed" cannot:

- **`verdict`** — `accepted`, `incomplete` or `rejected`. A rejected run
  shipped a file whose gates failed; do not publish it.
- **`gates`** — one entry per craft dimension, each with the measured fact
  it was read from. For a **timeline-ready** run, `story` (the plan
  validated and was creatively ready), `timeline` (the saved project reads
  and has clips), `visual` (the composited craft score) and `collisions`
  (no shared lanes) are **required**; `words`, `audio` and
  `export-integrity` need a render, so without one they are `skipped` and
  not required — the receipt says so instead of pretending. If the user
  exports, those three become required and the encoded file is probed,
  measured and re-read. A required gate that was never measured is
  `skipped`, which makes the verdict `incomplete` — never a silent pass. A
  9/10 craft score cannot hide a missing narration track or an unreadable
  file.
- **`capabilities`** — every area of the editor (measurement, planning,
  footage, motion, captions, music, sfx, grade, review, delivery) with
  `consumed: true` only when a tool that serves it answered. Anything left
  unreached is listed, so a skipped capability is a fact on the record
  rather than something to discover later.
- **`assets`** — the provenance manifest: every imported asset's provider,
  id, licence and whether its rights permit a monetized edit. OpenVid
  entries carry no licence (`commercialOk: null`) because the index has no
  per-clip record; `unlicensed()` names exactly those assets before you
  publish.

When the receipt reports `incomplete`, the missing measurement is the next
thing to run — not a reason to re-render the whole edit. When it reports
`rejected`, fix the named gate (replace the B-roll, re-mix the audio, repair the
timeline) rather than rebuilding it. Export only if the user asks.

## 4. Hard rules

1. Never claim "done" before `project_save` has written the timeline, the
   Phase 21 watch/critique loop has produced a score >= 7 after the LAST edit,
   and the Phase 24 timeline acceptance passed. If the user asks for an export,
   `export_wait` must report the file and the encoded gates must pass too.
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
| `prompts/broll_matching.md` | choosing B-roll queries and slot descriptions, shopping by the slot, scoring results, the fallback ladder, aspect fit |
| `prompts/cuts_and_pacing.md` | the eighteen cut types, J/L feel, pacing curves by mode |
| `prompts/transitions_guide.md` | the 122 transitions (classic 22 + the 100 viral pack), the clip-patch mechanics, when a plain cut wins |
| `prompts/sfx_design.md` | sound design: measured role ranking, semantic categories, peak-true placement with check_placement, anti-repetition |
| `prompts/motion_comps.md` | designing graphics: which comp, worked plans, density floors |
| `prompts/music_and_duck.md` | BGM choice (measured candidates, structure before placement), beat alignment, ducking keyframes, longform structure |
| `prompts/text_animation.md` | titles, captions, kinetic text, zones, color methodology |
| `prompts/color_grading.md` | the filter recipes per niche, and how a grade is actually applied |
| `prompts/keyframes_motion.md` | motion grammar: Ken Burns, parallax, punch zooms, entrances/exits |
| `prompts/niche_playbooks.md` | the whole look for a niche in one place (10 niches, reel + longform) |
| `prompts/reels_vs_longform.md` | the two modes' structures, pacing curves, CTA placement |
| `prompts/memory.md` | the editing journal: what to write, when to re-read |
| `prompts/review.md` | the Phase 21 checklist and the definition of done |
| `prompts/creative_direction.md` | Phase 1: turning a topic into a sharp angle, arc and payoff |
| `prompts/style_bible.md` | the one look: palette, type, grade, motion hand, sound, footage, continuity and vertical rules |
| `prompts/ai_media.md` | generative stills/video/3D/audio when a real generator is installed, and the honest fallback when none is |
| `prompts/quality_loop.md` | watch_render -> craft_critique -> revise, and how to read the scores |
| `prompts/power_moves.md` | the 100 concrete moves across the 25 phases, and where each belongs |
| `prompts/signal_matrix.md` | all 14 signal families and 70 named readings, and the decision each informs |
| `prompts/signal_recipes.md` | cut score, cut density, cut type and confidence fallback from the readings |
| `prompts/emotion_to_edit.md` | the seven emotion labels as pace/cut/sound/grade/motion/duck recipes |
| `prompts/token_budget.md` | the 10-minute token estimate and the rules that cut waste, not craft |
| `prompts/tool_reference.md` | every tool's arguments, units, and gotchas |

## 6. The maestro shelf (judgment, when a phase wants taste)

The `maestro` skill sits beside this one at `.claude/skills/maestro/`. This
skill is the hands — the pipeline, the tools, the floors; maestro is a
distilled library of design, motion and film judgment (28 reference modules
over ten vendored corpora: a 157-card product-shot vocabulary, engine-agnostic
motion principles, a sound-design and beat-sync method, art-direction
catalogs). Read it when a phase wants taste, not tool mechanics — and read
only the module the phase needs, never the whole shelf. Its Grill Gate does
not run here: the Phase 1 `creative_brief` is the one locked brief; use
maestro's interview questions to fill it, never to open a second one.

| Phase | Reach into `.claude/skills/maestro/references/` for |
| --- | --- |
| 1 direction | `design-direction.md` (style registers, art direction), `process.md` (brief-locking discipline) |
| 9 plan | `video-direction.md` (story structure, beat planning, pacing), `video-shotcraft.md` (the 157-shot vocabulary for product/demo work) |
| 16 comps | `motion-principles.md` (timing, easing, choreography), `design-foundations.md` (typography, color, hierarchy) |
| 19 grade | `design-foundations.md` (color systems) |
| 21 critique | `design-audit.md` (systematic critique, anti-slop gates) — feed its findings into the mandatory revision |
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
| 1 direction | `01-视频分类与剪辑逻辑.md` (type the video, pick the edit skeleton), `05-节奏与结构.md` (hook three-step, duration strategy, wave pacing) |
| 2 understand | `02-音频驱动剪辑流程.md` (the audio-first spine, breath/gap typology, the three gap rules behind the Phase 7 padding rule) |
| 7 tighten | `02` §三 (气口三规则: keep/expand/crossfade-pad — the judgment behind the 80 ms shrink), `15-人声与对白处理.md` (cleanup chain order, de-noise restraint) |
| 9 plan | `01`/`05` (section skeleton, 时钟理论 wave points as a thinking tool — labeled heuristic), `14-转场决策.md` (transition semantics for the plan's `transitionIn` fields — the "earned, not decorated" rule) |
| 13 gather | `03-素材AB分类与替换.md` (A/B classification to seed queries, the five B-roll rules: cover the sentence, group shots, no repeats, mute the track) |
| 15 place | `04-声音与音乐衔接.md` (J/L-cut feel, duck depths, leaving breath between peaks), `14` (transition choice + duration numbers), `15` (duck dB by context) |
| 17 captions | `13-字幕与文字设计.md` (caption readability six rules, the four-zone text system, word-time honesty) — with the `motion` skill for execution |
| 19 grade | `06-调色.md` (Log/LUT order, correct-then-style, scope-quantified deltas — map them to `effect_key_set` values) |
| 21 watch | `05` (pacing diagnosis: information density vs shot length, the fix list for a sagging middle) |
| 25 delivery | `12-导出与交付.md` (resolution/fps follow the source, bit-rate "recommended not max", platform framing table, naming), `15` §六 (LUFS targets per platform — what `measure_loudness` should read) |

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
