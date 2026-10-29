# FEATURES — the whole inventory, read from the source

This page lists everything FUNCUT is, verified against the source tree (not
against marketing or README claims): every tool an agent can call, every page
the window shows, every engine, library, model and skill. Counts are exact and
were re-derived from the code for this page. Where a thing is documented in
detail elsewhere, the link says so.

The one-paragraph version: FUNCUT is the Concat editor (a native Rust, GPU
composited, CapCut-class video editor for Linux) tuned for small machines,
plus an agent stack bolted onto it — a built-in MCP server with **87 tools**,
an analysis/sense layer, an emotion-driven edit pipeline with a brand kit, a
motion-graphics engine with its own sidecar, and two Claude skills (a director
that runs an 11-phase measured pipeline, and maestro, a vendored design and
film judgment library) — all drivable from a Forge page inside the editor
with one click.

## The numbers

| What | Count | Where it lives |
| --- | --- | --- |
| MCP tools | **87** | `src/crates/concat-mcp/src/tools.rs` (`catalogue()`) |
| JSON-RPC API methods | 48 requests + 8 progress notifications | `src/crates/concat-api/src/message.rs` |
| Core edit commands (`edit.apply` ops) | **52** | `src/crates/concat-project/src/commands/mod.rs` |
| Effect/filter/transition/audio packages | **127** = 57 filters + 23 effects + 22 transitions + 25 audio | `src/crates/concat-effects/packages/` |
| Text presets | 31 named + default | `src/crates/concat/text-presets/` |
| Motion presets | 12 | `src/crates/concat-motion/src/presets.rs` |
| Bundled CC0 sounds | 430 (11.5 MB, 12 categories) | `audio/` |
| BGM library | 1,191 CC0 tracks (FreePD 1,025 + MobyGratis 166) | fetched on demand |
| Stock clips (OpenVid) | 937,957 | ~435 MB local index, fetched per clip |
| Stickers | 149,758 (MAGICK 145,193 + OpenMoji 4,565) | ~18 MB indexes, fetched per sticker |
| Bundled fonts | 41 title faces + 6 script fallbacks + UI face | `src/crates/concat-text/src/lib.rs` |
| UI languages | 26 | `src/crates/concat/src/i18n.rs` |
| Rebindable keyboard actions | 28 | `src/crates/concat/src/keymap.rs` |
| Director-skill prompt playbooks | 18 | `.claude/skills/funcut-director/prompts/` |
| maestro reference modules | 28 over 10 corpora; **157** shot cards | `.claude/skills/maestro/` |
| Whisper models offered | 6 (tiny.en/tiny/base.en/base/small.en/small) | `src/crates/concat-speech/src/transcribe.rs` |
| Neural models in the engine | 6 ONNX + 1 compiled-in | see *Models* below |
| Library pages in the window | 13 | `src/crates/concat/ui/workspace/media-pane.slint` |

## The engine

Everything upstream Concat is, verified in this tree:

- **Timeline.** Multi-track, multi-timeline (sequences as tabs), untyped lanes
  (any media on any lane), magnetic mode, ripple edits, snapping, trim-follow,
  split/razor tool, delete-left/right (Q/W), merge, freeze frame, reverse,
  mirror, rotate, per-lane lock/visibility/mute/height, marquee and band
  selection, keyframe diamonds on the ruler, continuous zoom about the
  pointer, phone pinch zoom with glide momentum.
- **Cutting vocabulary (the 52 commands).** Media (add/remove/placeholder/
  fill-slot), clips (add, add-at-first-free, sequence, match, replace, move,
  trim, split, remove-spans, remove, update, consolidate, merge, freeze,
  detach/reattach audio, enhanced copies, colour range), speed (set + curve),
  transforms (place/scale/rotate, fit/fill), text and shape clips, effect
  layers, keyframes (set/clear/clear-all on clips and effects), masks,
  cutout, cutout strokes, stabilize, tracks, timelines (add/remove/rename/
  reorder/select, background, video format), fonts, media paths.
- **Keyframes.** 12 animatable properties per clip (scale, offset x/y,
  rotation, opacity, volume + 7 mask numbers) with full CSS cubic-bezier
  easing — arbitrary curves, not just presets — edited in a dopesheet or a
  curves view with per-key bezier handles. Effect knobs are keyframable too;
  speed ramps are a separate smooth-Hermite curve system (1/16×–16×) with six
  presets (montage, hero, bullet, jump-cut, flash-in, flash-out).
