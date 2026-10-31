# Concat for AI agents (MCP)

Concat ships an [MCP](https://modelcontextprotocol.io) server inside the app. An agent - Claude Code, Claude Desktop, or any other MCP client - starts it as `concat --mcp` and can then create projects, import media, cut, add titles and captions, keyframe, apply effects, look at frames and export, with the same edit commands the window uses. Nothing extra to install.

### Stock clips (OpenVid)

The `openvid` tools search 937,957 stock clips (the OpenVid-1M dataset on
Hugging Face) by caption and quality numbers and fetch only what is used.
The first call on a machine is `openvid_prepare`, which downloads a 435 MB
local index once (poll `openvid_status` until `ready`); after that every
search is local and instant. A typical run: `openvid_search` with words
and quality floors ("sunset ocean", `minAesthetic: 5.2`) → `openvid_get`
to read the best candidates in full → `openvid_import` with `start: 0` to
lay them on a lane in one undoable step. Frame sizes are measured from
each clip's own MP4 header; the first search with an `orientation` filter
also takes a one-time census of every shelf (a few seconds), after which
`square` finds the three 512x512 shelves and `horizontal` the WebVid
shelf's 720p/1080p landscape footage - nothing vertical has been measured
in it, and an honest vertical search returns nothing rather than
everything.

Caption search is SQLite FTS5 BM25, not a visual embedding. It now drops
stop words and de-duplicates terms (`"rain on a window"` is `rain window`,
not four mandatory prefixes), and when an exact AND search returns **zero**
rows it retries once with the terms ORed and widened by a small,
inspectable concept lexicon (`burnout` → `exhausted`, `tired`, `moon`, …).
BM25 still puts exact matches first. This raises recall but does not make
the index semantic: always call `stock_contact_sheet` and look at real
frames before importing. See `docs/STOCK_LICENSING.md` — OpenVid is a
research dataset, not rights-cleared stock.

### Sound effects

The `sounds` tools search the built-in CC0 sound library this repository
ships (see `docs/AUDIO_LIBRARY.md`) and place what they find:
`sounds_search` with words against the name, tags, use-for lines and
description ("cinematic impact", "click", "soft swish for a card
transition" - the little words are dropped, and where a word matched
ranks it: the name counts most, then the tags, then the description - or
a `category`: Whoosh, Impact, Riser, Motion, UI, Notification, Keyboard,
Digital, Foley, Writing, Ambience, Music) → `sounds_add` with `start: 0`
to lay the sound on a lane in one undoable step. Each match carries what
an agent needs to pick without hearing anything: a `description` of what
the sound is, `useFor` lines saying what it is for, and `syncAt`, seconds
in where its peak lands, for placing it by the moment it is for. The
first use of a sound downloads it (a few hundred kilobytes at most,
checked against its SHA-256) and keeps it in the app's cache; every sound
is CC0, so nothing asks for attribution. `sounds_add` creates a clip at its
source's unity gain; it does not accept a level. Use the dB-aware
`clip_audio_levels_db` tool on the returned clip id to set the planned gain.
### Background music (FreePD and MobyGratis)

The `bgm` tools search 1,191 whole CC0 music tracks on two shelves and
fetch only what is used. FreePD is the Internet Archive's mirror of Kevin
MacLeod's CC0 library (~1,000 tracks, grouped as freepd.com grouped them:
upbeat, epic, comedy, scoring, world…); Lo-Fi is Moby's MobyGratis
instrumentals as the OpenLo-Fi pack (166 tracks, 10 categories). Nothing
is bundled: `bgm_prepare` fetches the shelves' catalogs once (a few MB,
plus a one-time measuring of the Lo-Fi pack's zip, a few seconds; poll
`bgm_status` until the shelf you read is `ready` - an optional `source`
narrows the prepare to one shelf, and a search that names no shelf simply
searches whichever shelves are ready), and a track's audio arrives only
when it is picked - FreePD as its own file, a Lo-Fi track as **one ranged
read of exactly that track's span out of the pack's 554 MB zip**, never
the whole zip. A typical run: `bgm_search` with words and a length
("city rain", `maxSeconds: 180`, `source: "lofi"`) → `bgm_fetch` to check
a candidate's `localPath` (or just listen: the same file previews in the
window) → `bgm_import` with `start: 0` to lay it on a lane in one
undoable step. `surprise` as the sort is the shuffle that picks when you
cannot; a track whose length is unknown is passed by a duration filter
rather than guessed at.

