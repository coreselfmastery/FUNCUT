# FUNCUT versus Concat: what changed, and what it cost

This is the honest ledger of this fork against the upstream it stands on
([jub0t/Concat](https://github.com/jub0t/Concat), the commit this fork's `main`
is built from). Every number here is either read straight out of the code or
marked as an estimate; nothing is a benchmark this fork ran on your machine,
because the point of the changes is that the *machine decides* now.

Three kinds of change live here: **memory sizing** (the caches that had one
fixed size now scale to the machine) and the new features - the OpenVid
stock library, the background-music library, pause removal for
voiceovers, the built-in CC0 sound library, the Wyzowl sound effects
that grew it to 430 and the designed Hollywood pack that grew it to 530,
the hundred viral transitions and the hundred viral effects that grew the
package count to 327, the sticker library, the hosted free-tier vision
critic with a stronger measured baseline, the analysis and speech layer
on the API, the perception tools, the maestro judgment skill and the clip-skills editing-craft skill for
the AI director, the emotion-driven edit - the voiceover read that now
drives every layer - the funcut-director skill, the motion layer and its
skill, and the Forge integration. The complete verified inventory of what this
fork ships is [`docs/FEATURES.md`](FEATURES.md); section 10a of this
ledger maps the capability groups that grew up around the emotion work.
Everything else in the
program is upstream's own, including
its existing small-machine machinery: proxies for large files, preview quality
tiers, hardware decode where it wins, and the swap-friendly design of the
playback cache.

## 1. Memory: fixed sizes versus the machine's own

Upstream's frame cache was already a share of installed RAM (a sixteenth,
clamped 256 MB to 1 GB). Every other cache that matters on a small machine
was a constant, tuned on workstations. This fork makes them all answer the
same question. A "small machine" is **6 GB of RAM or less** - the class of a
budget laptop - and nothing changes at all on a machine with 8 GB or more.

| Cache | Where | Upstream (raw) | FUNCUT on a small machine | On 8 GB+ |
| --- | --- | --- | --- | --- |
| Frame cache (decoded pictures) | `concat-media`, `ReaderPool::frame_budget` | 1/16 of RAM, 256 MB - 1 GB | **unchanged** (this was already right; on 4 GB it means 256 MB) | unchanged |
| Warm decoders kept for scrubbing | `concat-media`, `ReaderPool::readers_for` | 8 open decoders | **4** - a decoder holds tens of MB of its own buffers; the 5th reopens at the price of one seek | 8 |
| In-memory stills (title drags) | `concat-media`, `stills_for` | fixed 24 frames | **8** (rides the frame budget) | 16-24, by the budget |
| Title paintings (live redraw) | `concat-host`, `live_budgets` | 128 MB budget, 512 MB ceiling | **~42 MB / ~170 MB** | unchanged |
| Decoded audio spans kept mapped | `concat-host`, `memory_cache_cap` | 64 spans | **16** (they are file-backed; the kernel can evict either way) | unchanged |
| The export's own frame pool | `concat-export` | fixed 512 MB beside the encoder | **a share of RAM** like the editor's own pool | now RAM-sized too (1 GB at 16 GB+) |
| The CLI renderer's pool | `concat-cli` | fixed 512 MB | **a share of RAM** | as above |

What that means on a 4 GB laptop cutting a 20-minute YouTube video:

- The editor's caches now add up to roughly **350-450 MB** where upstream's
  fixed sizes added up to **900 MB - 1.2 GB** - beside the encoder, the GPU
  driver's own share and the desktop itself, all competing for the same 4 GB.
- The export path was the sharpest edge: upstream opened a *second*, fixed
  512 MB frame pool on top of the encoder and the compositor during an
  export. On this fork the export pool is 256 MB (the machine's share), the
  same as the editor's.
- The idle-decoder sweep (close after 10 s unused) is upstream's own and
  stays; the fork only halves how many sit warm in the first place.

What it costs on a small machine: a fifth file scrubbed back and forth
alongside four others pays one seek per jump; a ninth title dragged at once
is painted again instead of held; undo history across a long voiceover
session re-decodes a little more often. None of these are visible in normal
editing - they are the difference between *held longer* and *held at all*.

Where the detection lives: `concat-media`'s `memory` module (upstream had it
in `concat-host`; the fork moved it down one crate so the export and the
command-line renderer - which hold pools of their own - can size them for
the same machine). The line itself is `total <= 6 GiB`, tested from both
sides.

## 2. The feature: the OpenVid stock library

