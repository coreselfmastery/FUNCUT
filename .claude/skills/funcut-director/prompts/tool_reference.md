# Tool reference

The schema each tool ships with (MCP `tools/list`) is **authoritative** for
argument names — read it before first use in a session. This page is the map,
the semantics, the units, and the traps. Arguments are checked against that
schema before a tool runs: a key it does not name is refused by name (a
misspelling is one round trip, never a silently-dropped field), and edits
autosave as they happen — the document is written after every applied edit,
so work survives without `project_save`.

## Jobs (the long work)

`export_start` · `analysis_wait` · `export_wait` · `export_status` ·
`analyze_scenes` · `analyze_beats` · `analyze_silence` · `transcribe` · `tts_generate` ·
`models_list` · `models_download`
(`export_cancel` and `project_close` need `CONCAT_MCP_ALLOW=all` - the
default surface a director runs on refuses both, deliberately.)

Start tools return `{"job":"jN"}` at once. Wait with `analysis_wait` /
`export_wait` `{job, timeoutSeconds}` (default 60, max 590; call again while
`state == "running"`). `export_status {job}` works on ANY job. Results of
analyses and transcription arrive as JSON in the waited status's `output`;
`tts_generate`'s `output` is the WAV path (import it with `media_import`).
Traps: a "not downloaded" model error is one `models_download {kind,id}` call
away — run `models_list` first, download what the plan needs, wait with
`analysis_wait`, and never retry the failing tool in a loop (`tiny.en` fastest
English whisper, `base` the multilingual default, `small` for a non-English take;
`kokoro-int8-multi-lang-v1_0` the default voice). Analysis tools need the
Python extras installed (`scenedetect`, `opencv-headless`, `librosa`) — same
rule: surface the pip hint.

## Motion design (Phase 16's engine)

