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
**Music** - 1,025 whole CC0 background tracks (FreePD) plus 166
MobyGratis tracks (free for non-commercial use only) on two shelves,
searched locally, fetched one track at a time. FreePD is the Internet
Archive's
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

## 16. The feature: multi-layer scenes, and the reel on the MCP

A scene in a sequence could hold one plate - a still under the comp,
drifting a few percent. That is the CapCut background, and it is one
layer deep. This change makes a scene as deep as it has pictures for:
a scene's `layers` stack bottom first, plates under the comp and - with
`above` - grain, vignettes, light leaks and foreground cards over it,
each on its own clock.

**What a layer is.** `SceneLayer` in `concat-motion/src/sequence.rs`:
an asset id, a drift (`zoom`, the scale delta the plate always had, and
`pan`, a drift across the frame as fractions of its width and height),
a position (`offset` from the centre, `scale`, `fit`), a mix
(`blend` - `screen` turns a soft radial picture into a light leak,
`multiply` shades one into a vignette; `opacity`; `fade_in`/`fade_out`)
and its own timing (`start`, `hold`). Two rules hold themselves so a
layer cannot go wrong quietly: a cover-fitted layer that pans or sits
off-centre is scaled up by its own travel, so the frame's edge never
shows behind it (a `scale` under one is a deliberate shape and keeps
its edges), and each fade is held to half the span with the hold key
dropped when the two would land on the same time, so the engine never
meets a track that runs backwards.

**The timeline it becomes.** A scene paints, bottom to top: its plates
in order (`s0.plate`, `s0.plate2`, ...), its comp, its overlays in
order (`s0.overlay`, `s0.overlay2`, ...), the curtain. A layer naming
an asset the sequence does not carry stops the plan and names the
scene - the old plate's silent skip is gone; a storyboard with a
missing picture is a mistake to hear about. A layer that starts after
its scene ends is dropped with a warning, and one that outlives its
scene is cut to the scene's end.

**The storyboard is the library's own now.** The document the
`concat-motion-agent reel` verb eats moved out of the binary into
`concat-motion/src/storyboard.rs`, so the MCP door and the CLI door eat
the identical contract: scenes (preset or comp file, ops, at, hold,
plate/zoom short form, layers), assets by id, audio beds, a curtain.
The short form is unchanged - `plate` + `zoom` is still one layer under
the comp - and every error names the scene it stopped at, which the old
CLI errors did not do for a bad preset or comp file.

**The AI can drive the whole thing.** A new MCP tool, `motion_reel`
(the eighty-eighth), takes the same storyboard inline: `plan: true`
lays it out and returns the timeline without rendering, `frame:
seconds` renders one still and returns it as an inline image (the
look-before-you-render of the design loop), and the render lands under
the project's `motion/` ready for `media_import` and `clip_add`. The
cache is content-addressed by the storyboard's own hash. The door holds
its line: the project is the storyboard's world - every asset and comp
path is confined to it, a `source_dir` in the payload is ignored, and
a symlink cannot quietly move an asset out.

**How it was verified:** fourteen new unit tests read the emitted
timeline document itself - layer order around the comp and under the
curtain, the pan keyframes in pixels, the cover-safe scale (1 + twice
the travel), the fade keys and their clamping, per-layer timing inside
its scene, the dropped-late-layer warning, the refused unknown asset,
the short form's compatibility, and the storyboard parser's full
document, short form, error messages (scene index, fit and blend
vocabularies, unknown bed asset, comp paths out of the folder,
malformed operations) and hash stability. The tool carries a sample in
the MCP suite's every-tool-has-a-schema-and-a-sample test, and the
whole `concat-motion` and `concat-mcp` suites run green - 116 and 29
tests plus the six end-to-end ones, under the same pinned toolchain,
FFmpeg and ONNX Runtime CI pins.

Then it was verified for real, end to end, on this machine: the real
Geneva 1.6.0 sidecar rendered the `layers` example (two scenes, eight
timeline layers - two plates, a comp, two overlays, a second scene's
comp and its late-arriving foreground card, and the curtain) to 300
frames of 1080p30 H.264 with zero errors and zero warnings, and the
frames were measured: the mid plate's fade-in, the screen-blended
leak's lift in the top-left quadrant, the card arriving exactly at its
`start`, and the curtain's open, dip and close all land where the
document says. The MCP door was driven through the protocol itself:
`motion_reel` answered `tools/list` as the eighty-eighth tool, planned
the same storyboard, returned one still as a real inline image, and
rendered it under the project's `motion/` - and a storyboard that
named a path out of the project, or a project outside the session's
roots, was refused. A vision model looking at the returned still read
the stack back: the warm mid layer, the glow in the upper-left, and
the wordmark over both.

**What it costs:** one `Vec<SceneLayer>` per scene and one clip per
layer in a document that renders in a single pass; a layer adds no
engine, no binary and no protocol - it is a picture on the timeline the
compositor already draws. `Plate` (the old single-plate field) is gone
from the public `sequence` API; in-repo callers were moved to
`SceneLayer` in the same change, and the storyboard's `plate`/`zoom`
short form keeps every existing storyboard rendering as it did.

## 17. The verification: measured, then looked at

Section 16 ended with "the frames were measured". Measurement is not
inspection: a number that says a region changed does not say the card
floats over the footage, only that something happened where the card
should be. This change closes that gap, and leaves the procedure in the
repository so it is a command, not a memory.

**The assets are written, not carried.** `scripts/motion/layers-assets.py`
generates the `layers` example's pictures from the standard library
alone: two gridded plates (gridded on purpose - a grid under an overlay
is identifiable in a diff, where a smooth gradient is not), a radial
light leak, a 256x256 grain tile from a fixed LCG seed, and the three
faces the comps name, copied from `assets/fonts/`. Deterministic by
construction, so two machines produce byte-identical assets and the
example's cache key survives a re-run; the plates are PNG now, which is
what every diff and every engine reads exactly.

**`scripts/motion/inspect-frames.py` measures the two claims a layer
stack makes, and looks at them.** The card claim: the frame where the
card overlaps its scene (t = 7.0 s) against the same scene before the
card arrives (t = 5.5 s) - the card's centre region differs (mean luma
difference 58.5, peak 102), a border ring around it does not (0.0), and
four samples inside the frame but outside the card read the scene's own
tones, not the card's; a card that *replaced* the footage would fail
all three. The blend claim: the frame with the screen-blended leak
against the same frame rendered with that one layer dropped from the
staged timeline - the leak's core differs (mean 144.8) while a far
corner does not (0.0), and at the leak's rim the plate is lighter with
the leak than without it (mean luma 164 against 72) with the grid's
contrast surviving underneath (46 against 49); a cover would have no
ground detail under the rim at all. Then the frames were read back by a
vision model, which said the same thing in words: the grid lines and
the wordmark stay visible through the glow, the card sits over a comp
whose own text is still readable around it, and the frame reads as one
composed scene, not a collage.

**Determinism, measured.** Rendering the whole example twice from the
same assets produces byte-identical H.264 (the engine's report: 300
frames of 1080p30 in 22.7 s on two cores, zero errors, zero warnings,
one note naming the x264 build) - which is what makes a content-hash
cache honest, and what the scene-level cache coming next stands on.

**What it costs:** two scripts under `scripts/motion/` that need only
`python3` and the sidecar, one edit to the example (the plates are
`.png` now), and a section in `docs/MOTION.md` that says how to run the
whole procedure in four commands.

## 18. The guard: limits, a breaker, and diagnostics that parse

A call to the motion sidecar was `Command::output()`: however long the
binary took, the caller waited, and the MCP's 900-second client timeout
was the only thing between an agent and a sidecar that hung. A refusal
came back as the first line of stderr the caller had to guess at, and a
sidecar that failed again and again was asked again and again.

Every call is now bounded and counted. Bounded: a render runs under a
300 s limit, a plan or a frame under 60 s, both configurable - one cap
from `agent reel --geneva-timeout SECONDS` or the `motion_reel` tool's
`genevaTimeoutSecs` - and a call that outlives its limit is killed and
refused as `MotionError::TimedOut`, which names the call and the limit.
The child's pipes are drained while it runs, so a chatty sidecar cannot
deadlock on a full buffer either. Counted: three failures inside sixty
seconds - a refusal, a crash, a timeout, from any door in the process -
open a circuit breaker for a minute; further calls are refused
immediately as `MotionError::CircuitOpen` with the fallback story in the
message, and a success closes it. `EnginePreference::choose` folds the
breaker into the engine choice: a failing sidecar is a missing one for
every comp the native compositor can draw - with a note the caller is
asked to log, because a fallback nobody can see is a fallback nobody can
trust - and a markup comp keeps Geneva, the native engine not being able
to draw it, so the honest answer is the fast refusal.