The library pane grew a fourth stock page beside Media and Audio:
**OpenVid** - 937,957 stock clips from the OpenVid-1M dataset on Hugging
Face (the Pixabay, Pexels and CelebV shelves cut to 512x512, and the WebVid
shelf of 720p and 1080p landscape footage, each clip with a caption and
quality scores beside it), searched in a local index, fetched one clip at
a time. The same library is scriptable: seven `openvid.*` tools on the MCP
server and JSON-RPC API (the seventh, `openvid_search_batch`, shops a
whole plan's B-roll list in one round trip), so an agent can search by
caption and quality
numbers, fetch and place clips without a window.

**How it stays small.** The dataset is 7.5 TB; what this machine holds is
one SQLite index of 435 MB (six hundredths of one percent), downloaded
once from this repository's releases and checked against a digest. Every
search - captions, quality floors, camera motion, duration, shelf, frame
shape - runs against that index in milliseconds and costs no network.
The index knows each clip's byte range inside the dataset's data files,
and Hugging Face's CDN honours a Range header, so fetching a clip is one
ranged GET of the clip's own size. A clip lands in `<data>/openvid/clips`
and is copied into the project's own `openvid` folder when it is laid on
a timeline, so a project owns its files the way a sound's copy does.

**What it costs on a small machine.** The index connection keeps
SQLite's default two-megabyte page cache; nothing about the library is
resident beyond that and the page of results on screen. The captions in
the index are zstd-compressed against a dictionary the index carries, and
a page of forty decompresses in well under a millisecond. The one-time
download is the only large moment, and it resumes if it is interrupted.

**The honest parts.** The dataset's metadata carries no frame sizes. What
the clips' own MP4 headers say, once read: the Pixabay, Pexels and CelebV
shelves are 512x512 squares (cut for text-to-video training) and the WebVid
shelf is 720p and 1080p landscape. A clip's frame size is read from its
own header the first time it is fetched - the end of the file its shelf
puts its `moov` box on, so one small read - and kept in a side table. The
first shape filter also takes a one-time census: a sixteen-clip sample of
every shelf, measured the same way in parallel and remembered, whose
unanimous verdict widens the filter to that shelf's unmeasured clips - so
Square finds the three square shelves at once and Horizontal the WebVid
shelf, without measuring 938,000 files - while a clip its own header has
spoken for is never overruled by its shelf's sample. Nothing vertical has
been measured in this library; the vertical filter says so in words
rather than returning everything quietly, and the page's note tells
someone filtering for vertical footage how a square clip still sits on a
vertical timeline. Search words are matched over the first 300 characters
of each caption - the scene descriptions - and everything past that is
one `openvid_get` away.

## 3. The feature: the background-music library

The library pane grew a fifth page beside Media, Audio and OpenVid:
**Music** - 1,191 whole CC0 background tracks on two shelves, searched
locally, fetched one track at a time. FreePD is the Internet Archive's
mirror of Kevin MacLeod's CC0 library (~1,000 tracks, grouped the way
freepd.com grouped them); Lo-Fi is Moby's MobyGratis instrumentals as the
OpenLo-Fi pack (166 tracks, 10 categories). Both shelves are scriptable:
five `bgm.*` tools on the MCP server and JSON-RPC API, so a person and an
agent see one library.

**How it stays small.** Nothing is bundled - not in the app, not in this
repository. A shelf's catalog arrives once (the Archive item's own file
listing, ~1 MB; the pack's own manifest, ~30 KB) and is kept, and a
track's audio arrives only when the track is picked, into `<data>/bgm`.
The Lo-Fi pack's whole audio sits in one 554 MB zip on a GitHub release,
and that zip honours byte ranges - so the library reads the zip's own
central directory once (~80 KB), measures every member's place and length
from its own headers (one ~4 KB ranged read a track, five at a time,
kept), and fetches a track as **one ranged read of exactly that track's
span**, inflated and checked against the CRC the zip itself carries. The
other 550 MB never moves; a small machine's disk never holds it either.

**What it costs on a small machine.** The catalogs are JSON held as one
`Vec` per shelf - a page of results is forty tracks - and nothing about
the library is resident beyond that and the tracks this machine has
actually played. The FreePD listing is 1 MB on the network once; the
Lo-Fi census is ~700 KB of ranged reads once; every fetch after that is
a single track's own size.

