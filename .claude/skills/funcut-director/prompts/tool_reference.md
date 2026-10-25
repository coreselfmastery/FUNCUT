# Tool reference

The schema each tool ships with (MCP `tools/list`) is **authoritative** for
argument names — read it before first use in a session. This page is the map,
the semantics, the units, and the traps.

## Jobs (the long work)

`export_start` · `analysis_wait` · `export_wait` · `export_status` · `export_cancel` ·
`analyze_scenes` · `analyze_beats` · `analyze_silence` · `transcribe` · `tts_generate`

Start tools return `{"job":"jN"}` at once. Wait with `analysis_wait` /
`export_wait` `{job, timeoutSeconds}` (default 60, max 600; call again while
`state == "running"`). `export_status {job}` works on ANY job. Results of
analyses and transcription arrive as JSON in the waited status's `output`;
`tts_generate`'s `output` is the WAV path (import it with `media_import`).
Traps: transcription needs a downloaded whisper model (`tiny.en` fastest
English, `base.en` the default sweet spot, `small` multilingual) — a "not
downloaded" error is an instruction to surface, not a retry loop. Analysis
tools need the Python extras installed (`scenedetect`, `opencv-headless`,
`librosa`) — same rule: surface the pip hint.

## Projects and reading

`concat_version` · `project_list` · `project_create {location,name,video?}` ·
`project_open {path}` · `project_save` · `project_close {path,save}` ·
`project_set_video` · `project_get {path}` · `timeline_read {path}` ·
`media_probe {path}` · `media_import {path,file}` · `catalogue` (the
effect/filter/transition/audio packages) · `effects_list {kind}`

- One API session per folder: open before editing; a project the window has
  open cannot be opened headless (and vice versa).
- `timeline_read` is the compact view (lanes, clips in time order, ids, media)
  — use it for gap/overlap checks. `project_get` is everything.
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

## Picture and motion

`clip_transform` · `keyframe_set` · `keyframe_clear` · `clip_mask` ·
`clip_stabilize` · `timeline_background` · `effect_layer_add` · `effect_key_set` ·
`title_add` · `captions_add`

- `keyframe_set` animates `scale`, `offsetX`, `offsetY`, `rotation`, `opacity`,
  `volume`, and the mask fields (`maskX/maskY/maskWidth/maskHeight/
  maskRotation/maskRoundness/maskFeather`). `at` is a 0–1 fraction of the
  clip; `ease` is a CSS cubic-bezier `[x1,y1,x2,y2]`.
- `effect_layer_add` is how a transition or effect package goes on a clip;
  `effect_key_set` sets a package parameter (`concat.warm`'s is `temperature`,
  Kelvin 3000–6000, default 4600 — `effects_list`/`catalogue` give you every
  package's parameter table at runtime; never hardcode what you can read).
- Masks: shapes `linear, mirror, circle, rectangle, heart, star`.
- `timeline_background`: `none` | `blur {amount 0–1}` | `color {#rrggbb}` |
  `image {mediaId}` — vertical videos on a 16:9 canvas want `blur`.
- `captions_add` places word-synced captions; `title_add` paints styled titles
  (see `prompts/text_animation.md` for the 31 presets and the style fields:
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
`sounds_add` takes the time the peak should land on) · `magick_status/prepare/
search/get/fetch/add` (shelves: `magick` 145k AI pictures **CC BY-NC — never in
monetized work**, and `openmoji` 4,565 emoji CC-BY 4.0)

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