- **Effects.** 127 packages, all GPU (WGSL) for picture and FFmpeg chains for
  audio: 57 look filters (Cinematic, Film, Retro, Portrait, Food, Scenery…),
  23 effects (blur family, chroma/luma key, grain, halation, color wheels,
  curves…), 22 transitions, 25 audio packages (cleanup, spaces, tones,
  voices). Multi-pass shaders, 3D LUTs, `.cube` import, user packages
  hot-loaded and GPU-vetted, a 164-filter FFmpeg allowlist for safety.
- **Grading.** Colour wheels (lift/gamma/gain with pucks + master), curves
  per channel, stops-based exposure, LUT shelf, adjustment layers that treat
  everything beneath, per-clip or one-look-per-video.
- **Text.** rustybuzz shaping, bidi, 41 bundled faces + 6 script fallbacks
  (CJK, Arabic, Hebrew, Devanagari, Bengali, Thai), colour emoji (COLR v0 +
  bitmap strikes), stroke, shadow, plate with rounded corners, tracking,
  line height, wrapping, per-word reveal maps, 31 presets, user font import.
- **Masks & background removal.** 6 analytic shapes (linear, mirror, circle,
  rectangle, heart, star — all keyframable), painted brush strokes, and
  three auto models: MediaPipe selfie (compiled in — works with nothing
  downloaded), Robust Video Matting MobileNetV3 (person), IS-Net (object),
  plus SlimSAM smart brushes that track a region frame to frame. Masks
  analysed at 10 Hz and cached in the project.
- **Stabilization.** Shi-Tomasi corners + pyramidal Lucas-Kanane flow +
  RANSAC similarity + Gaussian path smoothing; tripod mode, max-zoom clamp,
  measured-zoom readout. Classical CV — no model, no download.
- **Enhance.** Real-ESRGAN realesr-general-x4v3 upscaling (1×/2×/4×),
  tiled, strength-blended against bicubic so skin never goes waxy, with a
  before/after split preview.
- **Rendering.** wgpu compositor in Rgba16Float scene-linear, one draw call
  per layer, 6 blend modes, HDR end to end (HLG/PQ in, BT.2390 tone-map or
  HDR pass-through out), texture budgets sized to the GPU (512/256/128 MB),
  CPU reference renderer as a test oracle, SSIM parity suite.
- **Proxies & decode.** Every file ≥1080p gets a working copy (H.264 or
  all-keyframe MJPEG; HDR sources keep 10-bit HEVC) built on background
  lanes and adopted for scrub; decode-level ladder halves the source until
  it covers the request; hardware decode via VideoToolbox/D3D11VA/MediaCodec
  (VAAPI opt-in), with 8-bit H.264 deliberately on the CPU where it is
  faster. SVG via resvg. Waveform and filmstrip caches.
- **Export.** H.264 / HEVC / AV1, 10-bit colour, HDR (BT.2020 HLG/PQ),
  x264/x265/SVT-AV1 and NVENC/QSV/AMF hardware, VBR/CBR/average rate
  control, limited/full range, AAC 48 kHz audio, faststart MP4, staged
  progress (cutout → stabilize → video → audio → mux) with cancel.
- **Small-machine tuning.** Every cache sizes itself to installed RAM
  (7 caches retuned for ≤6 GB: warm decoders 8→4, stills 24→8, title
  paintings quartered, export pool RAM-shared…), so a 4 GB laptop edits
  20-minute videos. See [docs/FUNCUT.md](FUNCUT.md).

## The libraries

Nothing here is bundled whole; indexes download once, media arrives per pick.

- **OpenVid stock footage — 937,957 clips** with captions and quality
  numbers (aesthetic, motion, temporal consistency, camera motion), searched
  locally (SQLite FTS5 BM25 with stop-word drop and concept fallback), frame
  sizes measured from each clip's own MP4 header, and a semantic second pass:
  a local MobileCLIP2-S0 model re-orders >10-match relevance pages by what
  the clips actually **show**. `openvid_search_batch` shops a whole plan's
  B-roll in one call. See [docs/STOCK_LICENSING.md](STOCK_LICENSING.md).