`motion_presets` · `motion_inspect {preset|comp, project?, ops?}` ·
`motion_frame {project, preset|comp, at, ops?, width?}` ·
`motion_bake {project, preset|comp, ops?, width?, height?, fps?, crf?, encode?, name?}` ·
`captions_animate {project, words, preset?, wordsPerLine?, maxLineSeconds?, gapSeconds?, width?, height?, fps?, plate?, name?}` ·
`motion_overlay {frames | hash + project, plate, at?, fps?, width?, height?, crf?, out?}` · `motion_reel {project, storyboard, name?, frame?, plan?, crf?}` (a whole storyboard as one reel — scenes carry multi-layer stacks: plates under the comp, `above` overlays over it, each with zoom/pan/offset/scale/fit/blend/opacity/fades and its own start/hold) · `motion_import {action: lottie|gsap, project, file?|timeline?, preset?|comp?|ops?, name?}` (motion made elsewhere: a bodymovin JSON become a comp, or a GSAP-style timeline applied to one; the reply's `notes` name every skip, approximation and clamp — read them to the user)

- The loop: `motion_presets` → `motion_inspect` (never guess node ids) → one
  `ops` batch → `motion_frame` at 2-3 moments → `motion_bake {encode: true}` →
  `media_import` the file → `clip_add` at the cut.
- `ops` is the full authoring vocabulary: set-slot, set-text, set-words,
  set-word-confidence, set-markup, move, retime, set-track, set-track-points, set-noise-track
  (seeded drift), fade-in, slide-to, pop, add-group, add-rect, add-text,
  add-chart (data-driven bar/line/area/donut/dot/compare/progress/
  candlestick/timeline/table, any domain), add-counter
  (a baked count-up), add-particles (seeded, baked to keyframes),
  set-camera (zoom/pan/roll with per-layer depths), set-blend,
  set-opacity, set-duration, set-background, delete. One batch, applied
  all-or-nothing.
- Encoded files are opaque (H.264/VP9/ProRes carry no alpha here); a
  transparent overlay keeps its alpha only as the PNG sequence in the bake
  `dir`. Full-frame comps (stat cards, chapters, backdrops, stings) encode
  perfectly; overlays that must sit over real footage go through
  `motion_overlay` — take `dir` and `hash` from the `motion_bake` reply —
  which composites the RGBA frames over the plate with the alpha
  honoured (see `prompts/motion_comps.md`).
- `captions_animate` needs MEASURED word times (`[text, start, end]`
  triples). Whisper gives segment times - per-word estimates are not
  timing. 1080x1920 30fps default; captions sit centered in the lower
  third.

## Projects and reading

`concat_version` · `project_list` · `project_create {location,name,video?}` ·
`project_open {path}` · `project_save` ·
`project_set_video` · `project_get {path}` · `timeline_read {path}` ·
`collision_check {path}` · `plan_validate {plan}` ·
`auto_edit {path,copy?,script?,stockProvider?}` · `media_probe {path}` ·
`motion_probe {file|path, project?}` · `timeline_seek {path, time}` ·
`timeline_linkage {path, on}` ·
`media_import {path,file}` · `effects_list {kind}` (the effect/filter/transition/audio
packages)

- One API session per folder: open before editing; a project the window has
  open cannot be opened headless (and vice versa).
- `timeline_read` is the compact view (lanes, clips in time order, ids, media)
  — use it for gap/overlap checks; `collision_check {path}` reads the same
  view and reports every place two clips share one lane (`clean: true` when
  none — the placing tools never make one, so a collision means a hand-edited
  document). `project_get` is everything.
- `plan_validate {plan}` checks the director plan against its canonical schema,
  timeline continuity, visual references, supported audio/caption ranges, AND
  the structure: `mode` (a 70 s "reel" is refused), `sections` (hook first,
  ordered roles, covering the runtime, boundaries on cuts), cut variety (a run
  of six hards is named), entrances (floor M), comp density (one per 15 s) and
  SFX spacing (no id twice within 30 s). It also checks the plan's
  **`signalLog`** — the decision log naming the voice_signals readings behind
  each cut, caption, graphic, BGM and transition choice: a plan with a
  voiceover and no signalLog is refused as not creativeReady, and an entry
  citing a signal the profile does not answer is a named problem. Errors refuse the plan; warnings are
  the director's conscience — read them, then fix the plan, not the wording.
  Run it before asset gathering/editing; fix errors and revalidate. It cannot
  verify asset availability.
- `auto_edit {path}` is the window's fixed one-click spine as one call:
  repair, transcribe, voice signals, dead-air tightening, captions, B-roll,
  grade, music, sfx, preview, revise, save, verify and watch. It edits a
  sibling copy by default and leaves the editable timeline without
  exporting. It is not the Director's route - it has no creative brief,
  no eye-picked stock and no plan - but it is the deterministic baseline
  an agent can run when the request is "just auto edit this".
- `media_probe` before cutting anything unfamiliar: duration, streams, frame
  size, rate, codecs. Units: seconds and frames-per-second as floats.
  `motion_probe` is the engine's own reader's version of the same question —
  streams, frame size and rate, duration, the audio's channels — for any
  picture, video or sound the engine reads; use it when the answer feeds
  motion or render decisions.
- `timeline_seek {path, time}` scrubs the window's playhead (seconds):
  it needs the window (`--mcp window`) and a headless run refuses it. Use
  it to put the frame you are about to `preview_frame` on screen for the
  user. `timeline_linkage {path, on}` turns the timeline's ripple linkage
  between lanes on or off — off when placing on several lanes at once so
  one lane's trim does not shove the others.
- The project `path` in every edit tool is the folder `project_open` took.

## Cutting

`clip_add` · `clips_add_sequence` · `clip_trim` · `clips_split` · `clips_move` ·
`clips_remove` · `clip_update` · `clip_cut_spans {clipId, spans:[[start,end]…], ripple}` ·
`clip_frame` · `clip_speed {speed 0.0625–16}` · `track_add` ·
`edit_apply {command|commands}` · `edit_undo` · `edit_redo`

- Every wrapper tool is ONE undoable `edit.apply` command; `edit_apply` with a
  `batch` of commands is one undo step for many changes — prefer batches.
- `clip_cut_spans` is the silence remover: feed it the spans from
  `analyze_silence` (they are in seconds, `{spans:[{start,end}]}`).
  `ripple` closes the gaps left behind.
- Speed changes the on-timeline length (same source stretch kept).
- Ramps and freeze-frames exist as raw commands through `edit_apply`:
  `setClipSpeedCurve` (eased speed over time) and `freezeFrame`.
- Engine minimums: a clip is at least 1/60 s; scale 0.05–8; offsets ±3
  frame-fractions; stretch 0.1–10; preview/export max side 8192 px.

## Audio gain units

- `clip_audio_levels_db {path,clipId,baseDb,keyframes?,replaceAutomation?}`
  sets the clip's base gain and optional volume keys in dB relative to source
  unity (`0 dB` = unchanged; `−60 dB` is the editor's silence floor;
  negative attenuates; positive boosts), converting to the engine's linear
  multipliers. Each keyframe is `{at,gainDb,ease?}`;
  `gainDb` is an absolute target relative to unity, and `at` is 0–1 of the
  clip. `baseDb` is used outside keyed regions. `replaceAutomation: true`
  clears existing volume keys in the same undoable batch.
- `clip_update.patch.volume` and `keyframe_set` on `volume` are linear gain
  multipliers (`1` = unchanged), not dB. `sounds_add` cannot set gain; use
  the created clip id with `clip_audio_levels_db` after adding it.

## Picture and motion

`clip_transform` · `keyframe_set` · `keyframe_clear` · `clip_mask` ·
`clip_stabilize` · `timeline_background` · `effect_layer_add` · `effect_key_set` ·
`track_subject` ·
`title_add` · `title_style_words` · `title_clear_word_styles` · `captions_add`

- `keyframe_set` animates `scale`, `offsetX`, `offsetY`, `rotation`, `opacity`,
  `volume`, and the mask fields (`maskX/maskY/maskWidth/maskHeight/
  maskRotation/maskRoundness/maskFeather`). `at` is a 0–1 fraction of the
  clip; `ease` is a CSS cubic-bezier `[x1,y1,x2,y2]`. `volume` key values are
  linear gain multipliers, not dB; use `clip_audio_levels_db` for dB targets.
- `effect_layer_add` is how a transition or effect package goes on a clip;
  `effect_key_set` sets a package parameter (`concat.warm`'s is `temperature`,
  Kelvin 3000–6000, default 4600 — `effects_list` gives you each package's
  parameter table at runtime; never hardcode what you can read).
- Masks: shapes `linear, mirror, circle, rectangle, heart, star`.
- `timeline_background`: `none` | `blur {amount 0–1}` | `color {#rrggbb}` |
  `image {mediaId}` — vertical videos on a 16:9 canvas want `blur`.
- `captions_add` places segment-timed captions from explicit `{start,end,text}`
  intervals; it does not align words. `title_add` paints styled titles (see
  `prompts/text_animation.md` for the 31 presets and the style fields:
  `fontSize` is a FRACTION OF FRAME HEIGHT — 0.04 engine default, 0.05–0.06
  short-form; `offsetY` 0.35 ≈ lower third). `title_style_words` styles
  individual words of a title — the kinetic-caption look, per word, the
  tool behind word-timed emphasis — and `title_clear_word_styles` takes the
  per-word looks off, back to the shared style. Style the emphasized word,
  not the whole line.
- `track_subject {path, media|clip, …}` is the subject's path across a
  clip, frame by frame, as crop keyframes: it samples, finds what stands
  out from the frame and from the temporal median, smooths the path
  (holding through occlusion and exits) and returns crop keyframes a
  `clip_transform` or a mask follow can ride. It is the tool behind a
  label or mask that must follow a moving person.

## Libraries

The licensed live shelves a monetized brief MUST shop (Pexels, Pixabay):
- `stock_search {provider: "pexels"|"pixabay", query, orientation}` - hits carry `id`, `caption`, `url`, `license`, `author`, `width`, `height`, `seconds`
- `stock_fetch {provider, id, url}` - downloads to the cache, replies `{path, bytes}`
- `stock_import {path, provider, id, url, author?}` - imports with licence metadata, replies `{mediaIds, license, author, view}`; the author is written into the document beside the file, so the credit survives the reply; keep the provider/id/url triple - the receipt's provenance reads it

`openvid_status/prepare/search/search_batch/rank_for_slot/get/fetch/import`
(937k clips;
sources `pixabay|pexels|celebv|other`; orientation `vertical|horizontal|square`
with ±5 % slack — the shelf's own footage is landscape, so a `vertical`
filter answers empty and vertical reels shop the live `stock_search`
shelves instead (see `prompts/broll_matching.md`); sorts `relevance|aesthetic|motion|temporal|shortest|longest|
surprise`) · `bgm_status/prepare/search/fetch/import` (FreePD 1,025 CC0
tracks; MobyGratis/OpenLo-Fi 166, free for NON-COMMERCIAL use only; sorts
`relevance|shortest|longest|surprise`; run `sense_audio_repair {action:
"detect_music_segments"}` on the voiceover before any `bgm_search` — a take
that already carries music gets no bed) ·
`sounds_search/add` (530 CC0 sounds; categories: Whoosh, Impact, Riser,
Motion, UI, Notification, Keyboard, Digital, Foley, Writing, Ambience, Music;
`sounds_add` starts the file at `start`; the search result's `syncAt` says
where its peak lands — a guess the file may contradict, which is what the
measured doors below exist to end) ·
`sense_sound {action: describe_sound|rank_sounds|check_placement|index_library}`
(sounds measured, not guessed: `describe_sound` reads one file — level,
crest, attack, body, transients, brightness, bass, and where its peak
ACTUALLY falls; `rank_sounds` scores the six role fits with the evidence;
`index_library` builds the cached shelf index and reports every `syncAt`
the measurement contradicts — 61 of the 530 shipped sounds; and
`check_placement` verifies a PLACED accent: peak alignment against its
target, role fit, level, repetition) ·
`sense_sound_model {action: describe|index|rank|plan|generate}` — the five
actions are `describe`, `index`, `rank`, `plan` and `generate` (what a
sound MEANS: `describe` = YAMNet's 521 event labels + a CLAP embedding
with the description it best matches, fused with the measurements;
`index` the semantic shelf index; `rank` a description against the shelf
— the model wins transitions, the measurements win risers and textures;
`plan` the per-moment sound design — beats, visual arc, BGM structure,
style, durationSec and voiceLufs in, a moment-by-moment design out: the
role, the selected asset, the reason, the start, the measured peak, the
gain, the trim, the masking and the repetition constraints — the source
of SFX decisions, not filename browsing) ·
`sense_sound_model {action: "generate", kind, prompt,
seconds, seed, project, monetized?}` writes a sound the shelf does not
have — `sfx-small` (TinyAudio-MF, MIT, ~12 s per call) for accents,
`sfx-large` (Stable Audio Open Small, Stability Community Licence, ~24 s)
for the hook and for a monetized run's pad bed, `bgm` (TinyMusician,
CC-BY-NC 4.0 via its MusicGen-small weights, a MusicGen decode of
minutes) for a short bed; the file lands in
`<project>/generated-audio/` and `sounds_add {file}` or
`bgm_import {file}` places it like a shelf sound. `monetized: true`
tightens the project's own brief, never loosens it. A model that is not
downloaded, a licence a monetized cut may not spend, or a near-silent
output all *answer* — `ok: true`, `model: absent` or `reason: "licence"`,
`generated: false`, no `path` — so the shelf (or, for the bed, the
sfx-large pad) takes over; never retry, and never treat the answer as a
failure. The generator sets download headless:
`models_download {kind: "sound", id: "sfx-small"}` — the ids are
`sound-models`, `sfx-small`, `sfx-large`, `bgm`. The picture reader's
first-edit set (TransNetV2, Depth Anything, DINOv2, CLIP) downloads the
same way: `models_download {kind: "picture", id: "picture-models"}` · `magick_status/prepare/search/fetch/add` (shelves:
`magick` 145k AI pictures **CC BY-NC — never in monetized work**, and
`openmoji` 4,565 emoji CC-BY 4.0)