**The honest parts.** The zip and the manifest are pinned to the same
release tag, and a kept zip index is believed only while the live zip
still says the same total size - a zip that disagrees is read again
rather than misread. A Lo-Fi track's playing time is measured from its
own first MPEG frames (a Xing header's frame count exactly, a plain
frame's bitrate as the estimate a CBR track deserves); a track whose
length could not be measured is passed by a duration filter rather than
guessed at, and its card says "?" rather than a number. Nothing on
either shelf pays a licence: both are CC0 1.0, each card names its
artist, and `THIRD_PARTY_NOTICES.md` says where every byte comes from.

## 4. The feature: pause removal for voiceovers

Upstream records voiceovers well - count-in, teleprompter, retake, landing
the take on a lane under the picture - but cutting the pauses out of a take
was a manual business of scrubbing and splitting. This fork adds one button.

**Where it is:** the voiceover panel, under the transport, enabled once a
take has landed ("Remove silence" / your language's words for it).

**What it does, end to end:**

1. The take's file is streamed through FFmpeg - mono, 16 kHz - and folded
   into 20 ms windows, holding only one window at a time (the waveform
   module's own doctrine: an hour-long take costs the same memory as a
   jingle). A window is *quiet* when its loudest sample is under -40 dBFS.
2. A run of quiet windows **0.35 s or longer** is a pause; shorter gaps are
   the rhythm of speech and stay. Each pause is padded 60 ms a side so word
   onsets are not clipped, and padded pauses that touch merge.
3. Two guards keep a failed take from being edited instead of retaken: a
   take with nothing loud in it yields no pauses, and so does one whose
   pauses would leave under a fifth of a second of speech.
4. The pauses are mapped through the clip's in-point and speed onto the
   timeline and applied as **one** new edit command, `removeSpans` - one
   undo step, scriptable over the JSON-RPC API like every other command.
   The lane closes up behind the cuts (the app's own ripple semantics:
   other lanes keep their timing).

**How it is built** - the part that makes it robust rather than a bolt-on:

- The measurement (`concat-media`'s `silence` module) is pure past its
  input: six unit tests cover the finding, including one that writes a real
  WAV and reads it back through FFmpeg.
- The edit (`concat-project`'s `Command::RemoveSpans`) is a first-class
  command in the engine, alongside Split and Freeze Frame: spans are
  clamped into the clip, overlaps merge, a kept piece too short to stand
  alone folds into the pause beside it, the first piece keeps the clip's
  id and every look, later pieces are minted carrying the same looks, the
  way in stays with the first piece and the way out with the last, and
  keyframes are re-windowed per piece the way a split re-windows them.
  Eleven engine tests cover it: rippling, non-rippling, head/tail pauses,
  speed mapping, fade sharing, whole-clip coverage, no-ops, sliver
  folding, unsorted overlapping spans, old-document parsing, and NaN
  refusal.
- The window thread is never blocked: the measurement runs on the project
  thread pool and comes back as a message, the same shape as every other
  long thing the panel does.

**What it deliberately does not do:** it does not touch clips other than
the last take (select a clip, use the API's `removeSpans` if you want it
generally); it does not close other lanes' gaps; and it does not analyse
anything but a constant-speed clip - a clip with a speed curve is not one a
pause's place can be mapped onto, so the button waits until that is not the
case.

## 5. The feature: the built-in sound library

Upstream's Audio page has a Mine shelf and a design for a shelf of Concat's
own sounds, held back on "coming soon" because the assets repository it
reads has no manifest published. This fork ships the library itself.

**Where it is:** the Audio page's sidebar, "Built-in sounds" beside "My
sounds", with a search field, a category filter and a refresh button. The
MCP server grows two tools beside it: `sounds_search` and `sounds_add`.

**What it is:** 110 sounds, all CC0, in `audio/` at the root of this
repository - whooshes and swishes for transitions, impacts and cinematic
booms, risers and sweeps, ambiences (rain, wind, parks, dungeons), short
music beds, and UI clicks and digital pops. 7.1 MB in all; nothing ships
inside the app or loads at startup. The sources are Kenney's audio packs
and CC0 packs from OpenGameArt, each named in the manifest with the page a
reviewer can check the licence on, plus ten risers and sweeps generated
for this repository and dedicated to CC0. Every file was trimmed of head
silence, normalised to −1 dBFS and encoded Ogg Vorbis q5 at 44.1 kHz; every
entry carries its SHA-256, and the app checks a download against it and
deletes anything that does not match.

**How it reaches the app:** the manifest URL in `concat-host`'s `models.rs`
now points at this fork (`raw.githubusercontent.com/coreselfmastery/FUNCUT`)
instead of the unpublished upstream assets repository; each sound is
fetched on first use from the `audio-v1` release mirror, then the repository
itself, checked against its digest, and kept in the app's cache. The shelf
is upstream's own design all the way down - it was built and wired, only
unpublished - so the fork adds the UI it was missing (the sidebar row, the
search field, the category filter, the load and retry buttons) and a sixth
category, UI, with its word in all twenty-six locales.

**Search, both ways:** the shelf's field narrows the list as you type -
words ANDed and prefix-matched against each sound's name, id, category and
tags, the same rule the OpenVid search uses on captions - and the API's
`sounds.search` narrows by the same fields for agents, returning id, name,
category, tags, duration, bytes and whether the file is already local.
`sounds.add` downloads a sound if needed, copies it into the project's own
`audio` folder, imports it into the bin and - with a `start` - lays it on a
lane as one undoable step, exactly as the OpenVid import does for clips.

**What it deliberately does not do:** nothing is bundled into the binary
or fetched at startup; the shelf loads only when asked for, as upstream
designed. Only CC0 is offered - the shelf's own rule, and the licence every
curated pack actually carries - so nothing from the paid-for or
attribution-required libraries can appear on it; those still belong on the
Mine shelf, imported by hand, where they stay the user's own files.

## 6. The Intel graphics question, answered honestly

A Celeron's integrated GPU (Intel UHD/HD Graphics) is already used by this
program exactly where it helps, and deliberately not where it does not:

- **Drawing the preview and the export: yes, always.** The compositor runs
  on wgpu over Vulkan, and the Celeron's Intel driver (ANV) is a Vulkan
  driver. Every frame of the monitor and of an export is drawn on the iGPU
  in both upstream and this fork - nothing to enable.
- **Export encoding in HEVC or AV1: yes, automatically.** The encoder list
  tries Intel Quick Sync (`hevc_qsv`, `av1_qsv`) first when the export asks
  for hardware - many times faster than x265 on two weak cores, which is
  the difference between an export that sips CPU and one that owns the
  machine for an hour. This is upstream behaviour and stays.
- **Export encoding in H.264: on the CPU, deliberately.** x264 makes a
  better picture per bit than the hardware H.264 encoders, and H.264 is
  the compatibility choice; the codebase measured this and chose quality.
  On a Celeron that means a 20-minute 1080p H.264 export is slow - the
  honest advice is to export HEVC, which the chip does for nothing.
- **Decoding: on the CPU, deliberately.** The engine links VAAPI (the
  Linux decode API your iGPU speaks) and can be pointed at it, but two
  facts keep it off by default: upstream measured 8-bit H.264 - the format
  of essentially all YouTube footage - decoding *faster* on the CPU than
  through the iGPU-plus-copy-back path; and on Intel i915 drivers VAAPI
  has hung whole machines until reboot (upstream issue #70). A hard freeze
  is the one thing a "no compromises on this potato" build must not ship.
  Heavier codecs (HEVC, 10-bit) would win on the chip, but the safe
  default is the CPU, and the proxy system covers 4K sources with
  quarter-size copies that decode smoothly anywhere.

## 7. The feature: the Wyzowl sound effects, and a search that ranks

Section 5's library of 110 sounds grew by **320 CC0 sound effects from
Wyzowl's sfx-kit** (https://huggingface.co/datasets/wyzowl/sfx-kit) - a
collection built for video editors, with whooshes, swishes, zips, swipes,
reverse whooshes, risers and fallers, impacts, cinematic hits, booms and
bass drops, pops and transforms and shimmers, UI clicks, keyboard
typing, phone sounds, notifications, digital glitches, data and chart
sounds, foley materials, everyday props, mechanical switches, writing
and drawing, money, clocks, celebration and logo stings - **430 sounds,
4.4 MB more of Ogg, 11.5 MB in all**.

**What the metadata carries:** every one of the 320 arrives with more
than a name: a `description` ("A fast, crisp whip of air with a bright
edge and a little push behind it"), `useFor` lines ("Whip pans", "Fast
cuts between scenes", "Carousel and tab swipes"), and a `syncAt` point
- where in the file the sound's peak lands, the moment a move should
meet. The sounds themselves were only re-encoded: 48 kHz stereo and mono
Ogg Vorbis q5 out of the dataset's 24-bit WAV masters, no re-gain and no
trimming, because the dataset's loudness tiers (a *hard* whoosh is
louder than a *soft* one on purpose) and its timing metadata are the
product, and both must survive the conversion.

**Search, both ways, that ranks:** the search is now one implementation
in `concat-host` - `Catalogue::search` - asked by both the shelf's field
and the API's `sounds.search`, so a person and an agent are answered by
the same list. The words are still ANDed and still prefix-matched, but
they now match in six places instead of four - name, id, category, tags,
use-for lines and description - and where they matched says the order:
a word of the name counts most, then a tag, then the description, so
"fast whoosh" puts *Whoosh, fast and hard* ahead of a whoosh merely
tagged fast, and "card transition" finds the whooshes and swipes whose
use-for lines suggest cards. The little words that only glue a sentence
- "a", "the", "for", "with" - are dropped, so an agent's natural
"soft swish for a card transition" asks what it means and not one word
more; "in" and "out" stay, being what a zip does. `sounds.search` hands
the description, the use-for lines and the sync point back, so an agent
that cannot listen can still pick with the ears the metadata lends it,
and place by the moment a sound is for.

**Where the categories went:** the shelf's filter grew from six to
twelve - Whoosh, Impact, Riser, Motion, UI, Notification, Keyboard,
Digital, Foley, Writing, Ambience, Music - folding the dataset's own 28
categories into the ones a person browses by, each word translated in
all 26 locales. The dataset's own category words ride along in each
sound's tags ("swish", "zip", "notification", "foley"), so the search
narrows by them without a filter row of their own. A person hears a
sound the way the shelf always played it - a click previews it, fetching
it first (a few kilobytes now) when it is not cached; a plus or a drag
lays it on the timeline.

**What it deliberately does not do:** no `avoidFor` field is carried -
a search that matched what to avoid would find the wrong sounds on
purpose, and the descriptions say their own cautions in their own words
- and nothing of the dataset's pro extras (LUFS, true peak, rate ranges,
pairing advice) beyond the sync point, which is the one an agent can act
on. Everything else stays in the dataset, which the manifest's `source`
line points at.

## 8. The feature: the vision critic's three rungs

Upstream's `craft_critique` had two rungs: a measured pixel/audio baseline,
and a bundled 0.5B local model that flags visible defects. This fork adds
the third, and strengthens the first.

**The hosted rung.** `vlm.py` now speaks authenticated OpenAI-compatible
HTTP: a Bearer key travels with every request, so `FUNCUT_VLM_ENDPOINT`
finally works against endpoints that require one, and a saved provider
config (`vlm.json` in the app's own data dir) adds four free-tier hosted
vision APIs — groq (llama-4-scout), gemini (Flash models), zai (GLM vision
flash) and openrouter (`:free` models) — wired up by one MCP call:
`local_vlm {action:"configure", provider, apiKey}` (the key is masked in
every reply; `providers` lists the live table; `clear` removes it). A
keyed hosted entry is trusted even when its probe fails, so the critique
surfaces the provider's own 401 instead of silently dropping to pixels,
and a hosted entry unlocks the full six-axis prompt on its own. **The
privacy line is drawn in words everywhere it matters: a hosted provider
sends the contact-sheet frames to that provider's API; the local rungs
never leave the machine.** The full map is `docs/VLM.md`.

**The stronger baseline.** The no-VLM heuristic scorer measures more than
it did: overexposed frames (blown highlights were measured but never
consumed by any score), per-frame colour saturation and a whole-sheet
colour-starvation check, and an opening-frame hook check (a black, barred
or flat first frame costs the hook axis). The duplicate-shot dHash bug is
fixed — it sampled only the top ~2 of 18 grid rows before breaking at 64
bits, so two frames differing below the top border hashed identical and
duplicate detection was blind to the lower ~90% of every frame; the hash
now strides the whole frame. Confidence rises honestly from 0.45 to 0.5,
and the selftest covers every new path.

**The motion layer got faster and truer.** `Geneva::bake` honours its own
cache key: a bake whose manifest matches the comp hash, engine, engine
version, size, rate, alpha and frame count - with the frames still on
disk - answers without re-rendering (`cached: true` in the reply), so the
inspect -> frame -> bake iteration loop stops paying for a full re-render
every time. Bakes are clamped to 4096 a side (the tool description always
promised "never above 4K for safety"; now the clamp exists and the schema
agrees), and `captions_animate` takes `curtain: false` for hard cuts
between caption lines - the dips read as flashes when the reel is burned
over a voiceover.

## 9. The feature: the maestro judgment skill

The AI director got a second shelf to read: **maestro**
(https://github.com/leobbaroni/maestro, MIT, vendored verbatim at
`.claude/skills/maestro/` - 764 files at this count, its own documented project-skill
install path) - 28 distilled reference modules over ten vendored corpora:
a 157-card product-shot vocabulary, engine-agnostic motion principles, a
sound-design and beat-sync method, art-direction catalogs, critique gates.
The funcut-director skill routes into it phase by phase (its section 6):
the director is the hands, maestro is the eye, the Phase-0 brief stays the
single locked brief, and the hard floors win wherever guidance conflicts.
`scripts/maestro/` carries the upstream drift checker so the vendor can be
re-synced instead of freezing. All licenses (MIT / Apache-2.0) are
AGPL-compatible and attributed in `THIRD_PARTY_NOTICES.md`; the map is
`docs/MAESTRO.md`.

## 10. The feature: the emotion-driven edit

The director pipeline could see its cut (the contact sheets, the critic)
and change it (the edit tools), but structure alone drove every decision:
sections, pacing floors, variety laws. The `emotion` tool adds the missing
sense - what each line of the voiceover **feels like**, measured from the
words, and the laws that make every layer answer to that read.

The read itself: `emotion {action: "extract"}` on a `transcribe` job's
output (or `lines`/`script` before any recording exists) returns, per line,
one of seven emotions with intensity and a full score distribution, the
dramaturgical act (hook / setup / tension / turn / reveal / proof / cta), a
hormone shorthand for the viewer's state, the TTS delivery and speed that
would read the line, the easing curve as **exact concat-motion authoring
JSON** (tense eases in, warm eases out, a burst pops, anger snaps), the
colour temperature with a grade hint, the SFX register, the BGM duck depth
in dB, pacing, and 2-3 word stock queries - plus the whole piece's arc
(shape, score, warnings) checked against the brief's `emotionalArc`.
Two engines, both first-class: a stem lexicon always (confidence 0.55), or
the free hosted `j-hartmann/emotion-english-distilroberta-base` on
HuggingFace's Inference API when `FUNCUT_EMOTION_API_KEY` / `HF_TOKEN` is
set (65/35 blend, confidence 0.8). No key is ever required; a failed
hosted call falls back to the lexicon and says so.

What the read then drives: the edit plan carries per-segment `emotion`,
`act`, `easing`, `colourTemp` and `duckDb` (validated against the canonical
schema, optional so non-voiceover edits still pass); the **placement gate**
(`emotion {action: "gate"}`) checks every stock candidate against the beat's
emotion register, act shot and the brand guide's taboos - 0-1 failed checks
passes, 2+ rejects, every candidate rejected means DROP THE BEAT, never pad;
the brand guide system (`emotion {action: "template"}` returns one to copy,
`creative_brief` stores it as `brandKit`, taboos are enforced as blockers);
the mix ducks to the act's value (-15 under the naked hook, -9 letting the
reveal ride); the grade pushes per-act colour temperature inside one look;
and after export, `emotion {action: "verify"}` aligns the edit's word
timings against the re-transcribed render - matchRate, drift and duplicated
words - proving the speech itself survived the encode. The full map is
`docs/EMOTION.md`.

## 10a. The feature: the agent's senses and its director's brain

Six capability groups landed around the emotion work and belong in this
ledger (they were verified from source for the inventory in
`docs/FEATURES.md`, which lists them with their counts):

- **The analysis layer.** `analyze_scenes` (PySceneDetect, three
  detectors), `analyze_beats` (librosa: bpm, beats, downbeats, onsets),
  `analyze_silence` (ffmpeg silencedetect or librosa fallback), all run as
  jobs waited on with `analysis_wait`, the scripts embedded in the binary
  and materialized on first use - a missing wheel says the exact pip line.
  `suggest_cuts` covers rhythm with **no Python extras** (ffmpeg-only BPM +
  beat-aligned cut times), and `measure_loudness` puts EBU R128 (integrated
  LUFS, true peak, error against the platform target) on the API - the only
  proof a loudness target was met.
- **Speech on the API.** `transcribe` (Whisper, word timestamps, language
  hints), `tts_generate` (all three voice families, delivery presets,
  speed/pauses/steps), `models_list` / `models_download` - headless model
  fetching as jobs.
- **Perception.** `preview_frame`, `stock_contact_sheet` (one labelled
  image of B-roll candidates with pixel metrics, crop hints and
  ready-to-use 9:16 `clip_transform` offsets), `watch_render` (the whole
  cut as one contact sheet plus measured facts: black frames, letterbox
  bars, flat/duplicate shots, static transitions, audio envelope, R128),
  `motion_frame` - all return MCP image content so the model actually
  looks. `craft_critique` scores six axes on three honest rungs
  (`docs/VLM.md`) and journals every pass to `craft-history.json` with
  pass/delta/trend. `creative_brief` persists the why (and the brandKit)
  as `creative-brief.json`.
- **The funcut-director skill.** `.claude/skills/funcut-director/`: the
  0-8 phase pipeline with the 1b/5b sub-phases, the 13 hard floors, the 9
  hard rules, the v2 edit-plan schema (`templates/edit_plan.schema.json`)
  and the 18 prompt playbooks under `prompts/`. `plan_validate` enforces
  the schema's cross-field constraints **and the creative gate**
  (`creativeReady`, `creativeBlockers`, `craftScore`) before edits.
- **The motion layer and its skill.** `concat-motion` (the Geneva sidecar,
  12 presets, the authoring format, `motion_overlay`'s honest alpha path -
  `docs/MOTION.md`) plus the `motion` skill that teaches an agent to
  design with it, and the `concat-motion-agent` CLI.
- **The Forge integration.** The Forge page in the editor (library page 12:
  script box, voice/niche/mood, Start - the harness streams into the page
  and its edits land as undoable commands), `.forge/mcp.toml`, seven
  provider key fields, `FUNCUT_FORGE_MODEL`, overnight queues -
  `docs/forge/FORGE.md`, `ARCHITECTURE.md`, `RESEARCH.md`.
- **The sticker library** (also counted under the libraries): MAGICK
  145,193 + OpenMoji 4,565, five `magick` tools, the Stickers page -
  `docs/MAGICK.md`, and the CC BY-NC line the director skill enforces as
  floor N.

## 11. Everything else: verified, not changed

The rest of the fork's work was verification, and it is worth saying what
that means concretely. The full engine test suites were run against every
change (the counts are that moment's; the library features added more
after): `concat-media` (125 tests), `concat-project` (174), `concat-host`
(111), `concat-export` (39), plus clippy and rustfmt clean across all
touched crates, and the locales inventory in sync for all 26 languages.
The three HDR/HLG tests that fail in that local run fail on the untouched
upstream tree in the same sandbox for the same reason (Debian's minimal
FFmpeg build lacks the HDR filters); GitHub's CI, which builds against a
full FFmpeg, is the authority and runs on every push.

Keyframes, transitions, effects, the cutout models, captions, the proxy
system, speed ramps - none of it is modified by this fork. The two things
that touch it are the memory table above (which changes *how much* is held,
never what is computed) and `removeSpans`, which is exercised by the eleven
engine tests listed above, including the keyframe and fade sharing.

## 12. The files

| File | What it is |
| --- | --- |
| `src/crates/concat-media/src/memory.rs` | new: the machine's RAM, and the small-machine line |
| `src/crates/concat-media/src/silence.rs` | new: the pause finder (streaming, pure, tested) |
| `src/crates/concat-media/src/pool.rs` | readers and stills scale with the machine |
| `src/crates/concat-host/src/titles.rs` | painting budgets scale with the machine |
| `src/crates/concat-host/src/playback.rs` | audio mappings scale with the machine |
| `src/crates/concat-project/src/commands/` | new `removeSpans` command + tests |
| `src/crates/concat/src/panes/voiceover.rs` | the button's wiring |
| `src/crates/concat/ui/dialogs/voiceover.slint` | the button itself |
| `audio/` | new: the CC0 sound library - 430 files and `manifest.json` (110 curated, 320 from Wyzowl's sfx-kit) |
| `src/crates/concat-host/src/models.rs`, `sounds.rs` | the manifest and mirror URLs point at this fork; the `tags`, `description`, `useFor` and `syncAt` a search narrows by, and `Catalogue::search`, the one ranked search both the shelf and the API ask |
| `src/crates/concat-api/`, `src/crates/concat-mcp/` | the `sounds.search` / `sounds.add` methods and tools |
| `src/crates/concat/src/panes/sounds.rs`, `ui/workspace/media-pane.slint`, `audio-page.slint`, `locales/` | the shelf's sidebar row, search field, category filter and words |
| `src/crates/concat-host/src/bgm.rs` | new: the music library - catalogs, ranged reads, search, tests |
| `src/crates/concat/src/panes/bgm.rs` | new: the Music page's wiring |
| `src/crates/concat/ui/workspace/bgm-page.slint` | new: the Music page itself |
| `src/crates/concat-api`, `src/crates/concat-mcp` | the `bgm.*` methods and the five agent tools |
| `docs/BGM.md` | the music library's own page |
| `src/crates/concat-export/`, `src/crates/concat-cli/` | pools sized for the machine |
| `install-linux.sh`, `docs/FUNCUT.md` | the one-line installer; the small-machine notes |
| `src/crates/concat-mcp/sense/vlm.py` | hosted providers, Bearer auth, the `vlm.json` config, masked keys |
| `src/crates/concat-mcp/sense/sense.py` | the stronger heuristic baseline (overexposure, saturation, opening frame, full-frame dHash) |
| `src/crates/concat-mcp/src/sense.rs`, `src/crates/concat-mcp/src/tools.rs` | the `local_vlm` configure/providers/clear actions and schemas |
| `src/crates/concat-motion/src/geneva.rs`, `engine.rs` | the bake cache short-circuit (`cached` never persisted) |
| `src/crates/concat-mcp/src/motion.rs` | the 4K clamp, `cached` in the reply, `curtain: false` for captions |
| `docs/VLM.md` | new: the vision critic's three rungs, providers, privacy line |
| `docs/MAESTRO.md`, `.claude/skills/maestro/`, `scripts/maestro/` | new: the judgment skill, vendored, with drift tracking |
| `src/crates/concat-mcp/sense/emotion.py` | new: the emotional read - lexicon + hosted engine, the act classifier, the brand guide, the placement gate, the transcript alignment |
| `src/crates/concat-mcp/sense/sense.py` | the `emotion` proxy beside the `vlm` one, selftested in CI |
| `src/crates/concat-mcp/src/sense.rs`, `src/crates/concat-mcp/src/tools.rs`, `src/crates/concat-mcp/src/protocol.rs` | the `emotion` MCP tool: embedded runtime, schema, dispatch, creative-loop wiring |
| `.claude/skills/funcut-director/SKILL.md`, `templates/edit_plan.schema.json`, `templates/edit_plan.example.json` | Phase 1b the read, the per-segment emotion fields, the gate as hard floor E, the export verify |
| `docs/EMOTION.md` | new: the emotion-driven edit's own page |

## 13. The upgrades: the multilingual emotion read, and Download

Two changes asked for by the fork's own use: the emotion read now speaks
the creator's language, and the finished export can be downloaded.

**The multilingual model.** The hosted half of the `emotion` read was
`j-hartmann/emotion-english-distilroberta-base` - English-only, and the
fallback lexicon is English-only too, so a voiceover in Bengali, Hindi,
Arabic or any other script read as neutral line after line, silently, and
every layer downstream (easing, colour, duck, stock queries, the gate)
answered a read that never happened. The hosted engine is now
`tabularisai/multilingual-emotion-classification` - XLM-RoBERTa fine-tuned
across 23 languages (English, Chinese, Spanish, Hindi, Arabic, Bengali,
Portuguese, Russian, Japanese, German, Indonesian, Tamil, Vietnamese,
Korean, French, Turkish, Italian, Polish, Ukrainian, Urdu, Dutch, Punjabi,
Swahili). Its eleven labels fold onto the edit's seven before anything
downstream sees a score: contempt rides the disgust register, frustration
the anger cut, gratitude and love the joy lift (love at calm arousal lands
on oxytocin, which is the state the edit wants anyway). The fold ADDS
multi-label probabilities, so a tender line reads more joyful than a
merely happy one - which it is. The blend moves with the model's standing:
75/25 model/lexicon (the multilingual semantic read is the read now; the
lexicon keeps a quarter for the punctuation and caps the model never
sees), confidence 0.85, engine reported as `model+lexicon`. Long
transcripts travel in batches of 64 lines per request. And the honesty
runs both ways: without a key, a mostly non-Latin script is answered with
a note that says the lexicon is English-only and names the env var,
rather than a silent run of neutral lines. Nothing downstream changes -
the seven labels, the PROFILE, the plan schema, the gate, the verify were
already written in the edit's seven, which is why a better model slots in
as a drop-in.

**Download.** The export sheet's finished state had Show in folder and
Export again; it now also has **Download**, which copies the finished file
into the person's Downloads folder - the one place a browser's muscle
memory goes looking. A copy, never a move: the export stays where it was
written, Reveal still finds it, and Again still starts over beside it. A
file of the same name already downloaded is never replaced - the copy
lands as `name (2).mp4`, `name (3).mp4`, the way a browser numbers its
own. The copy runs on a worker (an export can weigh gigabytes) and
reports through a toast in the person's language; the words ship in all
26 locales. Show in folder gets the folder icon it always meant to have,
and Download takes the arrow-into-tray.

The files:

| file | what |
| --- | --- |
| `src/crates/concat-mcp/sense/emotion.py` | `tabularisai/multilingual-emotion-classification`, the 11-to-7 label fold, 75/25 blend, confidence 0.85, batched hosted calls, the non-Latin note, the selftest's fold/multilingual cases |
| `src/crates/concat-mcp/src/tools.rs` | the `emotion` tool's description and schema strings |
| `docs/EMOTION.md`, `docs/MCP.md`, `src/crates/concat-mcp/sense/README.md`, `.claude/skills/funcut-director/SKILL.md` | the engines table, the model, the weights, the honest-fallback wording |
| `docs/FEATURES.md` | the inventory's emotion-engine and export bullets brought along |
| `src/crates/concat/src/panes/export.rs` | the `Download` message, the worker-side copy, the numbered-name logic, the test |
| `src/crates/concat/ui/dialogs/export.slint` | the Download button beside Show in folder; the folder glyph on Reveal |
| `src/crates/concat/src/lib.rs` | the button's wiring |
| `src/crates/concat/locales/*.json` | `export.download`, `export.downloadedTo`, `export.couldNotDownload` in all 26 languages |

## 14. The fix: the red turned green, and the emotion model reachable from the editor

Section 13 shipped one file short and one file rough, and CI said so in
red: `lib.rs` called `app.on_export_download()`, but the window's
`app.slint` never declared the `export-download` callback that generates
that method - the whole workspace stopped compiling at E0599 - and
`export.rs` had drifted past `cargo fmt`. The callback and its wiring in
the dialog instantiation are now there, exactly as `export-reveal` and
`export-again` do it beside it, and the formatter ran over the file. The
Slint tree compiles clean through the same `slint-build` 1.18.1 pass the
window crate uses, and the generated code carries `on_export_download`.

The second half closes a gap the multilingual model could not cross
alone: the Forge page handed the harness seven chat-provider keys, but
never a HuggingFace token - and the `emotion` tool reads its hosted
model only through `FUNCUT_EMOTION_API_KEY` / `HF_TOKEN`. A director
launched from the editor therefore ran the English-only lexicon forever,
model or no model. The page now carries a HuggingFace row (saved in the
settings file with the others), injected at run time under BOTH
spellings, so the director's very first `emotion` call reads the
voiceover with `tabularisai/multilingual-emotion-classification` - the
75/25 blend, the 23 languages, confidence 0.85 - the way the lexicon
always fed the whole plan: easing, colour, duck, stock queries, the
gate.

The files:

| file | what |
| --- | --- |
| `src/crates/concat/ui/app.slint` | the missing `export-download` callback and its wiring in the dialog instantiation |
| `src/crates/concat/src/panes/export.rs` | `cargo fmt` over section 13's additions |
| `src/crates/concat/ui/workspace/forge-page.slint` | the `hf-key` property and the HuggingFace row among the provider keys |
| `src/crates/concat/src/prefs.rs` | `ForgePrefs.hf`, documented |
| `src/crates/concat/src/panes/forge.rs` | the `hf` provider's save; `FUNCUT_EMOTION_API_KEY` and `HF_TOKEN` injected into the run's environment |
| `src/crates/concat/src/studio.rs` | the page's key sync |
## 15. The feature: a hundred viral transitions, a hundred effects, and the Hollywood sound pack

The fork's shelves of transitions (22), effects (23) and CC0 sounds (430)
were honest upstream inherited sets. This change triples the first two and
grows the third by a quarter, all of it verified the way the repo already
verifies its own.

**Where it is:** the Effects and Filters browsers in the editor, the
`effects_list` MCP tool, and the Audio page's Built-in sounds shelf.
Nothing new was wired by hand: a package is a folder under
`src/crates/concat-effects/packages/`, embedded by the build, loaded by
the catalogue, previewed by its own card and offered by the tools - so
the two hundred new packages appear everywhere the old ones do.

**What it is:**

- **100 transitions in ten new shelves** - Viral Zoom (punch-ins, crash
  zooms, dolly vertigo, velocity ramps), Whip & Pan (directional whips,
  lurches, pendulum swipes), Glitch (datamosh, signal drop, bitrate
  crash, pixel storms), Shape Reveal (elastic circles, stars, hearts,
  hexes, crosses with lit rims), Slice & Split (venetians, checkers,
  razors, blades), Spin & Flip (doors, cards, barrel rolls, portals),
  Flash & Impact (whiteouts, blackouts, strobes, exposure slams),
  Organic (ink bleeds, smoke, ripples, kaleidoscopes, melts), Meme &
  Bounce (shake cuts, jelly wobbles, squash-and-stretch, elastic snaps),
  Cinematic (film burns, letterbox slams, speed streaks, anamorphic
  squeezes) and Social FX (clean story slides, swipes, zoom slams).
  Every one is a two-input WGSL shader with an FFmpeg `xfade` fallback
  for export without a GPU, one to three knobs, and a preview card drawn
  by the shader itself.
- **100 effects in eight new shelves** - Light & Glow (god rays, bokeh
  sparkle, anamorphic streaks, star filters), Retro & Tape (VHS lines,
  CRT triads, tape tracking, projector flicker, halftone print), Distort
  (heat shimmer, twirl vortices, fisheye pulses, earthquakes, handheld
  sway), Speed & Motion (speed lines, radial blurs, motion echoes, beat
  pulses, directional blurs), Color Play (chromatic pulses, hue drifts,
  thermal, night vision, x-ray, posterize), Particles (snow, embers,
  confetti, fireflies, rain, fog, lightning, matrix drizzle), Frame &
  Focus (cinema bars, vignettes, split lines, photo frames), and Stylize
  (manga impact, blueprint, crosshatch, toon shade, glitch blocks).
- **100 Hollywood sounds** designed and synthesised in this repository -
  trailer slams and sub drops, braams and reverse sucks, doppler flybys
  and dragon passes, glitch zaps, portal and magic reveals, water, ice
  and fire design, dark drones and wide winds, and logo stingers - all
  seeded and deterministic, encoded Ogg Vorbis q4 at 48 kHz stereo,
  normalised with a decode-verify loop so nothing leaves full scale,
  each with its SHA-256, tags, a sentence of description, use-for lines
  and a `syncAt` peak position, in the same manifest the shelf and the
  `sounds.search` API already read.

**How it was verified:** the probe fixtures every WGSL package ships are
pinned from a double-precision model of the same shader under the exact
conditions of the GPU probe (the 8×8 half-float picture, the centre
pixel, time zero), including the integer hash the prelude defines, which
the model reproduces bit for bit - the same method `concat.film-grain`'s
fixtures document. The loader's own gates all run in CI: every package
loads, every knob the manifest declares is read by its shader and every
`params.` read is declared, and every package probes its default and
both slider bounds. The sounds are checked into `audio/` with digests
the app verifies on download, exactly like the rest of the library.

**What it costs:** each package is a few kilobytes of TOML and WGSL
embedded in the binary; two hundred of them add well under a megabyte
and load in the same catalogue pass as before. The sound files add
about 4.5 MB to the repository and to the `audio-v1` release mirror;
nothing ships inside the app or loads at startup. The shaders use only
the prelude's helpers plus plain arithmetic, so they run anywhere the
existing packages run, GPU or not (the `xfade` fallbacks cover the
rest), and they were written for the small machines this fork targets.