- **Music — 1,191 whole CC0 tracks**: FreePD (Kevin MacLeod and friends,
  1,025) and Moby's MobyGratis instrumentals as OpenLo-Fi (166) — a Lo-Fi
  track is one ranged read of exactly its span out of the 554 MB zip. See
  [docs/BGM.md](BGM.md).
- **Sounds — 430 CC0 effects** bundled in the repo (12 categories), each
  with a description, use-for lines and `syncAt` (where its peak lands),
  ranked 6-field search. See [docs/AUDIO_LIBRARY.md](AUDIO_LIBRARY.md).
- **Stickers — 149,758 transparent PNGs**: MAGICK (145,193 captioned
  AI-generated, CC BY-NC — personal use) and OpenMoji (4,565, CC-BY 4.0),
  with favourites. See [docs/MAGICK.md](MAGICK.md).
- **Templates.** Any project saves as a template (folder + assets + poster);
  slots are placeholder media a later `FillSlot` swaps for the user's file
  in place, across all timelines.
- **Fonts.** 41 bundled title faces, 6 script fallbacks, user import.

## Speech

- **Transcription.** Whisper (whisper.cpp built in) with six models from
  tiny.en (75 MB) to small (470 MB), word-level timestamps (tokens grouped
  into words, clamped to segments), language hints (ISO/BCP-47), progress
  and cancel. Runs as a job on the API as well as in the window.
- **Text-to-speech, three families.** Kokoro v1.0 (36 voices, 9 languages,
  int8 132 MB or full 350 MB), Pocket TTS (Kyutai 100M, 98 MB, with
  delivery-aware pacing) and Chatterbox Turbo (Resemble, ~1.1 GB, desktop).
  **Voice cloning** from ≤10 s of reference audio (Pocket) or ≥5 s
  (Chatterbox, RMS-normalized); clones save to a voices shelf.
- **Delivery presets** (neutral/conversational/urgent/excited/calm/awed/
  somber/authoritative) set rate and sentence-breath together; `speed`
  (0.5–2), `pauses` (0–1) and `steps` (1–16) override.
- **Voiceover recording.** In-window panel: live meter, input gain,
  count-in, teleprompter with scroll speeds, retake, and **one-switch
  silence removal** (−40 dBFS, ≥0.35 s pauses, applied as one ripple edit).

## The agent layer

- **The MCP server** (`concat --mcp`, [docs/MCP.md](MCP.md)): **87 tools**
  over stdio, both protocol eras (initialize handshake and per-request
  `_meta`), schemas generated from the API's own types so an agent cannot
  invent an argument. It reaches the engine through the open window
  (`window`) or runs the engine itself (`headless`); project locks keep the
  two from fighting. The catalogue by area:

| Area | Tools |
| --- | --- |
| Projects & reading | `concat_version`, `project_list/create/open/save/close/get`, `project_set_video`, `timeline_read`, `collision_check`, `plan_validate` |
| Media | `media_probe`, `media_import` |
| OpenVid stock | `openvid_status/prepare/search/search_batch/get/fetch/import` |
| Sounds | `sounds_search`, `sounds_add` |
| Music | `bgm_status/prepare/search/fetch/import` |
| Stickers | `magick_status/prepare/search/fetch/add` |
| Cutting | `clip_add`, `clips_add_sequence`, `clip_trim`, `clips_split`, `clip_cut_spans`, `clips_move`, `clips_remove`, `clip_update`, `clip_transform`, `clip_speed`, `clip_frame`, `clip_audio_levels_db`, `track_add`, `timeline_background` |
| Picture | `clip_mask`, `clip_stabilize` |
| Titles & captions | `title_add`, `captions_add`, `captions_animate` |
| Keyframes & effects | `keyframe_set`, `keyframe_clear`, `effects_list`, `effect_layer_add`, `effect_key_set` |
| Analysis (senses) | `analyze_scenes`, `analyze_beats`, `analyze_silence`, `analysis_wait` |
| Speech | `transcribe`, `tts_generate`, `models_list`, `models_download` |
| Batches & history | `edit_apply` (any command or atomic batch), `edit_undo`, `edit_redo` |
| Templates | `template_list`, `template_instantiate` |
| Motion design | `motion_presets`, `motion_inspect`, `motion_frame`, `motion_bake`, `motion_overlay` |
| Export | `export_start`, `export_status`, `export_wait`, `export_cancel` |
| Looking | `preview_frame` (an image the model can see) |
| Perception & the creative loop | `creative_brief`, `stock_contact_sheet`, `watch_render`, `craft_critique`, `suggest_cuts`, `measure_loudness`, `local_vlm`, `emotion` |