- `openvid_search_batch {queries, …same filters as openvid_search}` runs the
  same search once per entry of `queries` (1–8, two or three words each) in
  one call — the reply's `searches` carry one `{query, total, clips}` per
  entry. This is how a plan's whole B-roll list is shopped in one round trip.

- `openvid_rank_for_slot {description, queries?, hero?, k?, tag_weight?,
  motion_min?, exclude_ids?}` is the slot's own search — the rich
  description is the query (BM25 page, CLIP second pass), the fallback
  queries blend in as Jaccard similarity at `tag_weight` (0.3 default),
  and the hard filters are enforced twice, on the request and on the
  page: a used id or a clip under `motion_min` is not ranked lower, it
  is not there. `hero` asks for thirty candidates, any other slot for
  twelve; every clip returns with `slotScore` and `slotRank`. An empty
  `description` is an error, never a quiet fallback to the queries.
  `footage_continuity {clips: [{slotId, clipId, colorTemperature, motion,
  dominantDirection, shotScale, category, motionMin}]}` reads the
  filled manifest — colour jumps past 0.5 on the −1..+1 scale, reversed
  travels, repeated scales, three of one category in a row, clips under
  their slot's floor — and answers `{count, flags}`, each flag naming
  its slot, rule and remedy: `replace {avoid}` or
  `downgrade_to_graphic` / `downgrade_to_hold`. The shopping discipline
  is `prompts/broll_matching.md`'s first section.

