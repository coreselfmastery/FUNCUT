# Tool reference

The schema each tool ships with (MCP `tools/list`) is **authoritative** for
argument names — read it before first use in a session. This page is the map,
the semantics, the units, and the traps.

## Jobs (the long work)

`export_start` · `analysis_wait` · `export_wait` · `export_status` · `export_cancel` ·
`analyze_scenes` · `analyze_beats` · `analyze_silence` · `transcribe` · `tts_generate` ·
`models_list` · `models_download`

Start tools return `{"job":"jN"}` at once. Wait with `analysis_wait` /
`export_wait` `{job, timeoutSeconds}` (default 60, max 600; call again while
`state == "running"`). `export_status {job}` works on ANY job. Results of
analyses and transcription arrive as JSON in the waited status's `output`;
`tts_generate`'s `output` is the WAV path (import it with `media_import`).
Traps: a "not downloaded" model error is one `models_download {kind,id}` call
away — run `models_list` first, download what the plan needs, wait with
`analysis_wait`, and never retry the failing tool in a loop (`tiny.en` fastest
English whisper, `base.en` the default sweet spot, `small` multilingual;
`kokoro-int8-multi-lang-v1_0` the default voice). Analysis tools need the
Python extras installed (`scenedetect`, `opencv-headless`, `librosa`) — same
rule: surface the pip hint.

## Motion design (Phase 5b's engine)

`motion_presets` · `motion_inspect {preset|comp, project?, ops?}` ·
`motion_frame {project, preset|comp, at, ops?, width?}` ·
`motion_bake {project, preset|comp, ops?, width?, height?, fps?, crf?, encode?, name?}` ·
`captions_animate {project, words, preset?, wordsPerLine?, maxLineSeconds?, gapSeconds?, width?, height?, fps?, plate?, name?}`

- The loop: `motion_presets` → `motion_inspect` (never guess node ids) → one
  `ops` batch → `motion_frame` at 2-3 moments → `motion_bake {encode: true}` →
  `media_import` the file → `clip_add` at the cut.
- `ops` is the full authoring vocabulary: set-slot, set-text, set-words,
  move, retime, set-track, set-track-points, fade-in, slide-to, pop,
  add-group, add-rect, add-text, set-blend, set-opacity, set-duration,
  set-background, delete. One batch, applied all-or-nothing.
- Encoded files are opaque (H.264/VP9/ProRes carry no alpha here); a
  transparent overlay keeps its alpha only as the PNG sequence in the bake
  `dir`. Full-frame comps (stat cards, chapters, backdrops, stings) encode
  perfectly; overlays go under a `plate` or wait for the Motion clip.
- `captions_animate` needs MEASURED word times (`[text, start, end]`
  triples). Whisper gives segment times - per-word estimates are not
  timing. 1080x1920 30fps default; captions sit centered in the lower
  third.

## Projects and reading

`concat_version` · `project_list` · `project_create {location,name,video?}` ·
`project_open {path}` · `project_save` · `project_close {path,save}` ·
`project_set_video` · `project_get {path}` · `timeline_read {path}` ·
`plan_validate {plan}` · `media_probe {path}` · `media_import {path,file}` ·
`effects_list {kind}` (the effect/filter/transition/audio packages)

- One API session per folder: open before editing; a project the window has
  open cannot be opened headless (and vice versa).
- `timeline_read` is the compact view (lanes, clips in time order, ids, media)
  — use it for gap/overlap checks. `project_get` is everything.
- `plan_validate {plan}` checks the director plan against its canonical schema,
  timeline continuity, visual references, supported audio/caption ranges, AND
  the structure: `mode` (a 70 s "reel" is refused), `sections` (hook first,
  ordered roles, covering the runtime, boundaries on cuts), cut variety (a run
  of six hards is named), entrances (floor M), comp density (one per 15 s) and
  SFX spacing (no id twice within 30 s). Errors refuse the plan; warnings are
  the director's conscience — read them, then fix the plan, not the wording.
  Run it before asset gathering/editing; fix errors and revalidate. It cannot
  verify asset availability.
- `media_probe` before cutting anything unfamiliar: duration, streams, frame
  size, rate, codecs. Units: seconds and frames-per-second as floats.
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
`title_add` · `captions_add`

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
  short-form; `offsetY` 0.35 ≈ lower third).

## Libraries

`openvid_status/prepare/search/get/fetch/import` (937k clips; sources
`pixabay|pexels|celebv|other`; orientation `vertical|horizontal|square` with
±5 % slack; sorts `relevance|aesthetic|motion|temporal|shortest|longest|
surprise`) · `bgm_status/prepare/search/fetch/import` (FreePD 1,025 CC0
tracks; MobyGratis/OpenLo-Fi 166; sorts `relevance|shortest|longest|surprise`) ·
`sounds_search/add` (430 CC0 sounds; categories: Whoosh, Impact, Riser,
Motion, UI, Notification, Keyboard, Digital, Foley, Writing, Ambience, Music;
`sounds_add` starts the file at `start`; the search result's `syncAt` says
where its peak lands) · `magick_status/prepare/search/fetch/add` (shelves:
`magick` 145k AI pictures **CC BY-NC — never in monetized work**, and
`openmoji` 4,565 emoji CC-BY 4.0)

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
`export_status` · `export_wait` · `export_cancel`

- Export: crf 20 default (63 max, lower = better), preset "medium",
  codec `h264|hevc|av1`. Stages you will see in status: `cutout`, `stabilize`,
  `rendering` (status stage "video"), `mixing audio` ("audio"), `muxing`
  ("mux"); states `running|done|cancelled|failed`.
- `preview_frame {path, time, output?, width?, height?}` — the true composite
  at a time, as an image to look at (titles, effects, transforms included).
  Budget: ≤5 calls per minute of finished video.