- **The JSON-RPC API** (Settings > Automation, or `concat-cli serve`): the
  same 48 request methods over TCP/Unix lines, plus gRPC with an events
  stream. Token-authenticated (constant-time compare), throttled,
  write-roots-restricted.
- **The CLI** (`concat-cli`): `probe`, `render`, `api`, `serve`, `preview`,
  `check` — headless rendering and effect-package validation.
- **The senses.** Analysis runs as jobs (start → id → wait → JSON output):
  scene cuts (PySceneDetect, adaptive/content/threshold detectors), beat
  grids (librosa: bpm, beats, downbeats, onsets in ms), dead air (ffmpeg
  silencedetect or librosa fallback), transcription, TTS and model
  downloads. `suggest_cuts` does the same rhythm work with **no Python
  extras** (ffmpeg-only BPM + beat-aligned cut times). Every script ships
  embedded and is materialized on first use; a missing wheel says the exact
  pip line instead of failing silently.
- **Perception.** An agent gets **images, not paths**: `preview_frame`
  (one composited frame), `stock_contact_sheet` (B-roll candidates as one
  labelled sheet, with per-candidate pixel metrics, a crop/energy-centroid
  hint and ready-to-use `clip_transform` offsets for 9:16 reframing, plus
  optional brief-ranked re-ordering), `watch_render` (the whole cut as one
  contact sheet + measured facts: black frames, letterbox bars, flat and
  duplicate shots, static transitions, an audio energy envelope, EBU R128
  loudness), `motion_frame` (one motion-comp frame).
- **The craft critic** (`craft_critique`): six 0–10 axes (hook, story,
  rhythm, shotChoice, design, sound) on three honest rungs — measured
  pixels only (confidence 0.5), a bundled 0.5B local VLM allowed to flag
  visible defects at most ±1 point (it was tested end-to-end and caught
  hallucinating), or a free-tier hosted vision API (groq/gemini/zai/
  openrouter, one `local_vlm {action:"configure"}` call) for full six-axis
  scoring (0.78). Every model answer is schema-validated with
  degenerate-answer rejection. Each pass journals to `craft-history.json`
  and reports pass/delta/trend. See [docs/VLM.md](VLM.md).
- **The emotion engine** (`emotion`, [docs/EMOTION.md](EMOTION.md)): reads
  the voiceover per line — 7 emotions with intensity, the act (hook/setup/
  tension/turn/reveal/proof/cta), a hormone shorthand, the TTS delivery +
  speed, the easing curve as exact concat-motion authoring JSON, colour
  temperature with a grade hint, the SFX register, the BGM duck depth in
  dB, pacing and 2–3 word stock queries — plus the whole piece's arc
  checked against the brief. Engines: a stem lexicon always (0.55), or
  j-hartmann's distilroberta on HuggingFace's free Inference API when keyed
  (65/35 blend, 0.8). Actions: `extract`, `brand` (validate + apply a brand
  guide), `gate` (the placement gate below), `verify` (re-transcribe the
  export and align words — what a render loses, duplicates or shifts is a
  defect), `template`.
- **The creative brief & the brand box** (`creative_brief`): the WHY of the
  edit persisted as `creative-brief.json` in the project — audience,
  platform, goal, thesis, curiosityGap, payoff, emotionalArc,
  referenceStyle, tone, durationSeconds, constraints, avoid, and
  **brandKit**: the channel's non-negotiables (id, name, vibe, palette
  hexes, type family/weight, motion easing policy, base grade + amount,
  SFX palette, BGM genres + duck, voice reference, **taboos**). The kit is
  validated (structure, hex colours), applied per act, auto-loaded by every
  creative tool given `project`, and enforced two ways: a taboo found in
  any planned query or caption is a **blocker** (replace the query, never
  the taboo), and the **placement gate** (`emotion {action:"gate"}`) checks
  every stock candidate against the beat's emotion register, act semantics
  and the brand's taboos — 0–1 failed checks passes, 2+ rejects the
  candidate, and if every candidate is rejected the answer is **drop the
  beat**, never pad with a bad match. `plan_validate` reports
  `creativeReady` and its blockers; the director prompt carries the kit.