### Stickers (MAGICK and OpenMoji)

The `magick` tools search 149,758 transparent-PNG overlays on two
shelves and fetch only what is used. MAGICK is OneOverZero's dataset of
145,193 captioned AI-generated pictures with the backgrounds already
taken away (people, animals, objects, gestures); OpenMoji is the
project's colour emoji set (4,565, CC-BY 4.0, and its tags are searched:
"smile" finds the grinning face). Nothing is bundled: `magick_prepare`
fetches both indexes once (~18 MB; poll `magick_status` until both are
`ready`), and a picture's PNG arrives only when it is picked - one plain
GET, ~1 MB for a MAGICK picture, a few KB for an emoji, checked to be a
PNG that can hold transparency before it is kept. A typical run:
`magick_search` with words ("red balloon", `source: "magick"`) →
`magick_add` with `id`, `start` and a placement (`scale: 0.3,
offsetX: 0.2`, `duration: 3`), which lays the picture *over* the video -
the first free lane above the highest occupied, never beneath it - as
one undoable step beside the clip's own. MAGICK's pictures are
AI-generated and CC BY-NC 4.0: fine for personal video, not for
commercial use; OpenMoji is CC-BY 4.0. The window's Stickers page
searches the same engine, and its heart button keeps the favourites the
`favourites` filter narrows by.

### Analysis and speech (the agent's senses)

Eight tools give an agent what a director has before touching a timeline: the
shot list, the beat grid, the dead air, the words - and a voice. Each runs
as a job (returns `job` at once) and is waited on with `analysis_wait`, the
same loop as `export_wait`, which returns the status; a finished analysis
carries its JSON result in the status's `output`:

