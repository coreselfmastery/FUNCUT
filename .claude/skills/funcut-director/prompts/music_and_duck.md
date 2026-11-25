# Music and ducking

Music is structure you can hear. This file is how it is chosen, placed
and automated.

## Choice

**First, the gate.** Before any `bgm_search`:
`sense_audio_repair {action: "detect_music_segments", audio: <the
voiceover>}`. `musicPresent: true` means the take already carries music —
no bed is searched, fetched or imported on top of it (duck or replace,
never stack; the reply's `editingAnswer` states the call), and the
copyright check runs:
`sense_audio_repair {action: "identify_music", audio}` describes the
embedded track (tempo, key, mood — an offline descriptor, never a title
match) while you warn the user that its licence is unknown. A dry take
(`musicPresent: false`) is the only take the rest of this page is for.

`bgm_search` (shelves: freepd 1,025 CC0 tracks; lofi 166, free for
non-commercial use only; a search that
names no shelf searches whichever are ready). The words come from the
read, not from you: every line of the Phase 3 extract carries
`bgmWords` — two queries its emotion and act want, the second a shelf
category the search always matches (`upbeat`, `epic`, `horror`,
`scoring`…). Start from the arc's dominant line's `bgmWords[0]`; if the
shelf answers thin, `bgmWords[1]` is the guaranteed hit — the chain is
the emotion's three-word primary (`horror suspense tension` for fear,
`scoring melancholy piano` for sadness) bent by the act (`energy` on a
hook, `triumphant` on a reveal), then the bare shelf category. What
this page adds on top of the code's words:

- **Register**: finance/tech → keep the code's words but lean minimal
  keys; motivation → building orchestral or hybrid; documentary →
  sparse piano/strings; history → era-adjacent; gaming → synth energy.
  A register nudges the choice among candidates, it does not override
  the emotion's words.
- **Length vs runtime**: the track must outlast the piece or loop
  gracefully; `maxSeconds` filters at search.
- **Vocal-free** for narration pieces, always.

Listen with the numbers the search returns (category is the shelf's own
word — "upbeat", "late-night", "scoring") — that is the FIRST pass,
title and category words only. The second pass measures the shortlist:

- **Rank the candidates by what they sound like.** A bed wants sustain
  and tonal body, not transients: `sense_sound {action: "rank_sounds",
  role: "emotional_texture", paths: [<the fetched top candidates>]}`
  scores each file's measured fit, and when the sound models are
  installed `sense_sound_model {action: "rank", index, query: "<the
  arc's own words, two or three — 'tense building orchestral'>"}`
  scores CLAP similarity against the description the read actually
  gave. A candidate whose category says "epic" but whose measurement
  reads thin and airy is a transition, not a bed — the ears overrule
  the shelf's words.
- **Read the structure BEFORE placing, not after.**
  `sense_audio_repair {action: "analyze_music_structure", audio}` on
  the CHOSEN track returns the BPM, the bar grid, sections with roles,
  the drops, the energy curve and `cutSafePoints` — and that map
  decides where a section change may land musically. Feed the same
  map to `sense_sound_model {action: "plan", bgm: {candidates: [...]}}`
  and the planner puts the bed's change points on safe cuts and ducks
  around the voice's own emphasis. Choosing first and analyzing later
  leaves the structure unused; the map comes first.

Trust but verify with one `preview_frame` of the mix at a speech
moment.

### Rank the audio, not the title

