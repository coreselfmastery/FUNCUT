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

The `bgm` tools search 1,191 music tracks on two shelves and
fetch only what is used — 1,025 whole-track CC0 (FreePD) and 166
MobyGratis (free for non-commercial use only). FreePD is the Internet Archive's mirror of Kevin
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

### Stickers (Fluent Emoji)

The `magick` tools search Microsoft's Fluent Emoji - 1,285 rendered 3D
transparent PNGs, **MIT licence**, free for commercial and personal use -
and fetch only what is used. The set's own CLDR names and keywords are
searched together ("smile" finds the grinning face by a word the name
alone does not say). Nothing is bundled: `magick_prepare` fetches the
index once (~135 KB; poll `magick_status` until it is `ready`), and a
picture's PNG arrives only when it is picked - one plain GET from
Microsoft's repository (jsDelivr behind it), checked to be a PNG that can
hold transparency before it is kept. A typical run: `magick_search` with
words ("party", `source: "fluent"` - the old names `magick`/`openmoji`
still answer) → `magick_add` with `id`, `start` and a placement
(`scale: 0.3, offsetX: 0.2`, `duration: 3`), which lays the picture
*over* the video - the first free lane above the highest occupied, never
beneath it - as one undoable step beside the clip's own. Every result
carries its `license` field, so "can this ship" is answered by the data. The window's Stickers page
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
- `transcribe` - Whisper's timed text: segments `{start,end,text}` and, inside
  each, the measured per-word times the kinetic captions and the SFX-peak
  placement read (docs claim the words; the engine measured them). The optional language hint is
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
is fifteen tools: ten thin wrappers over the `concat-motion` crate here (the same
vocabulary the `concat-motion-agent` CLI speaks), `geneva_explain` for the
engine's own diagnostic codes, `motion_farm_progress` for reading a farmed
render's progress live, `motion_farm_cloud` for the farm's legs on object
storage, `motion_import` for motion made elsewhere (Lottie and GSAP), plus
`motion_overlay`, the compositing path
over real footage, documented with the perception tools below:

- `motion_presets` - the eighteen motion-graphics presets (lower thirds,
  kinetic captions, stat cards, wordmark stings, pull quotes, chapter
  chips and cards, agendas, subscribe cards and end cards, terminals and
  diffs, bar charts and progress bars, split compares, counting numbers,
  mesh backdrops) with their slots, sizes and whether the Geneva engine
  is on the machine.
- `motion_inspect` - what is in a comp before it is touched: nodes, spans,
  animated properties, slots, validation problems; an optional `ops` batch
  is applied first, so an edit can be checked before it bakes.
- `motion_import` - motion made elsewhere, read here, nothing converting
  silently. `action: "lottie"` turns a bodymovin JSON (the After Effects
  export, the LottieFiles download) into a comp under the project's
  `comps/`; `action: "gsap"` applies a GSAP-style timeline to a comp the
  project already holds (or a preset) and writes the animated result as a
  new one. The reply carries the comp's summary and the adapter's notes -
  every skip, approximation and clamp in the adapter's own words, with
  `skipped` counted - because a conversion that does not say what it lost
  is a logo that comes back wrong.
- `scene_analysis` - the comp read as an engineer reads it, before a frame
  is drawn: the census (nodes by kind, animated against static, keyframes,
  curves, depth), the timing (coverage of the comp's span, the gaps),
  per-text legibility (size against the frame, WCAG-shaped contrast of the
  ink over the plate or background it draws over), the assets against the
  disk, and the lint's findings with their stable `L###` codes. No engine,
  no render, no write - the critique that happens before the pixels do.
- `motion_frame` - one frame of a comp at a time in seconds, drawn in tens
  of milliseconds, returned as an **inline image** (and also written under
  the project's `motion/`). Look at it before baking.
- `motion_preview` - the comp's motion watched: `frames` stills (1-12,
  six by default) spread evenly across the duration - never the dead last
  instant - drawn small (width 16-640, 480 by default) and returned as
  **inline images in time order** with each sample's time and file. The
  entrance, the hold and the exit at a glance, for a handful of frames
  instead of a bake.
- `motion_bake` - a comp (preset or file, optionally edited by `ops`)
  baked to PNG frames under `cache/comps/`, and with `encode: true`
  written as one video under `motion/` ready for `media_import` and
  `clip_add`. The encoded file is opaque - only the PNG sequence keeps a
  transparent comp's alpha.
- `captions_animate` - a measured word-timed transcript as one caption
  video: lines by the caption rules, each line a kinetic comp, baked and
  encoded in one call. This is the word-synced caption path; the
  timeline's own `captions_add` stays segment-timed. The canvas follows
  the `project`'s own frame when one is named (the way a bake's does);
  a caller that names neither gets the reel default, 1080x1920.