- `analyze_scenes` - PySceneDetect's shot changes of a video, `{start,
  end}` seconds. Optional threshold/detector/minSceneLen.
- `analyze_beats` - librosa's bpm, beat grid and onsets of a piece of
  music, in milliseconds (`beatsMs`, `downbeatsMs`, `onsetsMs`).
- `analyze_silence` - the dead air of a recording, `spans` and their
  complement `speech` in seconds, the shape `edit.apply`'s `removeSpans`
  cuts by. Optional thresholdDb (35 default) and minSilenceMs (500).
- `transcribe` - Whisper's timed text segments `{start,end,text}`; this is
  segment-level timing, not word alignment. The optional language hint is
  passed to Whisper (ISO code or BCP-47 tag; the primary tag is used).
  English-only models reject explicit non-English hints; choose a multilingual
  model such as `small` or `base` for other languages. `base.en` is the default.
- `tts_generate` - a voiceover spoken to a WAV in an open project's own
  audio folder; `output` is its path and length, ready for `media_import`.
  Voices are the same ids the Settings > Speech page shows (Kokoro 0-52);
  a missing model is one `models_download` away, window or no window.
  `delivery` (neutral, conversational, urgent, excited, calm, awed,
  somber, authoritative) sets rate AND sentence-breath for a paced read;
  `speed` (0.5–2) / `pauses` (0–1) override it, and `steps` (1–16) is
  Pocket's quality dial. These are the two dials the shipped voices
  actually honour — it is pacing, not a claimed emotion embedding.
- `models_list` - every voice (tts) and transcriber (whisper) model this
  build knows, with sizes and which are on disk; the ids are what
  `models_download` takes.
- `models_download` - one model from Concat's mirror, as a job a headless
  caller can wait on with `analysis_wait`; `kind` is `tts` or `whisper`,
  and a model already on disk ends at once.
- `analysis_wait` - waits for any job (analysis, speech, model download or
  export) up to `timeoutSeconds` (60 default, 600 at most) and returns its
  status.

The motion layer - the design door the director skill's Phase 5b assumes -
is six more tools, thin wrappers over the `concat-motion` crate (the same
vocabulary the `concat-motion-agent` CLI speaks):

- `motion_presets` - the twelve motion-graphics presets (lower thirds,
  kinetic captions, stat cards, wordmark stings, pull quotes, chapter
  chips, subscribe cards, terminals, bar charts, split compares, counting
  numbers, mesh backdrops) with their slots, sizes and whether the Geneva
  engine is on the machine.
- `motion_inspect` - what is in a comp before it is touched: nodes, spans,
  animated properties, slots, validation problems; an optional `ops` batch
  is applied first, so an edit can be checked before it bakes.
- `motion_frame` - one frame of a comp at a time in seconds, drawn in tens
  of milliseconds, returned as an **inline image** (and also written under
  the project's `motion/`). Look at it before baking.
- `motion_bake` - a comp (preset or file, optionally edited by `ops`)
  baked to PNG frames under `cache/comps/`, and with `encode: true`
  written as one video under `motion/` ready for `media_import` and
  `clip_add`. The encoded file is opaque - only the PNG sequence keeps a
  transparent comp's alpha.
- `captions_animate` - a measured word-timed transcript as one caption
  video: lines by the caption rules, each line a kinetic comp, baked and
  encoded in one call. This is the word-synced caption path; the
  timeline's own `captions_add` stays segment-timed.
- `motion_reel` - a whole storyboard as one reel: several scenes (preset
  or comp file each, with `ops`, `at`, `hold`), **multi-layer scenes** -
  a scene's `layers` stack bottom first, plates under the comp and, with
  `above: true`, grain, vignettes, light leaks and foreground cards over
  it, each with its own `zoom`, `pan`, `offset`, `scale`, `fit`, `blend`,
  `opacity`, `fade_in`/`fade_out`, `start` and `hold`; differing drift
  between layers is what reads as depth - plus shared assets, audio beds
  and a curtain. `plan: true` lays it out without rendering, `frame:
  seconds` returns one still as an inline image, and the render lands
  under `motion/` ready for `media_import` and `clip_add`. The same
  storyboard document the `concat-motion-agent reel` verb eats; every
  error names the scene it stopped at.

## Perception and the creative loop (eight more tools)

An agent that gets a PNG *path* back cannot see the picture. These tools
return MCP `type: image` content blocks, so the model can actually look:

- `creative_brief` - read/write `creative-brief.json` in the project:
  audience, platform, goal, thesis, curiosityGap, payoff, emotionalArc,
  referenceStyle, tone, brandKit, constraints, avoid. Returns a
  `directorPrompt` and the list of missing core fields. `plan_validate`
  now reports `creativeReady`, `creativeBlockers`, `craftScore` and
  `creativeNotes` in addition to `valid`; a plan with no brief, a hook over
  3 s, repeated visuals or no designed comp is **not** creativeReady.
- `stock_contact_sheet` - decode a real frame of each B-roll candidate
  (`query`, `ids` or local `paths`) into one labelled contact sheet image,
  with per-candidate pixel metrics, an energy-centroid `cropHint` and a
  `reframeHint.clipTransform` pair of `offsetX`/`offsetY` values ready to
  pass to `clip_transform` for the target `aspect` (9:16 by default).
  `rank: true` re-ranks candidates against the brief with the local vision
  model and suggests better queries.
- `watch_render` - sample the open timeline (via `preview.frame`, no ffmpeg
  CLI needed) or a rendered movie, and return the whole cut as ONE image
  plus measured facts: black frames, letterbox bars, flat and duplicate
  shots, static transitions, an audio energy envelope. The sheet is also
  written under the project's `cache/sense/`, and its path comes back as
  `pngPath` - which is how the next tool sees the same frames, because an
  agent cannot hand image bytes back in a tool call.
- `craft_critique` - six 0-10 scores (hook, story, rhythm, shotChoice,
  design, sound), notes and concrete tool-level revisions. Pass the whole
  `sheet` object `watch_render` returned: the critic reads its `pngPath`
  (a raw `pngB64` also works). Uses a local
  vision model when available, otherwise says `engine: "heuristic"` and
  grades measurements honestly — black frames, bars, flat and overexposed
  shots, duplicates, static transitions, colour saturation and the
  opening frame are all measured from real pixels (confidence 0.5).
  **The bundled 0.5B model only flags visible
  defects** (`engine: "local-vlm-flags"`, at most 1 point per axis); it was
  tested end-to-end and cannot judge taste. Full six-axis model scoring
  (confidence 0.78) needs a stronger critic: `local_vlm {action: "configure",
  provider, apiKey}` with a free-tier hosted vision API (groq, gemini, zai,
  openrouter — one call, see [docs/VLM.md](VLM.md)) or `FUNCUT_VLM_ENDPOINT`
  pointing at any OpenAI-compatible vision server. Every
  model response is schema-validated and degenerate answers fall back to the
  measured score. With `project`, each pass is journaled to
  `craft-history.json` and the reply reports `pass`, `previousOverall`,
  `delta`, `improved` and a `trend`, so the render -> inspect -> revise
  loop can be measured across turns instead of remembered.
- `suggest_cuts` - beat-aligned cut points from the soundtrack with
  **no Python extras**. Decodes audio with ffmpeg, estimates BPM from an
  energy-onset function and returns `cutTimes` spread across the whole
  piece plus an `energy` curve. Unlike `analyze_beats` (librosa), this
  works on a bare headless install. Optional `cuts`, `minGap`, `bpm`.
- `measure_loudness` - FFmpeg's EBU R128 meter on an exported file:
  integrated LUFS, true peak dBTP, loudness range, the error against the
  platform target (`targetLufs`, default −14; −23 for broadcast), and
  specific mix notes. `watch_render {path, audio:true}` reports the same
  figures alongside the contact sheet. `export_wait` reports no loudness,
  so this is the only way to **prove** a target was met.
- `motion_overlay` - composite a baked RGBA PNG sequence (`frames`, or
  `hash` + `project`) over real footage (`plate`) at `at` seconds, honouring
  the comp's transparency, and return an H.264 clip ready for
  `media_import` + `clip_add`. This is the honest alpha path: the sidecar's
  video codecs and FFmpeg's WebM muxer do not reliably carry alpha, so the
  transparency is honoured at the compositing step instead of faked in an
  encoder. Without a plate it composites over a solid `background`.
- `local_vlm` - the vision critic, on any of three rungs (see
  [docs/VLM.md](VLM.md) for the full map). `status` reports what is
  reachable; `setup` fetches SmolVLM2-500M-Video-Instruct Q8_0 + projector
  (~546 MB, 0.5B params, CPU-only, 4 GB laptop is enough) and a pinned
  llama.cpp server into the app data dir; no system install, no pip.
  `providers` lists the free-tier hosted vision APIs (groq, gemini, zai,
  openrouter) with endpoints, default model ids and honest free-tier
  notes; `configure` with `provider` + `apiKey` wires one up in a single
  call — the key lands in `vlm.json` in the app's own data dir, is masked
  in every reply, and unlocks full six-axis scoring in `craft_critique`
  (hosted means the contact-sheet frames go to that provider's API; the
  local rungs never leave the machine). `clear` removes the saved config.
  `FUNCUT_VLM_ENDPOINT` / `FUNCUT_VLM_API_KEY` / `FUNCUT_VLM_MODEL` env
  vars do the same without a config file — any OpenAI-compatible server
  works. `profile: "minicpm"` is ~2.2 GB and reads dense text
  better. `vlm.py ask` supports grammar-constrained JSON and retries once
  without the schema on servers that reject it.
- `emotion` - the emotional read of the voiceover that drives every layer
  (see [docs/EMOTION.md](EMOTION.md) for the full map). `action "extract"`
  takes a `transcript` (a transcribe job's output), `lines` or raw `script`
  text and returns, per line: the **emotion** (7 labels) with intensity and
  confidence, the **act** (hook / setup / tension / turn / reveal / proof /
  cta), a hormone shorthand, the TTS `delivery` + `speed`, the **easing
  curve** as exact concat-motion authoring JSON (tense eases in, warm eases
  out, a burst pops), colour temperature + grade hint, the SFX register,
  the BGM **duck depth** in dB, pacing and 2–3 word stock `queries`; plus
  the whole piece's **arc** (shape, score, warnings) checked against the
  brief's `emotionalArc`. Engines: a stem lexicon always (confidence 0.55,
  English-only), or `tabularisai/multilingual-emotion-classification`
  (XLM-RoBERTa across 23 languages, its 11 labels folded onto the edit's 7)
  on HuggingFace's free Inference API when `FUNCUT_EMOTION_API_KEY` /
  `HF_TOKEN` is set (75/25 blend, confidence 0.85) — with and without the
  key are both first-class.
  `project` loads the brief automatically. `action "gate"` checks stock
  candidates against a beat's `{emotion, act}` and the brand guide: 0–1
  failed checks passes, **2+ rejects the candidate, every candidate rejected
  = DROP THE BEAT** — never pad with a bad match. `action "brand"`
  validates a brand guide (`action "template"` returns one to copy) and
  applies it; `action "verify"` aligns the edit's words against the
  re-transcribed export — what a render loses, duplicates or shifts is a
  defect the eye cannot see.

The intended loop: `creative_brief` -> `emotion` (extract, on the
voiceover) -> `plan_validate` (creativeReady true) -> `stock_contact_sheet`
-> `emotion` (gate, before anything is placed) -> build -> `watch_render` ->
`craft_critique` -> revise -> `watch_render` again -> export ->
`watch_render {path}` + `measure_loudness` + `emotion` (verify) on the file
that shipped. See `src/crates/concat-mcp/sense/README.md` and the
director skill's `prompts/quality_loop.md`.

The first three need a Python 3 with the extras named in the analysis
folder's `requirements.txt` (scenedetect, OpenCV headless, librosa) - a
one-time `pip install -r <app data>/analysis/requirements.txt`. When
something is missing the reply says the exact line to run instead of
failing silently; `analyze_silence` prefers an `ffmpeg` binary on PATH and
falls back to librosa when there is none.

## Set it up

Point your client at the Concat binary with the one argument `--mcp`.

| System | Command |
| --- | --- |
| Linux (.deb, .rpm, Arch) | `/usr/bin/concat` |
| Linux (AppImage) | the `.AppImage` file itself |

**Claude Code:**

```sh
claude mcp add concat -- /usr/bin/concat --mcp
```

**Claude Desktop** (Settings > Developer > Edit Config, `claude_desktop_config.json`), and most other clients, which take the same shape:

```json
{
  "mcpServers": {
    "concat": {
      "command": "/usr/bin/concat",
      "args": ["--mcp"]
    }
  }
}
```

## How it reaches Concat

`concat --mcp` works one of two ways, and the `concat_version` tool says which (`mcp.mode`):

- **Through the window** (`window`). When Concat is open with **Settings > Automation > Scripts and apps** on, the window writes where its server is, and its token, to `automation.json` in Concat's config folder (readable only by you). The MCP server finds it and goes through the window's server. This is the safer way while you are editing: the agent cannot open the project you have on screen and save over it, and exports take turns with yours.
- **On its own** (`headless`). Otherwise the MCP server runs Concat's engine itself, with no window. Nothing has to be open. Every Concat process locks the project folders it has open (`concat.lock`, let go on close or when the process dies), so a project open in the window is refused here with a note to turn on Automation and use `--mcp window`, and the window refuses a project an agent has open.

Choose explicitly with `--mcp window` (fail rather than run on its own) or `--mcp headless`; the default is `--mcp auto`. To reach a server elsewhere, set `CONCAT_API_ADDRESS` (e.g. `127.0.0.1:7420`) and `CONCAT_API_TOKEN` in the client's `env` for the server.

Either way the agent writes files - projects, exports, frames, contact sheets, encoded bakes, the craft journal - only where the session is allowed to. In headless mode that is the directory the server was started in, which is where an agent's work area is; name more folders with `CONCAT_MCP_ROOTS` (a path list, `:`-separated) when a project lives elsewhere. Through the window, the window's server holds its own line. Writes the sense and motion layers make on an agent's word - a contact sheet's `out`, a bake's `name`, the brief, the journal, and the motion tools' `project` itself - are held to the same roots and refuse `..`, exactly like the API's own writes.

The agent edits projects of its own; it does not edit the project open in the window while you watch. That ("live mode") is planned in [#337](https://github.com/jub0t/Concat/issues/337).

## What the agent can do

Tools are named for what they do, and their argument schemas are generated from the Concat API's own types, so they carry the same limits and descriptions the engine does.

| Area | Tools |
| --- | --- |
| Projects | `project_list`, `project_create`, `project_open`, `project_save`, `project_close`, `project_set_video` |
| Reading and plans | `timeline_read` (compact: lanes, clips in time order, media, ids), `collision_check` (every place two clips share a lane; `clean` when none), `project_get` (everything), `concat_version`, `plan_validate` (checks a director plan before edits) |
| Media | `media_probe`, `media_import` |
| Stock clips | `openvid_status`, `openvid_prepare`, `openvid_search`, `openvid_search_batch` (several queries, one call), `openvid_get`, `openvid_fetch`, `openvid_import` |
| Sound effects | `sounds_search`, `sounds_add` |
| Music | `bgm_status`, `bgm_prepare`, `bgm_search`, `bgm_fetch`, `bgm_import` |
| Stickers | `magick_status`, `magick_prepare`, `magick_search`, `magick_fetch`, `magick_add` |
| Editing | `clip_add`, `clips_add_sequence`, `clip_trim`, `clips_split`, `clip_cut_spans`, `clips_move`, `clips_remove`, `clip_update`, `clip_transform`, `clip_speed`, `clip_frame`, `clip_audio_levels_db`, `track_add` |
| Picture | `clip_mask`, `clip_stabilize`, `timeline_background` |
| Titles and captions | `title_add`, `captions_add` |
| Keyframes and effects | `keyframe_set`, `keyframe_clear`, `effects_list`, `effect_layer_add`, `effect_key_set` |
| Analysis | `analyze_scenes`, `analyze_beats`, `analyze_silence`, `analysis_wait` (results as JSON in the status's `output`) |
| Speech | `transcribe`, `tts_generate`, `models_list`, `models_download` |
| Anything else | `edit_apply` (any edit command, or a batch of them as one undo step), `edit_undo`, `edit_redo` |
| Templates | `template_list`, `template_instantiate` |
| Motion design | `motion_presets`, `motion_inspect`, `motion_frame`, `motion_bake`, `motion_overlay`, `captions_animate`, `motion_reel` |
| Export | `export_start`, `export_status`, `export_wait` (with progress), `export_cancel` |
| Looking | `preview_frame` (the composited frame as an image) |
| Perception and the creative loop | `creative_brief`, `stock_contact_sheet`, `watch_render`, `craft_critique`, `suggest_cuts`, `measure_loudness`, `local_vlm`, `emotion` |

`plan_validate` takes the edit-plan object defined by the Director skill's
canonical schema and checks its structure, segment continuity, visual
instructions, and supported audio/caption ranges before any assets are placed.
It cannot prove creative quality or that a search will return media.

`clip_audio_levels_db` sets base gain and optional volume keyframes in dB
relative to source unity (`0 dB` = unchanged; `−60 dB` is the editor's silence
floor), converting to the model's linear gain values. Keyframe `gainDb` values
are absolute targets relative to unity;
`clip_update.patch.volume` and `keyframe_set` on `volume` instead take a linear
multiplier (`1` = unchanged). `sounds_add` has no gain argument.

Edits autosave as they happen - the document is written after every applied edit, so work survives without `project_save` - and arguments are checked against each tool's schema before the tool runs: a key the schema does not name is a misspelling, and the refusal names it rather than dropping it on the way to a half-read edit. Times are seconds; a keyframe's position is a fraction of its clip (0 to 1); sizes and offsets are fractions of the frame.

## Protocol

MCP over stdio, newline-delimited JSON-RPC 2.0. Both protocol eras are served: the `initialize` handshake (2025-11-25, 2025-06-18, 2025-03-26, 2024-11-05) and the per-request `_meta` of 2026-07-28 with `server/discover`. `ping`, `tools/list`, `tools/call`, progress notifications for `export_wait`, and `notifications/cancelled` are supported. Standard output carries only protocol; the server logs to standard error.

For scripts that do not speak MCP, the same API is served as JSON-RPC lines on a socket from Settings > Automation; see [the developer docs](https://concatenate.pages.dev/docs).

## The safety net: read-only, the allowlist, and per-tool timeouts

Three switches, off by default so the server an agent has always had is the server it still gets:

- **`--mcp-read-only`** (binary) or **`CONCAT_MCP_READ_ONLY=1`** (the app's `concat --mcp` and anything else that starts it): every tool that changes something is refused by name - an edit, an export, a stock import, the reel's render half. The reads stay, and `motion_reel` keeps its look halves (`plan`, `frame`, `sheet`): they change nothing outside the project's cache, and taking them away takes away the only way to see a design before committing to it.
- **`--mcp-allow a,b,c`** (binary) or **`CONCAT_MCP_ALLOW=a,b,c`**: the only tools this server serves. A tool not on the list is refused as if it were not there - its description still lists it, but the surface served is what the operator said, not what the build happens to carry. `.forge/mcp.toml` carries the repo's own list, ready to paste.
- **Per-tool timeouts**: every tool carries its own limit (`Tool::timeout_secs` - the VLM download and the overlay encode 1200 s, transcribe 900 s, exports and the sense family 600 s, the renders and watches 300 s, everything else 60 s), and a call that outlives it is cancelled and answered for at the deadline, with the limit in the message. The client-side `call_timeout_secs` stays as the ceiling over all of them; the engine's own calls are bounded a second time, per call, by the Geneva guard (see `docs/MOTION.md`).

## The reel contact sheet

`motion_reel` with `sheet: true` renders four stills of the reel - a quarter in, half, three quarters, near the end - tiles them into one image, and returns it inline **and** as `pngPath` under the project's `cache/sense/`, which is the path `craft_critique` takes. The creative loop over a reel is then: `motion_reel {sheet: true}` → `craft_critique {sheet: "<pngPath>", brief}` (the bare path string, or the whole reading object - both are accepted) → revise what scores under 7 → again.