A title that says "epic" and a build that never drops are two different
tracks. When the choice matters — the hook's bed, a longform spine, two
candidates the words cannot separate — `bgm_fetch` the top two or three
and READ them before importing: `sense_sound_model {action: "describe",
file: <the fetched track>}` answers the semantic question (a CLAP
embedding against the bed's own words — does this track SOUND like
"tension building under a reveal" whatever its title claims?) and
`sense_audio_repair {action: "analyze_music_structure"}` answers the
structural one: the sections and their roles,
the energy curve, the drops, and `cutSafePoints`, the moments where a
cut lands musical. Choose on the curve and the sound, not the caption — the track
whose build climbs where the tension act climbs and whose drop sits
where the reveal sits is the right bed whatever its title says. The
`cutSafePoints` also pre-answer chapter alignment (below): a track with
a safe point near every section boundary is a track the grid can use.
(The shelf's bytes arrive when fetched, by design — so the read is per
candidate, and the rejected candidates cost one fetch each, not a
whole-shelf download. A run that shops hard can index the fetched
folder once — `sense_sound_model {action: "index", dir: <the project's
bgm folder>}` — and `rank` it by the bed's words after.)

## Placement and the grid

`analyze_beats` on the chosen track gives `{bpm, beatsMs[],
downbeatsMs[]}` — the beat grid. The deeper map — sections with roles,
drops and `cutSafePoints` — comes from `sense_audio_repair {action:
"analyze_music_structure"}` at Choice time (above); use both. Align:

- Section changes to DOWNBEATS (compute the nearest downbeat to each
  section boundary; nudge the boundary or the track's start by the
  difference — the narration's trims flex a few frames for this).
- The end lands ON a downbeat; a piece that ends mid-beat sounds
  unfinished even when nobody knows why.
- In reels, the cut grid can BE the beat grid (cutType `beat`).

## Ducking (the automation)

`clip_audio_levels_db` — absolute dB (0 unchanged, −60 floor):

- Resting bed: −4 to −6 dB.
- Under speech: **−12 dB absolute** (−9 tight, −15 relaxed).
- Ride back up in gaps: keyframes to −6 through the breath between
  sentences.
- Section dips: −14 to −18 under the re-hook, resolve to resting at the
  outro.

Keyframes as `clip_audio_levels_db {path, clipId, baseDb, keyframes: [{at, gainDb, ease}]}` (`path`, `clipId` and `baseDb` are required beside the keyframes)
with `at` a fraction of the clip — a duck that anticipates the speech by
~150 ms (the key BEFORE the word) sounds engineered; one that lands
after sounds like a mistake.

## Longform structure (the music has chapters too)

One track can carry eight minutes, but it should not carry them flat:

- Build across chapters: resting level rises a dB per chapter, or a
  second track enters at chapter 3.
- A bed may change at a chapter boundary — multi-section arrangement
  is allowed on long cuts. The change lands on a `cutSafePoint` of the
  outgoing track (`analyze_music_structure`) or a downbeat
  (`analyze_beats`), never mid-phrase: a bed that swaps on an unsafe
  point sounds like a mistake nobody can name.
- The mid re-hook (50–60% dip) gets a dip-and-swell: bed to −15 under
  the promise, then up.
- The outro resolves: either the bed's own resolution bar or a fade to
  the outro card's signature sound.
- Chapter changes can re-drop the track (a beat of silence into the
  chapter chip's sound, then the bed returns) — the strongest structural
  signal music can send short of stopping.

## After the automation, measure the mix

Keyframes are a plan; the mix is a fact. After the duck automation,
`measure_loudness` on the rendered mix — with `watch_render {audio:
true}` for the envelope — checks what the bed actually does:

- **Space under speech**: the voice sits clear on top of every spoken
  span; masking is the defect the duck exists to prevent.
- **Build in the pauses**: the bed may rise through a dramatic hold —
  the one place it is allowed to be loud.
- **Dip before a reveal**: a small fall (−2 to −3 dB) into the reveal
  makes the impact land harder than the impact alone.
- **Resolve at the outro**: the bed resolves — its own resolution bar,
  or a fade under the outro card's signature sound — it never simply
  stops. A bed that ends with the picture is a render error with a
  soundtrack.

A flat envelope across a whole piece is the audio's version of a flat
cut grid. Fix it with keyframes, re-measure, then export.

## Licensing

The two shelves carry two different licences, and the difference
matters. **FreePD** (1,025 tracks) is whole-track CC0 — Kevin MacLeod's
public-domain library; attribution welcome, never required. **Lo-Fi**
(166 tracks) is Moby's MobyGratis — free for **non-commercial use
only**: MobyGratis's own licence governs what Moby granted, so never put
one of these tracks in monetized or client work, and credit
"Moby (MobyGratis)" where one is used. The third source refuses
itself: `sense_sound_model {action: "generate", kind: "bgm"}`
(TinyMusician) ships no licence file upstream, and the action answers
`model: "refused"` on a monetized brief — a monetized project never
carries a generated bed, and the run shops FreePD instead. The refusal
is the tool's, not yours to override. A bed that does come back, on a
non-monetized cut, is placed by `bgm_import {file}` like any shelf
track. A voiceover that already carries
music is a copyright question, not a bed: `identify_music` describes
the track, the user is warned before publishing, and the licence is
theirs to confirm. Stickers are Fluent Emoji, MIT — usable in
commercial work; every magick_search result says so in its license field.