- `motion_reel` - a whole storyboard as one reel: several scenes (preset
  or comp file each, with `ops`, `at`, `hold`), **multi-layer scenes** -
  a scene's `layers` stack bottom first, plates under the comp and, with
  `above: true`, grain, vignettes, light leaks and foreground cards over
  it, each with its own `zoom`, `pan`, `offset`, `scale`, `fit`, `blend`,
  `opacity`, `fade_in`/`fade_out`, `start` and `hold`; differing drift
  between layers is what reads as depth - plus shared assets, audio beds
  and a curtain. A scene's `grade` is its colour grade inside the comp:
  one of the emotion read's temperatures (`cold`, `warm`, `cool`, `hot`,
  `sickly`, `split`, `balanced`), `amount` 0-1, tonal overrides and a
  brand `tint` - baked under the comp's own words as `backdrop-filter`
  plus a soft-light cast, the LUT approximation a comp carries, so the
  act is the grade: a cold hook, a warm payoff. A scene's `style` is
  the personality it wears over the layout - `calm`, `punchy` or
  `editorial`: the weight the type carries, the tracking, the easing
  hand, the tempo, and the colour when no `grade` is named. One layout,
  three moods. `plan: true` lays it out
  without rendering, `frame: seconds` returns one still as an inline
  image, and the render lands under `motion/` ready for `media_import`
  and `clip_add`. The same storyboard document the
  `concat-motion-agent reel` verb eats; every error names the scene it
  stopped at.
- `motion_probe` - what a media file IS, as the engine's own reader sees
  it: streams, frame size and rate, duration, the audio's channels - the
  same reader the renderer will use, asked **before** anything is built
  on the file. A file that is not there is refused outright; one that is
  not media comes back as the engine's own refusal. The look-before-you-
  build over the file itself, next to `motion_frame`'s over a comp.
- `scene_timeline` - the look-before-you-render of a whole storyboard:
  the same plan `motion_reel` lays, staged into a scratch, handed to the
  engine's own validator (`geneva validate --probe`, which **opens every
  asset** - lengths, loudness), and returned with the timeline the render
  would take, the engine's verdict on it and the plan's own warnings.
  Nothing renders, nothing is written; a board that does not validate is
  refused with the scene named and `geneva explain` already consulted
  for the first diagnostic's code.
- `motion_farm_progress` - what a long farmed render is doing right now,
  without waiting for it: a `concat-motion-agent render --farm` run writes
  every state it passes through (the plan, each part's start and landing
  or serving, the join, the done) as lines in the farm directory's
  `progress.jsonl`, and this reads them back live, from another process -
  each part's state, the frames finished against the frames planned, the
  percent, and an `etaSeconds` estimate from the pace of the parts drawn
  so far. A farm that has said nothing is a whole farm pending, not an
  error. Takes `farm` (absolute, or relative to `project`).
- `motion_farm_cloud` - the resumable farm on object storage: `push`
  uploads a farm directory and its staged render tree (files whose stored
  sha256 already matches are skipped, so a re-push costs what changed),
  `pull` downloads them back, `status` says what differs, and `work` is a
  worker's whole day - pull, render ONE missing part through the sidecar
  with the farm's own atomic part-and-marker shapes and the joiner's own
  key, push it back, repeat until it answers done. The bucket, the
  endpoint and the credentials come from the operator's environment
  (FUNCUT_FARM_BUCKET, FUNCUT_FARM_ENDPOINT, AWS_ACCESS_KEY_ID), never
  from the arguments - the same posture the VLM endpoint allow-list
  takes. Everything streams a block at a time; every request is
  SigV4-signed.