Refusals parse now. `--format json` was already asked for; the
diagnostics it carries are read whole - code, severity, the JSON-pointer
`path` into the timeline, message, value, help - and `MotionError::Engine`
carries them (`E302: ... (at /layers/1/clips/1/start)`), with the most
specific help available: the engine's own suggestion, else a table that
says what the code means in storyboard terms (the codes are the
sidecar's own `geneva guide errors`; E302 is clip overlap, W405 a font
the machine lacks), else `geneva explain <code>`, asked of the sidecar
itself.

Nine tests drive stub executables written into tempdirs - a sleeper that
outlives its limit and is killed, a broken sidecar that trips the
breaker in three and is refused fast on the fourth, a moody one whose
success closes the breaker, a fussy one whose diagnostics arrive whole
with the fix, an explainer for the unknown code, the fix table's shape,
the cap arithmetic, and the version probe under the same limits - and
two cover `choose`'s matrix: the shape comp that falls back to native
with the note, the markup comp that stays. Verified against the real
sidecar by the geneva.yml CI lane, which renders the layers example
end to end under the same guard.

**What it costs:** one watchdog thread pair per call (25 ms poll, pipes
drained), a process-global `Vec<Instant>` for the breaker, and one new
field on `MotionError::Engine` (`path`) whose in-repo constructors were
all updated in the same change.

## 19. The contract, checked before the bake

storyboard.rs defined the contract and resolved it; it did not check it.
A layer naming an asset the storyboard does not carry stopped the plan -
at plan time, a comp deep in the render; a negative `start` reached the
engine and came back as E300 in the sidecar's words, not the
storyboard's; a blend typo was caught one scene at a time.

`Storyboard::validate` is the step before all of that, and it reads the
document the way an engine never gets to: every asset a layer, a plate
or a bed names is carried; every blend one of the nine and every fit
one of the four; every `start`, `hold`, `at` and fade non-negative;
every zoom, pan, offset, scale, gain and opacity finite (and the two
fractions in range); a comp a name under the folder, not a route out of
it; a scene naming exactly one of a preset and a comp. Every problem is
found and returned together as `Vec<SceneError>` - one refusal that
lists them all, each with its scene and its layer (`scene 2, the layer
"leak": the blend "glow" is not one of ...`), because a twelve-scene
storyboard with six layers a scene is debugged from one refusal that
says which of the seventy-two is wrong, not from three render attempts.

`sequence()` calls it first, so both doors - the CLI's `reel` verb and
the MCP's `motion_reel` tool - refuse an invalid storyboard with the
same list, before a preset is resolved or an engine is woken.

Six tests: the missing asset named per case, the one-refusal list that
names every problem across three scenes, the bad fit beside the
out-of-range opacity, the non-finite scale (JSON cannot carry one; the
check is on the struct a program writes), the scene naming both a
preset and a comp, and the valid storyboard that validates clean - 133
tests green in total.

**What it costs:** one pass over the document before the pass that
resolves it, and nothing else; the checks are the ones the resolver and
the engine were making one scene at a time, moved to where they cost a
microsecond instead of a render.

## 20. The scene builder's three first-class options

The fields were on `SceneLayer`; the builder did not speak them as one
call, and the storyboard could not say the one that matters most.

`SceneLayer::parallax(rate)` is that one. A rate is a multiple of the
layer's own drift - `zoom` and `pan` as written, or, for a layer with no
drift of its own, the classic plate's four percent of growth and three
of slide - applied where the timeline is emitted, so the layer's own
numbers stay what they said and a layer built before the field existed
renders exactly as it did (`None` is the whole of backwards
compatibility; five tests pin it). Three layers at 0.5, 1.0 and 2.0 are
three depths of one scene, and the emitted timeline carries three
different scale keyframes and three different pan keyframes, asserted in
that order and in that ratio. The storyboard's layers carry `parallax`
too, validated as a positive multiple.

`blend(mode)` and `delay(secs)` join `blending()` and `starting_at()` as
the vocabulary's other names: a stack of layers taking the stage one
after another - `delay` 0.0, 0.4, 1.2 - is choreography, where a stack
that arrives at once is a collage, and the emitted timeline carries each
delay as the clip's own start. The `motion_reel` tool's layer schema
carries all three.

**What it costs:** one `Option<f64>` on a struct that is built by hand
or parsed once, and the rate arithmetic at emission - the layer's
document stays what its author wrote.

## 21. The card-or-scene router

An agent chose between a motion card and a multi-layer scene by prompt,
which nothing could test - the choice lived in a sentence a model wrote,
and a four-second beat could become a five-layer scene nobody can read,
quietly.

`concat_motion::route` is the deterministic default. `route(duration_secs,
beat_type)` - the beat types are the emotion read's seven acts - answers
Card, Scene or Nested: under 5 s is a card; a hook or a CTA stays a card
until 8 s, both being read at a glance; 5-15 s is a scene; over 15 s is
a nested composition, an act of scenes within, except the hook and the
CTA, which are each one beat however long the slot. A span that is not
a finite positive number routes as the shortest answer rather than as
whichever branch a NaN comparison fell through.

The answer carries its reason: `Decision::taken_by_default` names the
rule that fired, and the director's override rides along with the reason
it was taken (`Decision::overridden`) - an override nobody can see the
why of is a guess with confidence. The funcut-director skill's design
loop now routes before it designs, and the override is the only way off
the default.

The matrix is tested cell by cell - every beat at 0.5, 4.99, 5.0, 7.99,
8.0, 12.0, 15.0, 15.01 and 30.0 seconds - plus the NaN and the negative
span, the reasons the rules fire, the override path, and the names the
plan carries.

**What it costs:** one new module of pure functions, no state, no
engine; the agent calls it or reads its table, and the table is the
code.

## 22. The scene-aware reel cache: one run, one entry

A reel re-rendered on every edit is a reel nobody iterates on: the
whole ten-scene document rendered again because one layer of one scene
moved ten pixels. The reel's cache was the storyboard's own hash - all
or nothing.

`concat-motion::reel` renders a storyboard **run by run** (see §26 for
why a run, not a scene), and each is cached by a key derived from what
actually draws it: its scenes' documents (comp or preset, layers,
timing, operations), the **bytes** of every asset they name - a
re-saved picture misses, a renamed one does not have to - the geometry,
the engine and its version, and the curtain at the run's boundaries. An
edit to scene seven does not move scene three's key, and scene three's
run is served untouched. The joined reel is cached by its ordered run
keys, so an unchanged reel serves the join whole - byte-identical,
rendering nothing - and a one-run change re-renders that run, re-joins,
and nothing else. Audio beds render once, audio-only, cached by their
own numbers and their file's bytes, and mix over the joined picture at
the end. The run cache is shared across reels: a run identical in two
reels is one entry, drawn once.

Both doors eat it - `motion_reel` reports `cache: {scenesDrawn,
scenesServed, joinedFromCache, ...}` and the agent CLI's `reel` the same
- and a relative `-o` works, where the first cut of the join ran its
child in the empty string's directory and was refused with the one error
that looks like the executable is missing.

**Found and fixed on the way:** the comp timeline's `encode.video.`
`pixel_format: "rgba"` was never a field in the engine's schema - every
comp bake and every comp frame through the strict door was refused as
E101 (the sequence path never wrote it, which is why the layers example
rendered clean). The PNG image sequence is the alpha by being a PNG
sequence; the field is gone and the comp frame path works, verified
against the real sidecar.

Verified with stub-executable tests - a second render of an unchanged
reel renders nothing and is byte-identical; one layer changed
re-renders only its scene; asset bytes, not names, are the key; the
geometry is in it - and against the real sidecar: two scenes drawn the
first time, **zero** drawn and the join served the second, **exactly
one** drawn after one layer's parallax changed. 147 tests green.

**What it costs:** one directory of segments and joins under the
project's cache, a per-scene plan instead of one, and a join the engine
does back-to-back; an unchanged reel costs a handful of stat calls.

## 23. Nested compositions: a scene inside a scene

A scene could not reference another scene. An act that wanted its
opening sting inside its chapter card rebuilt the sting by hand, or
pre-rendered it to a file and carried the file - both of which forget
what the sting was the moment it changes.

A scene can be named (`"id": "intro"`) and another scene's layer can
place it (`"layers": [{"scene": "intro", "scale": 0.5, ...}]`): the
named scene is emitted once, as a reusable composition in the engine's
own format - its plates, its comp, its overlays, at relative times -
and every layer that names it shows it, each with the layer's own
timing, offset, scale, opacity, blend and fades. A composition that
itself places a scene nests one level per name, the engine holds the
stack to eight, and the segment cache includes a nested scene's own
key in its parent's, so editing the sting re-renders exactly the scenes
that show it.