- **Loudness proof** (`measure_loudness`): EBU R128 on the exported file —
  integrated LUFS, true peak, range, error against the platform target
  (−14 default, −23 broadcast) and specific mix notes.
- **Motion design** ([docs/MOTION.md](MOTION.md)): a comp is a JSON scene
  graph (groups, rects, ellipses, paths, text with per-word timing, HTML
  nodes, images, Lottie, nested comps; animated tracks on every numeric
  property; linear/steps/spring curves, springs solved analytically) baked
  by the **Geneva** sidecar — a browserless HTML/CSS renderer — to an RGBA
  PNG sequence, or drawn by the native engine with no sidecar. 12 presets
  (glass lower third, kinetic caption, stat triptych, wordmark sting, pull
  quote, chapter chip, subscribe card, terminal, bar chart, split compare,
  counting number, mesh backdrop) with slots an agent fills; 19 editing
  ops; `motion_overlay` composites a transparent bake over real footage
  (the honest alpha path); `captions_animate` turns a word-timed transcript
  into a kinetic caption video in one call; `concat-motion-agent` puts the
  same vocabulary on a CLI (`list/inspect/edit/frame/bake/reel/captions`).

## The director skill (the AI's brain)

`.claude/skills/funcut-director/` — how an agent directs a whole video
instead of randomly calling tools. **Phases 0–8 plus sub-phases 1b and 5b**:

0. **Creative direction** — `creative_brief` before any cutting tool; the
   returned `directorPrompt` stays in context.
1. **Understand the material** — measure everything: TTS with per-section
   delivery, transcription with measured word times, dead air, scene cuts,
   beat grids; `suggest_cuts` when Python extras are missing.
1b. **Read what the voiceover feels** — `emotion {action:"extract"}`; flat
   scripts are fixed before planning.
2. **Tighten the voice** — silence removal with the padding rule; transcript
   times retime by exactly what was cut.
3. **Write the edit plan** — one JSON contract
   (`templates/edit_plan.schema.json`, v2) with sections, per-segment
   emotion/act/easing/colourTemp/duckDb, visuals, cut types, entrances and
   exits, SFX with peak times, captions, one required grade; validated by
   `plan_validate` until **both** `valid` and `creativeReady` are true.
4. **Gather (parallel)** — batched library searches; candidates are SEEN
   (`stock_contact_sheet`) and gated (`emotion {action:"gate"}`).
5. **Place (sequential, batched)** — atomic `edit_apply` batches; J-cut
   lead, entrance/exit motion from the plan, SFX at peak time, BGM ducked
   per act.
5b. **Motion comps** — one designed beat per section (every number,
   comparison and process in finance/science/tech is a comp, never a text
   card); inspect → ops → frames → bake → overlay.
6. **Grade** — one look per video, proved at 25 % and 75 % of runtime.
7. **Watch the whole cut** — `watch_render` → `craft_critique`; at least one
   revision acted on and re-watched, or it is not done.
8. **Export and watch the file** — the exported file gets its own
   `watch_render` + `craft_critique` (≥7), `measure_loudness` against the
   platform target, and `emotion {action:"verify"}` proving the speech
   survived the render.

Backed by **13 hard floors** (motion is the driver, transition mechanics,
pacing bands by words-per-minute, variety laws, motion on every element,
grade not optional, frame fitting with the no-vertical-OpenVid fact, sound
variety, one text system, the memory journal, honest captions, loudness
targets, sticker licensing) and **9 hard rules** (never claim done before
the gates pass, never a segment without a visual, never invent ids, the
preview budget, blockers are blockers, one video one look, searches
parallel/placements sequential, surface remedies, job discipline) — and 18
prompt playbooks in `prompts/` (script analysis, edit plan, B-roll matching,
cuts and pacing, transitions, SFX design, motion comps, music and ducking,
text animation, colour grading, keyframe motion, 10 niche playbooks, reels
vs longform, memory, review, creative direction, the quality loop, the tool
reference).

## maestro (the judgment shelf)