- `geneva_explain` - what a diagnostic code means, in the engine's own
  words: `code` is the code itself (E103, W210) or a refusal sentence
  pasted whole, its first code-shaped token picked out, and the reply is
  the sentence that says what the engine meant and how to fix it. The
  refusals the motion tools return have already consulted it for their
  first diagnostic; this is the door for the second and third codes a
  refusal carries, and for a code the engine printed anywhere else.

## Perception and the creative loop (nine more tools)

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
- `track_subject` - the multi-frame answer to `cropHint`: sample up to 24
  frames across `path` (or a `from`/`to` span), find the subject in each,
  and return a smoothed crop path as keyframes with the exact
  `clip_transform` offsetX/offsetY for `aspect` (9:16 default). Held
  through occlusion and exits, entered from centre when the subject
  arrives late; `moved: false` means a static subject - the single
  reframeHint is enough. Key the offsets with `keyframe_set` so a
  vertical crop follows the subject instead of centre-cropping it.
  Optional `samples`, `from`, `to`, `times`, `minConfidence`, `aspect`.
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
- `voice_signals` - the acoustic read of the voiceover, the other half of
  the driver (see [docs/VOICE-SIGNALS.md](VOICE-SIGNALS.md) for the full
  map). `action "extract"` takes the voiceover `audio`, the `transcript`
  (for the word timings) and optionally the `emotion` extract (for the
  arc), and returns per line: momentary **loudness relative to the take**,
  the **speech rate** with a pace class, the **sentence type**
  cross-checked against the pitch's terminal rise, the **emphasis words**
  the energy peaks land on, **fillers**, per-word **confidence** (a
  measured gate — or WhisperX scores when installed), the **speaker**, and
  the **entities** the line names with the artefact each suggests
  (`$10,450` → stat card, a name → lower-third, a date → timeline, PII →
  redact); plus the **pauses CLASSIFIED** (breath / beat / dramatic hold /
  scene turn / dead air — a hold is never cut), **motifs**, the take's
  **rhythm layers**, the **noise floor** (the BGM decision), the
  **diarization** with change times, and `directorHints` — the cut style,
  the caption gate, the protected pauses, and the font and music-genre
  hints for the detected script. Engines: ffmpeg ebur128 + silencedetect
  and pure-Python regex always; the Rust `voice-signals` binary
  (`cargo build -p voice-signals`) for prosody, forensics, speakers and
  NER — an honest note when it is not built; WhisperX (pip) for aligned
  words with scores. The reading also answers `signalConfidence` (one
  confidence and basis per signal family - lean less on a low one) and
  `decisionPolicy` (the table naming which signals drive each cut,
  caption, graphic, BGM and transition decision); the plan's
  `signalLog`, checked by `plan_validate`, records which signals drove
  each choice, and a plan with a voiceover and no signalLog is not
  creativeReady. `action "selftest"` proves the whole chain on a
  synthetic take.

**The consumption layer** — thirty-three readings behind five family
tools, what the director DOES with the readings. One tool per family:
`action` names the reading, the family's other arguments are the inputs
it reads (`transcript` / `lines` / `script`, the acoustic ones also
`audio`), `selftest: true` runs the reading's own proof, and every
family shares the voiceover tools' honest degradation contract. The
thirty-three readings behind the five doors:

*The script as language* — `sense_discourse` (`discourse`):

- `analyze_discourse` - connectives by relation (comparison / contingency /
  temporal / expansion / signposting), given-new overlap between lines,
  `abruptTransition` flags where a chapter card or scene change earns its
  keep, and the cohesion profile. A "but" is the emotional pivot.
- `segment_topics` - TextTiling-lite: segments with keywords, a headline
  and cohesion; each boundary is a scene-change point, each segment's
  keywords are its b-roll shot list.
- `detect_speech_acts` - the surface act per line: question, imperative,
  promise, warning, quote, enumeration — with the visual each earns.