A scene cannot be built from itself, directly or through another:
validation walks the names and refuses the chain as a sentence -
"a uses b uses a" - before a preset is resolved, and both doors eat the
same refusal. A layer naming an undefined scene, or both an asset and
a scene, is refused by name.

Verified: the placement's timeline asserted (composition source, start
at the parent's start plus the layer's delay, the layer's transform and
blend), the direct and the two-scene cycle refused with the chain
named, the undefined name and the both-at-once refused, and the whole
thing rendered against the real sidecar - a chapter-chip scene holding
a half-scale, offset, fading copy of a wordmark scene, two segments
drawn, zero errors. 151 tests green.

**What it costs:** one composition per named-and-placed scene in the
document (unused named scenes are not emitted), and the recursive plan
of each - a plan of a document, not a render, so the cost is
microseconds.

## 24. The reel contact sheet: the critic loop over the reel's own output

The agent authored scenes blind. `motion_reel` could return one still,
and the critic - the three-rung ladder behind `craft_critique` - never
saw the reel: the look and the score were two tools that did not meet.

`motion_reel {sheet: true}` closes the loop. It renders four stills of
the reel - a quarter in, half, three quarters, near the end, where the
exits live - tiles them into one 2x2 image in the server itself (the
`png` crate, the same one the API's inline frames use; no Python, no
second pass), and returns it as an inline image **and** as `pngPath`
under the project's `cache/sense/`, which is exactly the argument
`craft_critique` takes. The loop is then: `motion_reel {sheet: true}`
→ `craft_critique {sheet: pngPath, brief}` (the bare path string, or
the whole reading object - both are accepted) → revise what scores under
7 → again. The funcut-director skill's design loop says so now, at the
bake step: never ship an unscored reel.

A frame that is missing or unreadable is a black cell rather than a
refusal - the empty quarter is itself a finding, which is what a sheet
is for.

Verified: the tiler tested on four solid frames (each cell its own
colour, decoded back and asserted) and on three missing ones (black
cells, not refusals); the tool's argument carried through the schema;
32 tests green in concat-mcp.

**What it costs:** four frame renders at preview size and one PNG
encode - a quarter of a second of sidecar time on the measured
machine, once per look.

## 25. The sticker shelf swapped for a permissive set

The two sticker shelves were a liability wearing a search box: MAGICK's
145,193 AI-generated cutouts were **CC BY-NC 4.0** - unusable in a
monetized video - and OpenMoji's 4,565 colour emoji were **CC BY-SA
4.0**, whose share-alike term reaches into whatever it is layered on.
The fork had answered that with a blocking licence notice before the
page's first use; the honest fix was to stop shipping shelves that need
one.

**What changed:** the shelf is Microsoft's **Fluent Emoji**
(fluentui-emoji), 1,285 rendered 3D PNGs under the **MIT licence** -
free for commercial and personal use. The index is built by
`scripts/fluent-emoji-index.py` from the repository's own tree (one row
an emoji: codepoints, CLDR name, keywords, group, licence, PNG path) and
published as the `fluent-emoji-index-v1` release asset, digest-checked
by the build. Pictures fetch one plain GET each from
raw.githubusercontent.com with jsDelivr behind it. The `magick` tool
names stayed for compatibility - `source` accepts the old shelf names
and points them at the one shelf - and every result now carries a
`license` field, with the licence shown on the tile and as a line on
the page instead of a gate before it. The index is ~135 KB against the
old ~18 MB, and the first usable sticker arrives one download sooner.

Verified: the live end-to-end test (index down from the release, digest
checked, 1,285 rows parsed, a search, a real PNG fetched and
transparency-checked) runs green in `concat-host`; 11 unit tests hold
the parse, the search, the URL shapes, the licence field and the
favourites; the alpha check still refuses anything that is not a
transparent PNG.

## 26. The four pillars: the cut that shares its window, the noise that carries a seed, the cache that keeps its word

The motion layer had the vocabulary - six choreographed cuts, five curve
families, a beat-grid pulse, a critic that returns operations - and the
roadmap said what it lacked. This change is that audit implemented whole:

**The cut, choreographed in time.** A whip pan played as an exit, a
splice and an entrance, because the incoming scene started *at* the cut:
both halves animated, no frame ever held them both. The plan now lays a
scene entering by an overlapping cut `cut.seconds` **before** the scene
before it ends - the outgoing still on screen and sweeping out while
the incoming arrives - which is the whole read of one camera move. The
`stagger-exit` is the deliberate exception (its elements are gone
exactly *at* the cut; the reveal must never share the window), and a cut
longer than the scene before it can spare is clamped and said. The
shared window forced the reel cache's granularity to tell the truth:
scenes tied by an overlapping boundary are **one render unit** - a
composite no concat expresses - so the cache renders and keys them as
one *run*, while every plain boundary keeps its per-scene granularity.