`.claude/skills/maestro/` — vendored verbatim from leobbaroni/maestro 5.0.0:
**28 distilled reference modules** (design direction, motion principles
with numbers, sound design and beat-sync method, brand systems, design
audit, generative engines…) over **ten corpora** (impeccable, taste-skill,
genjutsu, gsap-skills, design-dna, motion-design-skill, higgsfield skills,
comfy-skills, video-shotcraft, and the author's own), including a verified
**157-card product-shot vocabulary** across 10 categories, a
twelve-upstream drift checker (`scripts/maestro/check-upstreams.mjs`), and
per-phase routing: Phase 0 → design-direction, Phase 3 → video-direction +
the 157 shots, Phase 5b → motion-principles, Phase 6 → design-foundations,
Phase 7 → design-audit, SFX/BGM → video-sound. The director's hard floors
always win; maestro refines how a floor is met, never lowers it. See
[docs/MAESTRO.md](MAESTRO.md).

## Forge (one click from script to film)

- **The Forge page** (library page 12 in the window): paste a script, pick a
  voice (5 Kokoro speakers), a niche (10) and a music mood (6), press Start
  — the external `forge` harness runs the funcut-director skill against
  this editor over MCP, its output streams into the page log, and its edits
  land on the open timeline as ordinary undoable commands. Seven provider
  API-key fields (Groq, Gemini, OpenRouter, Mistral, Cerebras, SambaNova,
  NVIDIA NIM) are stored in settings and injected as env vars for free-tier
  models.
- **The harness config** (`.forge/mcp.toml`): one stdio server (`concat
  --mcp auto`), a 900-second call timeout for patient work, servers
  allow-listed to `funcut`.
- Overnight queues (`forge queue`/`forge schedule`), a vision-capable
  director model picker (`FUNCUT_FORGE_MODEL`), and a research ledger of
  every claim. See [docs/forge/FORGE.md](forge/FORGE.md) and
  [ARCHITECTURE.md](forge/ARCHITECTURE.md).

## The window (UI tour)

- **One window, three shells** — desktop workspace, phone editor
  (`CONCAT_PHONE`), and the start screen (recents as poster cards, search,
  sort, drop-to-start).
- **A Blender-style dock**: 5 pane kinds (media, preview, inspector,
  timeline, keyframes), any pane in any seat, drag-to-swap seats,
  splitters, add/remove panels.
- **13 library pages**: Media (categories, sort, marquee select), Audio,
  OpenVid, Music, Stickers, **Forge**, Text (31 presets), Shapes,
  Transitions, Effects, Filters (audition a look by clicking its card),
  Adjustments (adjustment layers + LUT import), Templates. Favourites with
  hearts, per-shelf search.
- **The monitor**: transport, draggable timecode, quality ladder
  (full/half/quarter), output-ratio picker, fit/pan/zoom, on-stage
  transform with snap guides and rotation, mask overlay with grips, save
  frame as PNG.
- **The inspector**: per-kind tabs — transform, canvas/backdrop (blur,
  colour, stills), blend, stabilize (with measured zoom), background
  removal (auto person/object + smart brushes painted live), chroma key,
  masks, crop, **enhance** (AI restore with before/after split), the full
  grading panel (wheels, curves), audio (source, dB fader, fades, pitch
  preservation, the 25 audio packages), speed (curve editor with presets),
  the effect chain, text typography, transition parameters; batch title
  styling and batch volume.
- **The timeline**: tabs for sequences, the full tool tray (split, razor,
  delete left/right, merge, freeze/reverse/mirror/rotate, hand, snap,
  magnetic, captions, TTS, voiceover with live meter, preview axis, zoom,
  VU meters), per-lane lock/visibility/mute/height, filmstrips, waveforms,
  a draggable volume line with unity snap, fade ramps, fx chips, transition
  markers, keyframe diamonds on the ruler, drop ghosts, marquee select.
- **The keyframes pane**: dopesheet or curves view, per-key bezier handles,
  ease presets, step through keys.
- **Dialogs**: new project, export (resolution ladder, codecs with
  encodability notes, 10-bit, HDR, size estimate, progress, cancel),
  captions (script or transcribe; import/export SRT), speech (expression
  tags, voice pickers, quality, clone from a clip, save voices), voiceover
  (teleprompter, count-in, retake, silence removal), project settings,
  relink (one folder, relink all by basename), settings.
- **Settings, 8 pages**: General (language, log level, download source,
  magnetic, proxies on by default + codec, GPU), Appearance (dark/light, 6
  accents + custom, playhead colour, UI scale 100–200 %), Transcriber,
  Speech, **Automation** (the API server, token, MCP pointer), About (copy
  system info, show log), Version (update/rollback), Shortcuts.
- **28 rebindable keyboard actions** with conflict eviction and reset;
  26 languages with user-drop-in locale overrides; Wayland drag-and-drop;
  undo/redo everywhere; toasts; tooltips; context menus.

## Environment variables

`CONCAT_API_ADDRESS` / `CONCAT_API_TOKEN` (reach a server elsewhere) ·
`CONCAT_MCP_ROOTS` (extra write roots for headless agents) ·
`CONCAT_GENEVA_BIN` (the motion sidecar's path) · `CONCAT_PHONE` (phone UI) ·
`CONCAT_GPU=software` (software raster) · `FUNCUT_PYTHON` (the sense
interpreter) · `FUNCUT_FFMPEG` (ffmpeg override) · `FUNCUT_VLM_ENDPOINT` /
`FUNCUT_VLM_API_KEY` / `FUNCUT_VLM_MODEL` / `FUNCUT_VLM_STRONG` / `FUNCUT_VLM_DIR`
(any OpenAI-compatible vision critic; STRONG forces six-axis prompts) ·
`FUNCUT_EMOTION_API_KEY` / `HF_TOKEN` (hosted emotion classifier) ·
`FUNCUT_EMOTION_MODEL` · `FUNCUT_MOBILECLIP` (the rerank model dir) ·
`FUNCUT_FORGE_MODEL` / `FORGE_MODEL` (the Forge director's vision model) ·
`GROQ_API_KEY` / `GEMINI_API_KEY` / `ZAI_API_KEY` / `OPENROUTER_API_KEY`
(hosted vision providers).

## Models and licences

Nothing is bundled except the compiled-in MediaPipe selfie segmenter; every
download is SHA-256-verified against `models/manifest.toml` (mirror first,
upstream fallback): Whisper tiny.en→small (MIT, 75–470 MB each) · Kokoro
int8/multi-lang (Apache-2.0, 132/350 MB) · Pocket TTS (CC-BY-4.0, 98 MB) ·
Chatterbox Turbo (MIT, ~1.1 GB) · RVM person matting (GPL-3.0, 15 MB) ·
IS-Net object (Apache-2.0, 179 MB) · SlimSAM brushes (Apache-2.0, 40 MB) ·
Real-ESRGAN enhance (BSD-3, 4.9 MB). The optional VLM critic (SmolVLM2
0.5B ≈ 546 MB, or MiniCPM-V-2 ≈ 2.2 GB) and the MobileCLIP2 rerank model
fetch on demand into the app data dir. Stabilization is classical CV — no
model at all.

## Where the code is

| Layer | Path |
| --- | --- |
| Engine vocabulary (time, frames, masks, curves, timeline) | `src/crates/concat-core` |
| FFmpeg media (probe, decode, encode, audio, proxies) | `src/crates/concat-media` |
| GPU compositor + plans | `src/crates/concat-render` |
| The 127 effect packages + WGSL libraries | `src/crates/concat-effects` |
| Text shaping and painting | `src/crates/concat-text` |
| The document and its 52 commands | `src/crates/concat-project` |
| Vision models (cutout, brushes, enhance) | `src/crates/concat-vision` |
| Speech (whisper.cpp, three TTS families) | `src/crates/concat-speech` |
| Motion engine (Geneva, presets, authoring) | `src/crates/concat-motion` |
| The API (JSON-RPC methods, jobs) | `src/crates/concat-api` |
| The MCP server (87 tools, senses, emotion) | `src/crates/concat-mcp` |
| The socket server + CLI | `src/crates/concat-server`, `concat-cli` |
| The window (Slint UI, studio controller) | `src/crates/concat` |
| Session, proxies, models, templates, sounds | `src/crates/concat-host` |
| The skills | `.claude/skills/{funcut-director,maestro,motion,run-concat}` |
| The Forge wiring | `.forge/mcp.toml`, `docs/forge/`, the Forge page |
| Bundled sounds | `audio/` (430, CC0) |
| Model manifest | `models/manifest.toml` |