- `classify_speech_acts` - Searle's five (assertive / directive /
  commissive / expressive / declaration) with the visual treatment per
  class and the "all assertives = a lecture" balance check.
- `score_readability` - Flesch, Flesch-Kincaid, Gunning Fog, passive voice,
  nominalizations, and the caption CPS budget the grade carries.
- `extract_claim_graph` - claims, evidence, hedges and the edges between
  them; `uncoveredClaims` is the proof-b-roll punchlist.
- `score_script_quality` - hook / structure / clarity / rhythm / CTA /
  concision scored 0-100 with the fixes named.
- `detect_sarcasm` - marker cues, sentiment contrast, and the flat-delivery
  cross-check on positive words when audio is given. A heuristic that
  names its cues, never a verdict.
- `detect_code_switching` - per-line Unicode-script scan with switch
  points, romanized borrowings, and the fonts / voices a mixed take needs.
- `detect_cross_lingual_emotion` - the offline emotion read for Bengali
  (native and romanized), Hindi, Spanish and French, folded onto the
  edit's seven — the fallback when there is no emotion-API key and the
  script is not English.

*The take's health* — `sense_voice_quality` (`voiceq`):

- `detect_vocal_events` - laughs (quaver pulse trains), sighs, breaths,
  coughs and throat clears in the envelope, cross-checked against written
  cues; coughs are cut candidates, laughs are authentic delivery.
- `classify_voice_quality` - resonant / breathy / tense / creaky from
  jitter, shimmer, voicing strength and spectral tilt — named proxies,
  plus the genre fit.
- `classify_recording_quality` - noise floor, SNR, clipping runs, hum at
  50/60 Hz + harmonics, DC offset, bandwidth → broadcast / clean / home /
  rough with its fixes and the BGM decision.
- `measure_reverb` - RT60 by Schroeder backward integration over onset
  tails, early/late ratio, the room class, gate and ADR hints.
- `detect_restarts` - stalling repeats, elongations, fragments, abandoned
  clauses — each with its span and cutSafe verdict; the punch-up tool.
- `classify_filler_type` - filled pauses vs discourse markers vs hedges vs
  intensifiers vs backchannels: what each costs the cut, per instance,
  with `safeRemovalSeconds`.

*The delivery* — `sense_prosody_style` (`prosody`):

- `score_emphasis_type` - every emphasis word typed (contrastive / new
  information / list / intensity / written) with the visual it earns;
  contrastive emphasis IS the edit point.
- `detect_speaking_style` - conversational / presenter / newsread / podcast
  / documentary-narration / high-energy from rate, pauses, pitch spread
  and dynamics — with the cut style each implies.
- `measure_rhythm_meter` - syllable-nuclei timing, the PVI (stress-timed
  vs syllable-timed), stress regularity, and the beatSync map when `bpm`
  is given: which words land on the downbeat.
- `detect_tempo_changes` - accelerando / rallentando / plateau segments of
  the speech rate — cut ON the trend changes.
- `detect_accent` - an L2-likeness proxy for caption pacing and mix
  clarity ONLY (the note says it plainly: accents are normal speech; this
  is not a verdict on the speaker).
- `measure_vocal_tract` - formants (LPC ear when built, band proxies
  otherwise), vowel space, tract length, timbre descriptors, and the
  voiceMatch tolerances for multi-narrator edits.

*The clean and the music* — `sense_audio_repair` (`audiorepair`;
enhance / remove / normalize WRITE a new file; the input is never touched):