**Noise with a seed.** `noise.rs` is a seeded simplex field - 2-D, 3-D,
fractal - a pure function of its seed, so the one randomness the
determinism contract allows is a function of the document. Two doors eat
it: a layer's `wobble` (`seed`, `amount` as a fraction of the frame,
`rate` as keys a second) folds the field onto the drift, baked into the
position keys at plan time (a cover layer grows to cover its own
wobble; a cut's sweep is never chopped into samples), and the
`set-noise-track` operation wanders a node's own property - position in
pixels (placed nodes only), scale, rotation, opacity around the resting
value. No seed is never a silently-picked one.

**The cache that keeps its word.** Three correctness bugs fixed and a
lifecycle added (`cache.rs`): the comp bake key covered the document but
not the **files it names** - "changed the logo, still the old logo" -
so every asset's bytes are streamed into the bake key now
(`manifest.json`'s `assets`); the file hash read whole files into RAM,
so it streams a megabyte at a time and answers *exactly* the old number
(no key in the wild missed); and every cache entry lands beside its
place and is renamed in, with the `.done` marker - written after the
rename, atomically itself - as the sentinel no half-written entry can
slip past. Every `render_reel` then sweeps its own cache root under a
2 GiB budget: least-recently-used *complete* entries go, serving counts
as a use, junk (orphan markers, stale temp files) goes first, and the
report says what went.

**The farm that remembers.** `--parts N` parallelised a render and
forgot it. `--farm DIR` (`farm.rs`, agent verb) asks the engine's own
`plan` verb where the good boundaries are, renders each part with a
plain `--frames` call, lands it in DIR marked `.done`, and joins with
the timeline's audio without a re-encode - so a run that dies at part
seven serves one to six on the way back and draws only the missing, and
the reply's `farmReport` says part by part what drew and what served.
The parts are keyed by the staged timeline's bytes: an edit re-renders
every part, an unchanged plan serves the lot.

**The discipline, as a lint instead of a mood.** `discipline.rs` audits
every planned scene and rides the plan's warnings: an entrance that
creeps in on `ease-in` or a straight line, an exit that hangs in the
doorway on `ease-out`, a bounce on a scene that never asked to be
playful (the `punchy` style is the ask, and it is honoured; `steps()` is
exempt as the mechanical reveal it is). Advice, not refusal.

**The determinism contract, published and enforced.**
`docs/DETERMINISM.md` writes the guarantee - same comp, same asset
bytes, same geometry, same engine build: bit-identical frames - and the
code holds it: `Comp::validate` refuses a markup that fetches
(`url(http://…)`, remote `src=`/`href=`), imports a stylesheet, or asks
the clock or an unseeded random for a value, with the token and a JSON
pointer. `scripts/motion/golden.py` proves it per run in CI: the
committed board renders at five fixed instants - one of them inside the
whip pan's shared window, both plates on screen - and is held to
committed goldens with a pure-Python DSSIM, plus one frame rendered
**twice and held to identical bytes** on the spot. The Geneva lane runs
it after the scene-quality floors; an intended change is `--update`,
said in the commit that moved the frames.

Verified: 305 tests in `concat-motion` (was 263), clippy and rustfmt
clean under the pinned channel - and for real, through the pinned
Geneva 1.6.0 sidecar: the whip-pan reel's boundary frames measured
pixel by pixel (left third the outgoing plate, centre and right the
incoming, at the shared window's middle); the run cache serving an
unchanged reel in under a millisecond; the farm's 317 frames in four
parts, resumed after one marker was deleted (one part drawn, four
served); the wobble's 21 keys, ±19 px of wander and the cover growth
read straight out of the emitted timeline; and every golden frame at
DSSIM 0.00000 against its committed image.

## 27. The plan's last four gaps, closed

The Geneva integration plan had ten steps, and the audit that re-read
them against the tree found ten built - and four places where what
shipped was beside the plan's own words rather than on them. This
change closes the four, each in the shape the plan asked for.

**What changed:**

- **`hue-rotate()` in the grade (step 8).** The LUT approximation was
  `saturate() contrast() brightness()` plus a soft-light cast; the plan
  named `saturate() hue-rotate() contrast()`. The grade now carries a
  `hue` field - degrees, -360..360, overridable per scene - emitted
  between the saturation and the contrast, and the temperatures that
  need a wheel turn carry one (`sickly` +25°, `split` -5°); the cast
  still does most of the hue work, and the warm/cold CI shifts are
  unchanged.
- **The critic score, as a number (step 6).** "Applying the fixes
  converges the score above 7" was held as "the defects are gone";
  `critic::score` makes it a number the loop can steer by: 10 to
  start, a contract problem -2, a clipped element -4, an element the
  reel never shows -2. The convergence test now asserts the broken
  scene scores 6 and the fixed one 10.
- **The act wired into the planner (step 4).** `curve_for_act` was
  exported for callers to translate; now a scene card names its `act`
  (hook, setup, tension, turn, reveal, proof, cta) and the planner
  gives a pulse that names no curve of its own the act's curve - calm
  for the hook, the overshoot for the reveal. A pulse that names a
  curve keeps it, and the pulse's `curve` field now takes a library
  name (`"bounce"`) as well as a full curve object, the same two forms
  `fadeCurve` always took.
- **`geneva_explain` as an MCP tool (step 7).** The engine's own
  answer to "what does E302 mean" was consulted inside refusals only;
  it is now a tool of its own - `code` is the code or a refusal
  sentence pasted whole, its first code-shaped token picked out - the
  door for the second and third codes a refusal carries. The catalogue
  is 91 tools; the README, FEATURES, MCP.md and the Forge allowlist
  all say so, held by the count test.

Verified: `cargo test -p concat-motion` - 268 tests and the doctests,
green, including the new `an_act_chooses_the_pulse_curve_when_nobody_
else_did`, `the_score_costs_what_the_contact_sheet_names` and the
three `hue-rotate` tests; `concat-mcp`'s count test holds 91 against
every document. The tool itself needs the sidecar and the ffmpeg
headers, so its build is CI's to run.

## 28. The whip's smear: the motion blur the whip pan shipped without

The whip pan shipped with its sweep, its shared window and no blur -
the dry read, for every whip, whether the scene asked for one or not.
A whip is a camera move, and a camera that moves that fast smears; the
sweep alone reads as a slide wearing the whip's name. This change gives
the whip its smear:

`whip-pan` carries `blur` (default 12 px - enough to smear the sweep,
not enough to read as a focus pull; `0` is the dry whip, the sweep
without the smear, which is a legitimate read). Both planes get it on
both sides: the plates rack through the cut's window - the outgoing
racking **up** into the boundary and gone into it, the incoming
resolving **out** of the same radius - and the comp's nodes rack beside
their slide, so the whole frame smears as one. The plan bakes it as a
keyframed blur effect on the plate clips and blur tracks on the comp's
nodes, the same machinery a rack focus rides; a dry whip racks nothing
at all - no track, no effect, the cut riding on the slide alone. Old
storyboards need nothing: the field defaults in, an unchanged document
is a changed read, and the scene keys - which hash the transition's own
serialisation - miss once, honestly.

Verified: 307 tests in `concat-motion` (was 305), clippy and rustfmt
clean under the pinned channel - and for real, through the pinned
Geneva 1.6.0 sidecar: the golden board's whip frame, re-rendered inside
the shared window, moved on purpose and was `--update`d - only
`whip.png` changed, every other golden byte-identical - the whip frame
still renders twice to identical bytes, and every golden frame holds
at DSSIM 0.00000 against its committed image.

## 29. `track_subject`: the crop path that follows the subject

The H->V reframe was answered by a single-frame `cropHint`: the energy
centroid of ONE frame, one static offset per clip. A vertical edit is
seconds long and the subject moves - the top feature for short-form
content, and the one the audit kept asking for.

`track_subject` (the 92nd tool) samples up to 24 frames across a clip
(or a `from`/`to` span), finds the subject in each, and returns the
crop path. The subject's weight in a tile is what stands out from the
frame's own median PLUS half of what stands out from the temporal
median of the same tile across every sampled frame - a static
background cancels to zero, a moving subject dominates, a
standing-out still subject still reads; confidence is how concentrated
the weight is, and a frame under the floor is "lost". A lost frame
HOLDS the last known position - through occlusion, through the subject
leaving the frame - rather than snapping to centre, and a subject that
enters late is entered upon from centre. The smoothed path is reduced
to keyframes at its turns, each with the exact `clip_transform`
offsets for the target aspect - the same geometry `reframeHint` uses,
so the two agree - ready for `keyframe_set` on the clip's
offsetX/offsetY. `moved: false` says the subject never went anywhere:
the single-frame hint is enough, key nothing.

The tracker lives in the sense layer (`track` command): pure stdlib,
no numpy, no OpenCV, the same ffmpeg decode the contact sheets use -
and it also takes `sources` as base64 PNGs, so the selftest drives it
with a synthetic blob that crosses the frame, vanishes for one sample,
and enters late, and CI proves the path follows it, holds through the
occlusion, and enters from centre with no decoder on the machine at
all. Verified against a real stepped-box video end to end: nine
frames, the box tracked -0.71 to 0.52, six keys at the turns.

**What it costs:** a frame decode a sample (24 at most) and one ffmpeg
probe - a few seconds on the 4 GB machine, once per look. Every
document that states the tool count now says 92; the Forge allowlist
carries the tool; the guard holds all of them.


## 30. The ship-blockers, answered: the cache's budget, the key's privacy, the stranger's endpoint, the pinned download, the batch's second pass

Five bugs the audit named and a roadmap's near side, closed in one pass.

**The comps bake cache is bounded.** `cache/comps/` grew a directory
per comp-content and size, forever: nothing swept it but the File
menu's clear-everything. The 1 November audit described an LRU that
never landed in a commit - this is it, built beside the reel cache's
own sweep instead of over it (`cache::sweep_comps`): a bake directory
is an entry when its `manifest.json` is there, the manifest's clock is
the use (a cache hit touches it, so a *served* bake outlives a merely
baked one), the bake this render just wrote is kept but counted, an
in-flight bake - no manifest, younger than an hour - is never touched,
and a crash's orphan goes as junk before anything useful does. Every
`motion_bake` sweeps its own root under the machine's own budget: 2
GiB, 1 GiB on a machine with six gigabytes or less,
`CONCAT_MOTION_CACHE_GIB` to name another. The reply says what the
sweep did only when it did something.

**The key's privacy.** `vlm.json` - a hosted critic's API key in
plaintext - was written 0644, world-readable on a shared machine. It
is written 0600 now, through a temp that is 0600 itself, in a
directory that is 0700, and a file from before the guard is tightened
at read. The vlm selftest pins the modes.

**The stranger's endpoint.** `local_vlm {action:'configure'}` took any
endpoint an agent named, and the critic's frames - and the key beside
them - would have been sent there: an exfiltration vector shaped like
a configuration option. The endpoint is allow-listed at all three
doors it leaves by (`configure`, `detect`, `ask`): a named provider's
own host, anything loopback, a host the operator listed in
`FUNCUT_VLM_EXTRA_ENDPOINTS`, the operator's own
`FUNCUT_VLM_ENDPOINT`, or `FUNCUT_VLM_ALLOW_ANY_ENDPOINT=1` for a
machine that wants it that way. Environment is the operator's; an
agent cannot write it.

**The pinned download.** The ffmpeg bootstrap fetched a rolling
"release" pointer from a personal site, digest-checked against an
empty table - the machinery was there, the pin was not. The bootstrap
now fetches BtbN's month-last autobuild of FFmpeg 8.1, the same
immutable tag `scripts/ci/deps.lock` pins the CI libraries from, and
every asset carries its digest, verified against the downloaded
bytes; the selftest refuses an unpinned asset or a rolling URL ever
again.

**The batch's second pass.** `openvid_search_batch` called the API
directly, past the dispatch arm that runs the CLIP rerank - eight
queries of BM25 word-matches, none of them looked at. The rerank rides
along now, once per query, exactly as `openvid_search` runs it;
`rerank: false` is the fast BM25-only sweep, and the reply's
`searches` carry each query's `rerank` verdict.

**The roadmap's near side.** Two looks the audit asked for - `vignette`
(the corners fall, the centre holds: the fringe's own shape in black,
all four edges) and `letterbox` (the cinema bars: solid black, holding
the frame to a ratio, 2.39 the scope screen, a ratio under the comp's
own the pillarbox) - with the grain's render coverage the looks never
had: a golden frame in the board (the existing five rendered
byte-identical beside it) and an eleventh scene held at a floor in the
quality manifest. Four curves the library lacked - `drift` (the sine's
own pace), `snap` (the harder emphasis), `accelerate` (from rest to
full speed) and `anticipate` (the wind-up that dips under its start) -
named, tested, mirrored everywhere the library's names are listed.
And the critic's fixes carry a `confidence` now, 0 to 1, said by the
prompt, clamped by the defence, refused out of range by the Rust
parser - the measured heuristic fixes say the engine's own 0.5 - so an
agent orders its work by a number instead of a hunch.

**What it costs:** the sweep is a directory walk a bake (the manifest
read it already had), the allow-list a hostname parse a call, the pin
nothing at all, and the batch's rerank the budget `openvid_search`
already ran per query. Eleven quality scenes hold their floors; six
golden frames hold at DSSIM 0.00000.

## 31. The library at eighteen: the six comps the vocabulary lacked

The preset library was twelve strong and thin in the places a real
video is made: one lower third, one chapter device, one code device,
one chart, one call to action. Geneva roadmap item 04 - eighteen
comps - closes the six gaps that were felt, not counted:

- **`funcut.lower-third.minimal`** - the documentary read: a name, a
  mono role, a hairline rule that draws from the left. No card, no
  shadow, for footage the glass card would have shouted over.
- **`funcut.chapter.numbered`** - the full-frame section change: a
  kicker, a giant number, an accent rule that draws, a title. The chip
  is the corner badge; this is the door.
- **`funcut.chapter.agenda`** - the table of contents: four numbered
  rows, each arriving a step after the one before, hairlines between.
- **`funcut.cta.endcard`** - the close: a kicker, a headline with one
  accent word, two buttons that settle one after the other, a footer.
  The subscribe card is the mid-roll ask; this is the last frame.
- **`funcut.code.diff`** - the second code device: a file header, a
  removed line in red, an added line in green, context between,
  landing on `steps()` a beat apart. Where the terminal types a
  session, the diff shows a change.
- **`funcut.chart.progress`** - one number, one fill: a wide hairline
  track and an accent bar that grows to the number it names.

All six carry the library's one design system (paper ink, editorial
gold, Archivo/Inter/JetBrains Mono), staggered CSS reveals, a spring
only where something is placed, and slots an agent fills - so the
`every_slot_can_be_filled` and `every_preset_builds_and_validates`
tests hold them to the library's own bar. The transitions side of
item 04 was already met: the catalogue's six choreographed cuts
(whip-pan, zoom-through, inverse-zoom, rack-focus, velocity-match,
stagger-exit) shipped with the overlap work in section 26.

Two drifts found and fixed on the way: `examples/reel.rs` still filled
a `Numbers` slot on the stat triptych that no longer exists (the reel
is an example, not a test, so nothing failed when the slot was
renamed - it failed the moment anyone ran it), and the committed
`examples/comps/` dumps predated the counting number's restyle. The
reel now plays all eighteen presets - nineteen scenes, 66.1 s - and
the dumps are the eighteen the source builds. Every new comp was
baked with the pinned sidecar 1.6.0 and its frames read before
landing; `docs/MOTION.md`, `docs/FEATURES.md`, `docs/MCP.md`, the
motion skill and the `motion_presets` tool description all say
eighteen now, because a catalogue the docs miscount is a catalogue an
agent misuses.

## 32. Typography's second pass: the directional sweeps, the tracking entrance, the paragraph and the gradient ink

Geneva roadmap item 13. The typography builder had three entrances,
two staggers and one treatment; the measured engine facts said three
more were free for the taking, and the module's own doctrine - build
what the engine draws, refuse what it does not - picked them:

- **Four entrances** - `slide-down`, `slide-from-left`,
  `slide-from-right` (the directional sweeps a third, a badge and a
  timestamp arrive on, the rises in pixels off the style's own size)
  and **`track-in`**, the cinematic title arrival: `letter-spacing`
  animates from `0.6 x size` to the style's rest while the line fades
  up. That last one rides a newly measured fact - the pinned engine
  **interpolates `letter-spacing` inside keyframes** (a 90 px line's
  right edge measured travelling 319 px), where the module's older
  facts are all refusals.
- **`paragraph(&[lines], ..)`** - the multi-line reveal, one `<div>`
  per line staggered `Kinetic::lines()`' 180 ms apart, the weight ramp
  riding the lines as it rides the words. An empty line takes no beat:
  a stanza break is not a 180 ms wait on nothing. Measured rendering:
  three lines at 180 ms stagger, each rising on the settle curve.
- **`gradient_headline(.., from, to)`** - the ink is a clip, not a
  colour: `linear-gradient` behind `background-clip: text`, the fill
  transparent, written after the base stylesheet so the cascade cannot
  lose it. Measured: the clip carries into the staggered inline spans -
  gold into ink across three words, drawn.

And one negative finding, held to the same standard: a **clip-path
wipe is not offered**. `inset()` in keyframes is refused outright
(E442, its own error naming `polygon()`), and a `polygon()` from-to
pair does not interpolate - it jumps to its end state at the delay's
edge (measured: zero ink at 0.2 s, full ink at 0.3 s, nothing between).
The module docs and MOTION.md both say so, so the next hand author
does not spend a bake discovering it.

The example (`examples/typography.rs`) grew to four nodes - the weight
ramp, the karaoke caption, a two-line paragraph and a gradient headline
arriving on its tracking - and its frames were read before landing: a
layout that clipped its own paragraph was caught by exactly that
discipline and restacked. Six tests hold the new surface to the
module's own bar, the static-transform guard included.

## 33. Scoped sub-compositions: the sting placed nine times, the scope kept

Geneva roadmap item 05. The `composition` node has said "define a
sting once, use it nine times" since the node table was written, but
nothing behind it was: the timeline mapping never emitted the
engine's `compositions` map, a placement's name resolved against
nothing, and the promise was a dead link. Now:

- **The document carries its comps**: `comps` maps a name to a whole
  comp document. A `composition` node places one - once or nine
  times, each placement with its own timing and transform. The name
  resolves **inside the same document**, which is the scoping rule:
  no reaching into another document's comps by id.
- **Each sub-comp is a scope**: its node ids, assets, curves and
  slots are its own; a parent and a child may both have a `card` and
  a `logo`; a diagnostic inside one is pointed at `/comps/{name}/...`.
  Cycles are refused at `validate` with a pointer into the document
  (the engine's E207 after, ours first), and so is nesting past the
  engine's eight levels.
- **The timeline flattens by scope**: the engine resolves composition
  names from one top-level map, so the mapping spells the sting's
  `logo` as `sting::logo` and an `outer` comp's sting as
  `outer::sting` - the document scopes, the timeline flattens, and
  neither knows the other's business. Staging copies every scope's
  files under the same spelling, with the same `.part`-then-rename
  discipline, and the bake's assets key covers them: changing the
  logo *inside the sting* is a different bake, measured by test.

A real bug found on the way, by rendering the thing: `source_of`
wrote an image node's `fit` **inside the timeline source**, which the
engine refuses as E101 (`asset` is the only field an image source
takes) - so every comp with an image node failed to bake through
`Geneva::bake` (the MCP `motion_bake` path and the agent CLI), while
the sequence path, which writes `fit` on the clip, was green. The
comp path now writes it on the clip too, and `examples/stings.rs` -
the worked proof: one definition, three placements, three sizes, the
third tilted, rendered with the pinned sidecar and read before
landing - carries an image inside its sting for exactly this reason.

## 34. The adapters: Lottie read as a document, GSAP written as tracks

Item 03 of the Geneva roadmap. `geneva-adapter` is a new workspace
crate — the first new crate since the fork's own motion lane — with one
rule carried through both of its halves: **nothing converts silently.**

The Lottie half reads a bodymovin document into a comp: solids,
images, shape layers (rectangles, ellipses, paths), shape groups with
their own transforms, keyframed properties with their bezier and hold
eases, layer parenting, in/out points, precomps nested to eight deep
with the depth limit the cycle detector. The GSAP half applies a
GSAP-style timeline — tweens on `x`, `y`, `scale`, `rotation`,
`opacity` and `blur` with `delay`, `stagger`, `repeat`, `yoyo` and the
named ease families — to a comp's nodes as tracks, `x` and `y` written
as one position track the way a position moves.

Everything the comp vocabulary has no answer for is skipped with a note
naming the layer and the feature — text layers, mattes, masks, trim
paths, gradient paints, repeaters, effects, 3D, hidden layers — and
everything that converts approximately is approximated *and said*: a
non-uniform scale takes the mean, a curved motion path straightens, an
animated shape size holds its first value, a group paint over several
shapes splits per shape, an elastic ease reads as the library's pop
spring. A conversion with no notes lost nothing the source document
said; a document whose every layer is skipped is refused with the
comp's own words, because an empty comp is not a conversion either.

Two order rules are pinned by the tests because they are the ones the
reference players render by: the first layer of a `layers` array is
the front, and within a shape list later items paint on top. Paths are
re-based to their own bounding box and the box placed where the source
drew it; they are drawn by the native engine only, as any hand-written
path is, and the bake says so per node.

The crate is deliberately dependency-light — `concat-motion`, serde,
serde_json, thiserror, nothing else — because an adapter that reads a
document should not pull a pixel library to do it. It carries 42
tests: every supported feature and every refusal has a fixture, the
output of every fixture validates *and lints* by the library's own
rules, and the GSAP fixtures are built through the Lottie adapter so
the two halves are proven against each other's output. A
`geneva-adapt` binary puts both verbs on the command line for the
agent and the curious; `docs/ADAPTERS.md` is the reference. The
adapters are not a player — a `.lottie` file already plays through the
Lottie engine — they are for when motion made elsewhere should become
document: editable, lintable, fillable by a slot, baked by whichever
engine the project prefers.

## 35. The farm's progress stream: a long render, said as it happens

Item 11 of the Geneva roadmap. `geneva farm` cuts a long render into
parts that survive their own failure, and until now the call was
silence: a forty-minute render blocked, and an agent watching it had
nothing to watch. The stream is the other half of the contract - every
state the farm passes through is one line of JSON, appended to
`progress.jsonl` in the farm's own directory as it happens, and read
back mid-render, from another process.

The events, in order: the plan (parts, frames, audio), each part's
start (its frame range) and landing or serving, the audio part under
its own name, the join, the done. One write call per event, the file
in append mode, nothing buffered between events - a progress stream
that only flushes at the end is a report wearing the name. A reader
racing the writer sees a torn tail and ignores it; a farm that has
said nothing answers a whole farm pending, because asking about a
render that has not started is a legitimate question with a boring
answer.

What the reader aggregates is the number an agent actually wants:
each part's state (pending, rendering, done, served), the frames
finished against the frames planned, the percent, the wall-clock
seconds the drawn parts cost, and an `etaSeconds` estimate from the
pace of the parts drawn so far - half the frames done at four seconds
a part says the other half is four seconds away.

The `motion_farm_progress` tool puts the reader at the MCP surface
(read-only, idempotent, nothing rendered, nothing written), and the
tool count the documents hold goes from ninety-five to ninety-six:
the README badge, FEATURES.md's inventory, FUNCUT.md, the Forge
architecture page, the allowlist in `.forge/mcp.toml` and the
allowlist's own count comments all say the new number, held there by
the count test that reads them. Seven tests over the stream itself -
the story read back whole, the pending farm, the torn tail, the pace
estimate, the live answer, the farm-level integration against the
stub engine - and the e2e arm that proves a farm which said nothing
answers live.

## 36. The scene read as an engineer reads it: `scene_analysis`, item 08

Item 08 of the Geneva roadmap. `motion_inspect` says what a comp holds;
`scene_analysis` says how well it is built - read off the document, with
no engine, no render and no write, so it costs nothing and refuses
nothing. It is the critique that happens before the pixels do.

The census: nodes by kind, the animated against the static, the
keyframes counted, the curves they ride named by their own CSS shape and
counted, the nesting depth walked parent by parent with the engine's own
32-deep cap, the effects. The timing: the first start, the last end, the
union of every node's window as the fraction of the comp's span anything
is on screen, and the gaps - spans of at least fifty milliseconds where
the comp draws its background and nothing else, the dead air of a motion
piece. The text: sizes against the frame height, the distinct fonts,
which nodes are karaoke by their own words, which carry plates, outlines,
shadows, gradient ink.

Per-text legibility is the part a render would otherwise have to tell
you: for every text node, the size as a percent of the frame height, and
a WCAG-shaped contrast ratio of the ink over the backdrop it actually
draws over - its own plate when the plate is opaque enough to matter,
else the comp's background, else honestly null, because a ratio read
against video nobody has seen would be a guess wearing a number. A
gradient ink answers with its worst stop: a ramp is only as legible as
its quietest moment. The verdicts are plain words - small (under three
percent of the frame height), low-contrast (under 3:1), both, or ok.

The assets are checked against the disk a bake would stage from - the
same folder the comp file lives in, so `present` is a fact, not an
expectation; a preset, which has no folder, answers `assetsChecked:
false` rather than pretending. And the lint's findings ride along with
their stable codes - every family the document and the disk can answer:
L1xx structure, L2xx timing, L3xx assets, L4xx motion, L5xx determinism,
each finding with its code, its place, its sentence and its help - the
lint had been a crate-internal discipline since it landed; this is its
door at the MCP surface, the fifth of the motion tools that need no
engine.

The tool count the documents hold goes from ninety-six to ninety-seven:
the README badge, FEATURES.md's inventory, FUNCUT.md, the Forge
architecture page, the allowlist in `.forge/mcp.toml` and the allowlist's
own count comments all say the new number, held there by the count test
that reads them. The e2e arm proves the sample analysis answers its four
numbers and its findings.

## 37. The motion watched: `motion_preview`, item 09

Item 09 of the Geneva roadmap. `motion_frame` is one exact moment; a
comp is a span. `motion_preview` is the span at a glance: `frames`
stills (1 to 12, six by default) spread evenly across the comp's
duration, each drawn small (width 16 to 640, 480 by default), answered
as one document of inline images in time order with each sample's time
and file - the entrance, the hold and the exit, for the cost of a
handful of frames instead of a bake.

The spread never lands on the dead last instant, where an exit has
usually already gone: the samples walk [0, duration) at even fractions,
so six frames of a six-second comp read 0.0, 1.0, 2.0, 3.0, 4.0, 5.0 -
the motion's own story, not its frame after the curtain. The count is
bounded because the point is a glance, not a flipbook; the width is
bounded because a preview that costs what a bake costs has missed its
own name. A comp without a duration is refused with the reason rather
than sampled six times at zero.

The reading the reply carries says what to watch for - the entrance
should land on its curve, the hold should breathe, the exit should
leave - and names its own borders: `motion_frame` for one exact time,
`scene_analysis` for the document with no engine at all, `motion_bake`
for the finished clip. The tool rides the render budget (300 s) with
the other frame-drawing tools, and its timeout place in the catalogue
says so.

The tool count goes ninety-seven to ninety-eight across every document
that holds it - the README badge, FEATURES.md's inventory, FUNCUT.md,
the Forge architecture page, the allowlist in `.forge/mcp.toml` and the
allowlist's own count comments - held there by the count test that
reads them.

## 38. The motion engine's own numbers: the timing benchmarks, item 15

Item 15 of the Geneva roadmap. The editor's benchmark
(`scripts/benchmark.py`) measures the machine's whole pipeline on a real
video; the motion engine's hot paths had no numbers of their own. Now
they do: five timing tests in `concat-motion`'s `bench` module,
`#[ignore]`d on the same contract the vision crate's
`time_a_frame_at_the_analysis_size` set — a timing assertion is flaky by
nature, so nothing fails a lane; run them by hand, in release, and read
what they print:

```sh
cargo test -p concat-motion --release -- --ignored --nocapture bench
```

The five, each earning its place: **the timeline document** (the pure
half of every bake - all eighteen presets turned into the JSON the
sidecar reads; the floor under the engine), **track sampling** (the
per-animated-property per-frame cost, with a spring and a steps curve on
the segments, because a linear-only track is the cheap case wearing the
expensive one's name), **the streaming hash** (the bake key's own cost
over a 128 MiB generated asset - generated, and saying so, because a
benchmark that pretends its file was a real clip is a boast), **the
comps sweep** (the LRU walk the bounded cache runs once a bake, over 48
manifested entries under a half budget), and **the noise field** (the
organic layers at the particle cap, 64 motes a frame plus drift).

The numbers a two-core sandbox printed on 3 November 2026 - a machine
weaker than the laptop this fork is for, so floors, not boasts: the
library's timeline documents in 0.3 ms, a track sample in 39 ns, the
streaming hash at 689 MiB a second (a 2 GB plate keys in about three
seconds, once), the 96 MiB sweep in 8 ms, and a frame of organic layers
under a microsecond. The shape they draw: the document layer is
microseconds, and the engine's real costs are the sidecar's frame draws
and the disk's hashes - which is where the farm and the cache spend
their effort.

`TESTING.md`'s skipped table carries the five by name (the six skipped
became the eleven), and `PERFORMANCE.md` carries the table of numbers
and how to run them.

## 39. The cloud farm: `motion_farm_cloud`, item 10 - the farm's legs on object storage

Item 10 of the Geneva roadmap, the last of it. The resumable farm
renders a long reel in parts that survive their own failure, and its
staged render directory is self-contained by design - the unit of work
a worker anywhere is handed whole. What it did not have was the
transport: the folders still had to reach the worker by NFS, SSHFS or a
synced copy. `farm-sync.py` - shipped as the MCP tool
`motion_farm_cloud` and runnable by hand - is the transport, on
S3-shaped object storage, written against nothing but the standard
library.

Four actions. `push` uploads a farm directory and its staged render
tree under one prefix, HEADing each file first and skipping the ones
whose stored sha256 metadata already matches - a re-push after a dead
worker costs what changed, not what existed. `pull` downloads them
back, atomically, skipping where the local size already matches.
`status` says what is remote, what is local, what differs. And `work`
is a worker's whole day: pull, render ONE missing part through the
sidecar with the farm's own atomic part-and-marker shapes, push the
part and its marker back, repeat until the answer says done - the
missing part found by the sidecar's own `plan` verb, asked for the
number the farm's progress stream names, and the marker written with
the joiner's own key: the FNV-1a of the staged timeline, computed here
exactly as `cache::stream_hash` computes it, or the part would be
re-rendered for nothing.

The wire is SigV4 over HTTPS - the standard AWS HMAC-SHA256 chain,
every request signed, the payload hash travelled as
x-amz-content-sha256 - and an upload streams a block at a time: the
file is hashed once, sent once, never held whole in memory, the same
megabyte-at-a-time rule the cache's own hashes follow. `FUNCUT_FARM_ENDPOINT`
names an S3-compatible host (MinIO, R2, a self-hosted one), so the
farm is not married to one company's storage.

The security posture is the one the VLM endpoint allow-list set:
everything the storage needs - the bucket, the endpoint, the region,
the credentials - comes from the environment the OPERATOR set, and
never from a tool's arguments. A prompted agent cannot aim a render,
or the credentials beside it, at a host the operator did not choose.

The selftest proves the whole thing with no network: a mock S3 that
VERIFIES the SigV4 signature of every request it serves (the same chain
recomputed from the request's own bytes), the sha256 skip on a re-push,
the FNV-1a's three published vectors, the part key's shape, and the
whole `work` loop - pull, three parts drawn one a call through a stub
sidecar, the fourth call serving, the joiner's pull landing everything.
Eight checks, wired into the CI lane beside the sense selftests. The
tool count goes ninety-eight to ninety-nine across every document that
holds it, held there by the count test that reads them.

## 40. The GPU encode door: the overlay on the machine's own encoder, item 06

The editor's compositor has been wgpu - GPU work, always - since the
fork began; item 06 was the motion chain's own GPU path, and the
overlay is that chain's one encode (an overlay of two planes is trivial
next to it).

`gpu` is a field on `motion_overlay`: `auto` (the default) picks
`h264_vaapi` where a VAAPI driver and a render node exist - the
finished frame uploaded once as nv12, the encoder fed in hardware;
`on` asks for it and falls back to libx264 **with a note that says
so**, because a clip that encodes beats a refusal that does not; `off`
stays in software. `FUNCUT_OVERLAY_ENCODER` is the operator's pin and
`FUNCUT_VAAPI_DEVICE` names another node; the reply says which encoder
ran, so the crf number is read for what it is (a QP on the vaapi path).
Presence is honest: the encoder in ffmpeg's list AND the node behind
it, or it is not a GPU this call can reach.

The pick is a pure function of what the machine has, what the call
asked for and what the operator pinned, and the selftest proves its
precedence table and both command shapes without a GPU in the room.
Verified end to end on the no-GPU sandbox: both the plate and the
colour branches, the fallback note in the reply, a valid 30-frame h264
out each way.

The docs tell the truth they had stopped telling: PERFORMANCE.md said
the wgpu compositor was "optional, off by default, behind
concat-render's `gpu` feature (Vulkan, Metal or DX12)" - it is always
built (the feature is an empty shim), and this fork is Linux-only, so
Metal and DX12 were never ours to promise.

## 41. The findings register, closed

The 1 November audit's ten findings, worked through on a machine with
the pinned toolchain: the shared `.part` in a staging directory
(`cache::part_path`, unique per writer), the scratch the frame verb
leaked when no `-o` was given (always copied out, always removed), the
exec bit discovery never checked (`runnable()` + `Geneva::pick`, with
a regression test that names no environment), the authored `at` that
flattened a choreographed cut in silence (the plan warns now, tested
both ways), and the words that overstated - the cache door's comment
and `BakeRequest::alpha`'s doc now say exactly what each moves.
Findings 2, 5, 8 and 9 were resolved before this pass (the postscript
in `audit/2026-11-01/geneva-four-pillars.md` says so, item by item).
379 lib tests and 6 doctests, clippy `-D warnings` clean.

## 42. The voice read as a score: `voice_signals`, item 11 - the sixth sense

The emotion read told the director what every line FEELS; nothing told
it what the voice IS. Six signals the editor already computed were
never wired to the edit (whole-file loudness, raw silence spans, a
derivable speech rate, whisper's word timings used raw, a language the
transcriber knew and the fonts never saw, an arc that warned but never
paced), three more were a regex away (sentence type, repeated motifs,
fillers), and the rest needed real extraction. This adds the
`voice_signals` tool: the Python half (sense/voice_signals.py, pure
stdlib, one ebur128 pass for per-line momentary loudness, silencedetect
spans CLASSIFIED breath/beat/dramatic/scene_turn/dead_air, speech rate
with a pace class, a measured word-confidence gate, script detection
with font and music-genre hints, sentence types cross-checked against
the prosody's terminal rise, motifs, fillers, and a regex complement
for the bare years and money the NER misses) and the Rust half (the
`voice-signals` crate: spectral-prosody for pitch, emphasis and rhythm,
oximedia-audio-analysis for formants, noise floor, SNR and flux,
polyvoice for VAD and voice-print diarization, anno for offline NER,
WhisperX bridged when installed). The director skill grew a Phase 1c
and an "A - the voice is a score" law: only dead air is cut, a dramatic
hold survives, SFX peaks land on emphasis words, the BGM obeys the
measured noise floor, and a speaker change is staged like a scene
change. polyvoice is pinned at 0.17 for the pinned toolchain's MSRV;
the diarization says in its own `method` that it is a voice-print
heuristic, not a neural model. 23 new tests in the crate, the
concat-mcp suite green end to end (36 lib, 7 e2e) on the CI dependency
set, and both selftests - the crate's synthetic two-voice take and the
sense script's - answer. Full map: docs/VOICE-SIGNALS.md.

## 43. The consumption layer: thirty-three tools in five families, item 12 - what the director DOES with the score

`voice_signals` (section 42) is the extraction layer: what the voice
contains. Thirty-three proposed tools were listed across the work and
never built - the consumption layer, what the director does with the
readings. All thirty-three now exist, in five pure-stdlib Python
families served through one table-driven door in sense.rs
(`Action::Sense(name)`): **discourse.py** (the script as language -
analyze_discourse, segment_topics, detect_speech_acts,
classify_speech_acts, score_readability, extract_claim_graph,
score_script_quality, detect_sarcasm, detect_code_switching,
detect_cross_lingual_emotion), **voice_quality.py** (the take's health -
detect_vocal_events, classify_voice_quality,
classify_recording_quality, measure_reverb, detect_restarts,
classify_filler_type; one 16 kHz mono decode per call, a pure-Python
radix-2 FFT where a spectrum is needed, numpy used when installed and
never required), **prosody_style.py** (the delivery - score_emphasis_type,
detect_speaking_style, measure_rhythm_meter, detect_tempo_changes,
detect_accent, measure_vocal_tract), **audio_repair.py** (the clean and
the music - enhance_speech, remove_hiss_hum_rumble, normalize_loudness
write a NEW file with every filter decision measured from THIS take and
returned with its why; detect_music_segments, identify_music,
analyze_music_structure read: BPM, the Krumhansl-key chroma, bar grids,
sections, drops and cutSafePoints), and **captions.py** (the words and
the take made to agree - evaluate_tts_quality, detect_ai_voice,
seal_voice, align_captions, clean_subtitles).

The honest-degradation contract holds everywhere: identify_music says
naming a song needs an online fingerprint service and hands back the
libraryQuery that IS the bgm_search; detect_accent exists for caption
pacing and says in its own reply that it is not a verdict on the
speaker; detect_ai_voice is a disclosure heuristic that states what
modern TTS can pass; classify_voice_quality names its jitter and
shimmer as autocorrelation proxies; remove_hiss_hum_rumble refuses to
write anything when the take is cleaner than the cure. The catalogue
carries **133 tools** (every doc, the README badge and the Forge
allowlist held to it by the tests that already did), the tool-list
context budget reviewed to 330 kB at 320,624 measured, CI runs the
five family selftests beside sense.py's own, and the whole chain is
proven end to end through the real MCP binary: 36 lib tests, 7 e2e
tests, ten sense-module selftests, and a 12-point protocol probe
(handshake, tools/list, and one call per family including the
seal/verify round trip and a loudness pass that writes its file).

## 44. The usable surface: descriptions to one line, five family doors, the director's default, and the decision log

Section 43 shipped 133 tools and a 320,624-character tool list - most of
that description prose (75,611 characters, 569 a tool) rather than the
parameter schemas that carry the arguments. No model holds that plus a
conversation plus a task. Four changes made the surface one an agent can
actually use, with nothing lost:

- **Every description is one line** - what a tool does and when to reach
  for it, under 200 characters, held by a test. The parameters, their
  units and their traps live in the schema (where an agent reads them
  when it calls) and the deep semantics in the director skill's
  `prompts/tool_reference.md`, where they always did: 75,611 -> under
  9,000 characters.
- **The thirty-three consumption-layer readings moved behind five family
  doors** - `sense_discourse`, `sense_voice_quality`,
  `sense_prosody_style`, `sense_audio_repair`, `sense_captions`, the
  `action` argument naming the reading (`sense_discourse {action:
  "score_script_quality", script}`), `selftest: true` proving one. 133
  tools became **105**; every reading survives, and a member named
  through the wrong family is refused with the family's real members
  listed.
- **`tools/list` now serves what the allowlist serves** - a tool off the
  list is neither listed nor callable, as if it were not there. The
  default an operator gets naming no list is the **director's surface**,
  95 of the 105: the creative loop the skill documents plus the remedies
  its own refusals point at. The ten left off (project_close,
  export_cancel, the farm pair, the specialist motion reads,
  track_subject, geneva_explain) are deliberate opt-ins;
  `CONCAT_MCP_ALLOW=all`, `--mcp-allow all` and the new `--full-tools`
  flag serve the whole catalogue.
- **The reading is now consumed**: `voice_signals` answers
  `signalConfidence` (one confidence and basis per signal family -
  formants on a phone-band take, reverb ±20% on a single channel,
  emphasis reliable) and `decisionPolicy` (which signals drive each cut,
  caption, graphic, BGM and transition decision, in priority order);
  the plan carries a **`signalLog`** - one entry per decision naming the
  signals behind it - and `plan_validate` checks it against the
  vocabulary, reports coverage, and refuses creativeReady to a plan with
  a voiceover and no log. The director skill's Phase 1c and Phase 3 and
  the canonical edit_plan.schema.json (v2, with signalLog) teach the
  loop.

Measured after: the whole catalogue 233,617 characters over 105 tools
(the cap follows, at 240 kB), the director's default 220,003 over 95
(its own 230 kB cap), the description guard at 200 characters a tool.
The crate: 40 lib tests, 7 e2e, ten sense-module selftests, clippy and
rustfmt clean, and the chain proven live through the real binary - a
family dispatch per door, the schema refusal naming a wrong family's
members, and a `voice_signals` extract answering `signalConfidence`
(13 families) and `decisionPolicy` (5 decisions) on a synthetic take.

## 45. The footage layer: the plan's slots, the search that takes a description seriously, the gate that reads the cut, and the revision that keeps its wins

The sound side of the director was done; what the picture did between the
words was still the agent's guess. The footage layer closes that, in five
moves, each one branch and one pull request:

- **The engine** (`concat-motion`'s `footage` module, `oximedia-auto =0.1.8`
  pinned patch-exact like `ort`): `suggest_footage(beats, transcript)`
  reads a beat grid and a transcript and returns a `FootagePlan` - the
  dialogue gaps the edit leaves uncovered (as the crate's own
  `CoverageGap`, carrying the topic tags the words either side asked
  for), the monotony runs it drones over, and the J/L cuts the dialogue's
  own boundaries propose. Gap scoring, colour continuity and cut
  positions come from the crate, wired not rebuilt.
- **The Slot schema** (`concat-motion`'s `plan` module, the canonical
  `edit_plan.schema.json`, and `plan_validate`): a plan may carry
  `slots` - slotId, role (the seven acts), treatment (hold, broll,
  graphic, black), job, entry, exit, the rich description CLIP ranks
  against, the fallback queries, hero, motionMin, durationS. The
  validator's laws: a description under 40 characters is refused, a
  drawn treatment takes no queries, b-roll takes at least one, and the
  b-roll share keeps the 30-45% band - warned past 45%, refused past
  60%. A plan without slots is a plan the segments alone describe.
- **The search takes a description seriously**: `openvid_search` and
  `openvid_search_batch` gain `excludeIds` (snake_case `exclude_ids`
  accepted) and the motion floor - both filtering before the ranking,
  an excluded id not ordered lower but not there, the clause one NOT IN
  with a bind an id, refusing past 500 rather than quietly forgetting.
- **`openvid_rank_for_slot`**: rank a page against a slot's rich
  description - BM25 builds it, the MobileCLIP2-S0 second pass re-orders
  it by what the clips show, the queries blend in as Jaccard similarity
  at `tag_weight` (0.3 default), the hard filters enforced twice (they
  ride the request and are checked again on what lands), a hero slot
  asking for thirty and any other for twelve, every clip carrying its
  `slotScore` and `slotRank`. An empty description is an error that says
  why, not a quiet fallback.
- **The gate and the revision**: `footage_continuity` reads a filled
  manifest the way an editor scrubs - colour jumps past the half-scale
  threshold, reversed travels, repeated scales, one-category runs,
  clips under their own slot's floor - each flag naming its slot, rule
  and remedy (search again with an avoid-hint, or the treatment
  downgrade: b-roll to graphic for a rhythm problem, b-roll to hold for
  stillness the slot can own). `concat-motion`'s `revision` module is
  the director's loop's other half: at or above 0.65 the edit ships,
  below it exactly the slots the critic named go back to the planning
  step while two revisions last, every attempt rendering to its own
  `edit_v1`/`edit_v2`/`edit_v3.mp4`, and after that the edit ships
  exhausted with the failures recorded per slot.

The catalogue grows to **107** tools, and the director's default grows
with it, to **97** - the two footage tools join the loop they serve,
on the allowlist and in the forge config, and the ten left off are the
ten the policy always named. The canonical plan schema, the forge
config, FEATURES.md and this ledger say so in the same breath.

## 46. The director learns the footage layer: slots in the plan, the slot's own search, the gate before the fetch, and the revision that keeps its wins

The layer's engine, schema, search and gates had all landed, and the
director's own instructions still taught the keyword path: shop by
query, judge by eye, revise by re-planning. The tools were on the
allowlist and in the catalogue, but the skill that runs the loop never
said a word about them - the documentation had advanced further than
the director's actual orders. This change is the skill catching up to
its own tools.

- **The plan writes its slots.** SKILL.md's Phase 3 now teaches the slot
  list beside the segments - the seven-act role, the treatment (only
  `broll` shops), the job, entry and exit against the words and the
  beat grid, the rich description of twelve to twenty-five words, the
  fallback queries, `hero`, `motionMin`, `durationS` - and the
  validator's laws (forty characters, no queries on a drawn treatment,
  the 30-45% b-roll band) are the skill's laws too. The edit-plan
  playbook carries the same fill order, FEATURES.md's phase list says
  the same words, and the hard V floor now says the tools hold the
  variety law for a slotted plan.
- **The search is the slot's.** Phase 4 shops each broll slot with
  `openvid_rank_for_slot` - the description verbatim, the hard filters
  (`motion_min`, `exclude_ids`) riding the request and enforced again
  on the page, every chosen id carried onto the next slot's exclude
  list so the variety law is held by the tool - and
  `broll_matching.md` gains the shopping-by-the-slot section beside its
  keyword laws. The tool reference documents both footage tools'
  arguments, defaults and gotchas.
- **The gate runs before the fetch.** The filled manifest - motion from
  the clip's own measurement, direction from its `camera`, scale and
  category from the slot's vocabulary, colour read off the contact
  sheet - goes to `footage_continuity`, and each flag's remedy is the
  work: re-shop the slot with the avoid hint, or take the downgrade and
  let the comp or the A-roll carry the span. `count: 0` is the clean
  answer; nothing is fetched until it is.
- **The revision keeps its wins.** Phase 7's loop now follows the
  revision gate: below the 0.65 ship line, exactly the slots the critic
  named re-shop (everything else stands), each attempt renders to its
  own `edit_v1`/`edit_v2`/`edit_v3.mp4`, and after two revisions the
  edit ships with the failures recorded per slot - shipping with a note
  beats not shipping. The skill's own 7 bar stays; the gate is the
  floor beneath it.

The README's version badge moves to 0.2.8-funcut.10 with the release it
names, and TESTING.md's count moves to the `564acdbe` snapshot that
release was cut from: **1725** run and passed with the same fourteen
skips, **8** documentation tests, **30** gRPC - a Rust total of
**1763**, sixty-two over the 2026-11-04 follow-up's 1701, all of them
the footage layer's.