- `prepare` warms a library's index (OpenVid's is ~435 MB — check
  `openvid_status` first; it reports `missing|downloading|ready|failed`).
- Search results carry `aesthetic`, `motion`, `temporal` scores, `camera`
  motion (`static`, `zoom_in`, `pan_left+tilt_up`, …), `seconds`, `bytes`.
  Use them — see `prompts/broll_matching.md`.
- Fetch then import; the import reply says where the clip landed.

## Templates and export

`template_list` · `template_instantiate {template, location, name, fills:
[{mediaId, file}]}` · `export_start {path, output, crf?, preset?, width?,
height?, rateNum?, rateDen?, codec?, tenBit?, colorRange?, hdr?}` ·
`export_status` · `export_wait`

- Export: crf 20 default (0-63 for export_start; a plan's export.crf is validated 0-51), preset "medium",
  codec `h264|hevc|av1`. Stages you will see in status: `cutout`, `stabilize`,
  `rendering` (status stage "video"), `mixing audio` ("audio"), `muxing`
  ("mux"); states `running|done|cancelled|failed`.
- `preview_frame {path, time, output?, width?, height?}` — the true composite
  at a time, as an image to look at (titles, effects, transforms included).
  Without a size it fits within 960 on the long side, which each clip's
  proxy serves: the first frame is fast and the next ones faster. Budget:
  ≤5 calls per minute of finished video.

## Perception and the creative loop

`creative_brief {path, action: get|set, …}` · `local_vlm {action: status|setup,
providers|configure|clear, profile?}` · `stock_contact_sheet {query|ids|paths,
limit?, cols?, rank?, brief?}` · `watch_render {path|project, samples?, times?,
cols?, audio?, targetLufs?, targetTruePeakDbtp?}` · `craft_critique {sheet|pngPath|pngB64,
brief?, autoSetup?, profile?}` · `emotion {action:
extract|brand|gate|verify|template, project?}` · `voice_signals {action:
extract|selftest, audio?, transcript?, lines?, script?, emotion?}` ·
`sense_discourse` · `sense_voice_quality` · `sense_prosody_style` ·
`sense_audio_repair` · `sense_captions` · `sense_sound` · `sense_sound_model` ·
`sense_visual {action:
measure_motion|camera_motion|shot_scale|visual_sync|scene_cuts|visual_quality|
palette|saliency|composition|motion_blur|tracks|measure_clips|bench|models|
action_class|depth_map|subject_mask|visual_embedding|describe_scene|shot_boundaries|
film_shot_type|optical_flow|aesthetic|pose|…|hf_search|hf_files|download, …}` · `measure_loudness {path,
targetLufs?}` · `suggest_cuts {path, cuts?, minGap?, bpm?}` ·
`footage_plan_suggest {transcript: {segments: [{start,end,text}]}, beats?}`
(the dialogue gaps and monotony runs that want B-roll, named before the plan is written)

- `voice_signals` is Phase 4's reader — the acoustic score the plan cuts
  by. Its reply carries `signalConfidence` (how much to lean on each signal
  family, and why), `decisionPolicy` (which signals drive each decision) and
  `directorHints` (the cut style, the caption gate, the BGM call). The plan's
  `signalLog` cites its signals; `plan_validate` holds the citations.
- `sense_visual` is the picture's door: eleven readings need nothing
  installed (motion, camera, shot scale, sync, scene cuts, quality, palette,
  saliency, composition, blur, tracks), and the ONNX shelf behind them —
  thirty model entries, twenty-one door actions — adds what a
  classical read cannot (a shot boundary, depth, an embedding, an action, a
  mask, a pose). A door with no file refuses with the reason. Install one
  from the Hub in three calls - `hf_search` (find a repo), `hf_files` (its
  ONNX files), `download` (fetch it; `HF_TOKEN` for a gated repo, and a
  digest is pinned when none is given). `measure_clips` spends whatever is
  installed on the stock sheet; `enrich: false` keeps it classical and fast.
- The eight `sense_*` family tools are the consumption layer: `action` names
  the reading (`sense_discourse {action: "score_script_quality", script}` —
  the ten discourse readings, the six voice-quality readings, the six
  prosody readings, the eight audio-repair actions, the five caption
  readings, the four sound measurements, the five sound-model actions and
  the picture's thirty-eight — motion, camera travel and its
  direction, shot scale, audio-visual sync, scene cuts, the technical
  read, palette, saliency, the batch `measure_clips` read, the `bench`,
  the `models` listing and the shelf's door actions; `selftest: true` proves
  one). Each reading names its own traps
  in its reply; the family schema documents the shared inputs. Three
  audio-repair actions are wired into the pipeline itself:
  `enhance_speech` (Phase 2 — a rough take is repaired before it is
  analyzed, and every later reading runs on the enhanced `file`),
  `detect_music_segments` (Phase 13 — the BGM gate before `bgm_search`:
  `musicPresent: true` means no bed, duck or replace, never stack) and
  `identify_music` (Phase 13 — the copyright check on a take that carries
  music: an offline descriptor, never a title match; the user is warned
  before publishing).

- `sense_sound` — what a sound file IS, measured (`sound_sense.py`, pure
  standard library, no model). Four actions, Phase 18's measuring
  instrument:
  - `describe_sound` {path} — the measured read of one file: level
    (`peakDbfs`, `rmsDbfs`, `crestDb`), timing (`attackMs`, `bodySec`,
    `decaySec`, transients, and `peakAt` — where the file's peak
    ACTUALLY falls), spectrum (`centroidHz`, bands, flatness, pitch
    movement) and the six role fits with the evidence behind each.
  - `rank_sounds` {role, paths|dir|index, limit?, maxDuration?,
    minDuration?} — candidates scored for one of the six roles
    (`quick_transition`, `cinematic_reveal`, `action_impact`,
    `gentle_entrance`, `emotional_texture`, `tension_build`); each
    candidate carries `measuredPeakAt` beside the manifest's `syncAt` so
    the drift is visible. An `index` written by `index_library` ranks
    without re-decoding the shelf.
  - `check_placement` {path, clipStart, intendedAt|moment, inPoint?,
    gainDb?, voiceLufs?, voicePeakDbfs?, recent?, toleranceMs?} — the
    proof after placing: the measured peak against the moment it was
    meant to land (a `sync_off` issue names `moveBy`), the level under
    the voice, the role against the moment, and repetition inside
    `repetitionWindowSec` (default 12, `maxSameRole` default 3). Every
    issue names its fix; `recent` is the `[{path, role}]` list of what
    you already placed.
  - `index_library` {dir, out?} — measure the whole shelf once (530
    sounds ≈ 2.5 min) into `sound-index.json`; rank against it after.
- `sense_sound_model` — what a sound MEANS (`sound_model.py`; YAMNet's
  521 AudioSet labels + a 512-d CLAP audio/text embedding; numpy +
  onnxruntime when installed, NEVER required — without the models every
  action still answers with `model: {state: "absent", reason}` and the
  measured half only, and `rank` says `degraded` — a weaker answer it
  admits, never hides). Five actions:
  - `describe` {file} — one record per asset: identity, YAMNet labels,
    the CLAP embedding + the description it best matches, the Sound
    Sense measurements, quality flags, roles, `syncOffset` and trim
    advice.
  - `index` {dir, out?, workers?} — the shelf as one
    `sound-model-index.json`, cached by size + mtime + model version
    (default 2 workers).
  - `rank` {index, query, role, limit?, maxDuration?} — a creative
    description in words ("dark cinematic braam", two or three words)
    scored by CLAP similarity fused with the measured role fit, the
    weights fitted on the 530 shipped sounds.
  - `plan` {index, beats, visual?, arc?, bgm?, style?, durationSec?,
    voiceLufs?} — the per-moment sound design: every signal at once
    (the beats' emphasis words and pauses, the visual events/cuts/motion
    hits, the arc's acts, the bed's structure with its `cutSafePoints`)
    into accents — each with file, `startAt`, `peakLandsAt`,
    `syncOffset`, `gainDb` under the voice's LUFS and the why — plus
    held silences and the music plan (change points, ducking spans, the
    ending). Feed it Phase 3's arc, Phase 4's beats and Phase 13's bed
    structure; execute its accents with `sounds_add` +
    `clip_audio_levels_db`; prove each with `sense_sound {action:
    "check_placement"}`.
  - `generate` {kind, prompt, seconds?, seed?, steps?, threads?,
    project?, out?, monetized?} — the one action that WRITES a sound.
    `kind` picks the model: `sfx-small` (TinyAudio-MF, MIT, 10 s mono,
    0.48 GB) for a short accent, `sfx-large` (Stable Audio Open Small,
    Stability AI Community Licence, 11.9 s stereo, 1.2 GB) for a hook or
    a written bed, `bgm` (TinyMusician — MusicGen-small's weights,
    CC-BY-NC 4.0, 1.0 GB) for a music bed. The prompt is the sound in
    words ("a heavy wooden door slams shut in a stone hall"), and the
    file lands in `<project>/generated-audio/` with a
    `<file>.licence.json` beside it, placed like any shelf sound
    (`sounds_add`, or `bgm_import` for a bed). A kind that is not
    downloaded answers `model: "absent"` in about a second; a kind that
    would not fit the machine answers `reason: "memory"` before it loads
    anything; and `bgm` in a monetized project (`monetized: true`, or the
    brief's own flag — the caller can only tighten it) is refused with
    `reason: "licence"`, because the MusicGen weights are
    non-commercial. Generate for the moment the shelf cannot cover — a
    sound that must match the words — never as a way to skip shopping.

- The loop that uses them is the skill's Phase 1 and §4 (and
  `prompts/quality_loop.md`): brief first, plan, `stock_contact_sheet` to SEE
  B-roll candidates before choosing, `motion_frame` to SEE graphics, export,
  `watch_render` to SEE the whole cut, `craft_critique` for the honest score.
  `stock_contact_sheet`'s `ids` name OpenVid clips from any search, so a
  shortlist from `openvid_search_batch` can be looked at before anything is
  fetched. `local_vlm` reports whether the local vision critic is installed
  (`setup` fetches it, no system install); without it the ranking and the
  critique degrade to captions and scores.
- `emotion` is Phase 3's reader and Phase 13's gate (see
  `prompts/creative_direction.md` and the placement-gate floor E): `extract`
  on the transcript, `gate` on stock candidates before anything is placed,
  `brand`/`template` for the brandKit, `verify` in Phase 24 — and only on a
  render the user asked for.
  Give it `project` and the brief and brandKit load themselves.
- `measure_loudness` is the only proof a loudness target was met — EBU R128
  integrated LUFS, true peak and the error against `targetLufs` (−14
  short-form default, −23 broadcast). `export_wait` reports no loudness.
  `watch_render {audio: true}` measures the same numbers over the whole cut
  in one pass (the default), with `targetLufs`/`targetTruePeakDbtp` to
  score against; `{times: [...]}` pins exact frames and skips the audio
  side. `craft_critique` takes `pngPath` (the sheet a watch or contact
  sheet wrote — the loop's intended input), `autoSetup: true` to fetch the
  local critic unattended, and `profile: "minicpm"` for the denser-text
  local model.
- `suggest_cuts` is Phase 2's no-Python rhythm path: BPM estimate and
  beat-aligned `cutTimes` from any file with an `ffmpeg` binary — use it
  when the librosa extras are not installed.