- `enhance_speech` - the measured chain (highpass, de-esser, compressor at
  the take's own level, gate over the measured floor, limiter, loudnorm)
  with every decision's why and the before/after table.
- `remove_hiss_hum_rumble` - finds the hum (50/60 Hz families) and notches
  it, cuts the rumble, denoises the hiss at measured strength — and says
  so when the take is cleaner than the cure.
- `normalize_loudness` - two-pass ebur128 loudnorm to the platform target
  (streaming -14, podcast -16, EBU -23, ATSC -24), linear when possible,
  with the honest after-measurement.
- `detect_music_segments` - music / speech / silence / mixed per span, and
  the editing answer: never double a bed over existing music.
- `identify_music` - tempo, key and mode (Krumhansl chroma), balance,
  mood tags, and `libraryQuery` — the descriptor string that IS the
  bgm_search query. Naming the exact song needs an online fingerprint
  service and it says so.
- `analyze_music_structure` - BPM, the bar grid, sections with roles, the
  drops, the energy curve and `cutSafePoints`: where every cut lands
  musical.

*The words and the take, made to agree* — `sense_captions` (`captions`):

- `evaluate_tts_quality` - is this TTS take alive: pitch range and
  variability, word-gap regularity, pause placement against punctuation,
  word dynamics → naturalness score with the fixes.
- `detect_ai_voice` - the disclosure heuristic: too-clean floor, no
  breaths, near-zero jitter, machine-regular gaps → an aiLikeness band.
  It informs disclosure; it never proves anything about a person.
- `seal_voice` - `seal` / `verify` / `status`: SHA-256 + voiceprint +
  usage policy in voice-seal.json — the provenance and permission record
  any edit to the audio breaks, by design.
- `align_captions` - drift correction against energy onsets, cue building
  under reading-speed budgets, boundaries snapped to silence; the cues fit
  `captions_add` directly.
- `clean_subtitles` - punctuation, casing, filler stripping, merge/split
  under CPL and CPS budgets, overlap and gap repairs, with the changeLog;
  SRT in, cues or SRT out.

The intended loop: `creative_brief` -> `emotion` (extract, on the
voiceover) -> `voice_signals` (the acoustic score, with the emotion read
for the arc) -> the consumption layer where the edit needs it
(`score_script_quality` before the read, `classify_recording_quality` +
`enhance_speech`/`normalize_loudness` on a rough take,
`detect_restarts` + `classify_filler_type` for the punch-up pass,
`analyze_music_structure` + `measure_rhythm_meter`'s beatSync when cutting
to music, `align_captions` + `clean_subtitles` before burning captions)
-> `plan_validate` (creativeReady true) -> `stock_contact_sheet`
-> `emotion` (gate, before anything is placed) -> build -> `watch_render` ->
`craft_critique` -> revise -> `watch_render` again -> export ->
`normalize_loudness` on the shipped file -> `watch_render {path}` +
`measure_loudness` + `emotion` (verify) on the file that shipped. See
`src/crates/concat-mcp/sense/README.md` and the
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
| Reading and plans | `timeline_read` (compact: lanes, clips in time order, media, ids), `timeline_seek` (scrubs the open window's playhead - window mode only; headless refuses), `collision_check` (every place two clips share a lane; `clean` when none), `project_get` (everything), `concat_version`, `plan_validate` (checks a director plan before edits) |
| Media | `media_probe`, `media_import` |
| Stock clips | `openvid_status`, `openvid_prepare`, `openvid_search`, `openvid_search_batch` (several queries, one call, the CLIP rerank riding along per query unless `rerank: false`), `openvid_get`, `openvid_fetch`, `openvid_import` |
| Footage planning | `openvid_rank_for_slot` (rank a page against a slot's rich description: the CLIP second pass, the queries blended in at `tag_weight`, used ids and clips under the motion floor never returned, a hero slot asking for thirty), `footage_continuity` (the filled manifest read the way an editor scrubs - colour jumps, reversed travels, repeated scales, one-category runs, clips under their slot's motion floor - each flag naming its slot, rule and remedy) |
| Sound effects | `sounds_search`, `sounds_add` |
| Music | `bgm_status`, `bgm_prepare`, `bgm_search`, `bgm_fetch`, `bgm_import` |
| Stickers | `magick_status`, `magick_prepare`, `magick_search`, `magick_fetch`, `magick_add` |
| Editing | `clip_add`, `clips_add_sequence`, `clip_trim`, `clips_split`, `clip_cut_spans`, `clips_move`, `clips_remove`, `clip_update`, `clip_transform`, `clip_speed`, `clip_frame`, `clip_audio_levels_db`, `track_add` |
| Picture | `clip_mask`, `clip_stabilize`, `timeline_background` |
| Titles and captions | `title_add`, `title_style_words` (some words in another colour, weight, size...), `title_clear_word_styles`, `captions_add` |
| Keyframes and effects | `keyframe_set`, `keyframe_clear`, `effects_list`, `effect_layer_add`, `effect_key_set` |
| Analysis | `analyze_scenes`, `analyze_beats`, `analyze_silence`, `analysis_wait` (results as JSON in the status's `output`) |
| Speech | `transcribe`, `tts_generate`, `models_list`, `models_download` |
| Anything else | `edit_apply` (any edit command, or a batch of them as one undo step), `edit_undo`, `edit_redo` |
| Templates | `template_list`, `template_instantiate` |
| Motion design | `motion_presets`, `motion_inspect`, `motion_import`, `scene_analysis`, `motion_frame`, `motion_preview`, `motion_bake`, `motion_overlay`, `captions_animate`, `motion_reel`, `motion_probe`, `scene_timeline`, `motion_farm_cloud` |
| Export | `export_start`, `export_status`, `export_wait` (with progress), `export_cancel` |
| Looking | `preview_frame` (the composited frame as an image) |
| Perception and the creative loop | `creative_brief`, `stock_contact_sheet`, `watch_render`, `craft_critique`, `suggest_cuts`, `measure_loudness`, `track_subject`, `local_vlm`, `emotion` |

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

Three switches - two off by default for a server started bare, and the allowlist already on for the server this repository's own `.forge/mcp.toml` declares:

- **`--mcp-read-only`** (binary) or **`CONCAT_MCP_READ_ONLY=1`** (the app's `concat --mcp` and anything else that starts it): every tool that changes something is refused by name - an edit, an export, a stock import, the reel's render half. The reads stay, and `motion_reel` keeps its look halves (`plan`, `frame`, `sheet`): they change nothing outside the project's cache, and taking them away takes away the only way to see a design before committing to it.
- **`--mcp-allow a,b,c|all`** (binary), **`--full-tools`** (binary) or **`CONCAT_MCP_ALLOW=a,b,c|all`**: the only tools this server serves - in `tools/list` and at `tools/call` alike. A tool not on the list is neither listed nor called, as if it were not there: the surface served is what the operator said, not what the build happens to carry. An operator who sets no list gets the **director's default** - 99 of the 109 tools, the creative loop the funcut-director skill documents plus its remedies (the library prepares, the model downloads) and the five sense family doors, the footage layer's two included (`openvid_rank_for_slot`, `footage_continuity`) and the playhead (`timeline_seek`); the ten left off are the ones a director never calls by surprise (`project_close`, `export_cancel`, the farm pair, the specialist motion reads, `track_subject`, `geneva_explain`). `CONCAT_MCP_ALLOW=all` (or `--full-tools`, or `--mcp-allow all`) serves the whole catalogue. `.forge/mcp.toml` starts the repo's own server with the director's list already set (its `[servers.transport.env]` table), and carries both that list and the whole-catalogue list as comments, ready to paste for a server started by hand.
- **Per-tool timeouts**: every tool carries its own limit (`Tool::timeout_secs` - the VLM download and the overlay encode 1200 s, transcribe 900 s, exports and the sense family 600 s, the renders and watches 300 s, everything else 60 s), and a call that outlives it is cancelled and answered for at the deadline, with the limit in the message. The client-side `call_timeout_secs` stays as the ceiling over all of them; the engine's own calls are bounded a second time, per call, by the Geneva guard (see `docs/MOTION.md`).

## The reel contact sheet

`motion_reel` with `sheet: true` renders four stills of the reel - a quarter in, half, three quarters, near the end - tiles them into one image, and returns it inline **and** as `pngPath` under the project's `cache/sense/`, which is the path `craft_critique` takes. The creative loop over a reel is then: `motion_reel {sheet: true}` → `craft_critique {sheet: "<pngPath>", brief}` (the bare path string, or the whole reading object - both are accepted) → revise what scores under 7 → again.
