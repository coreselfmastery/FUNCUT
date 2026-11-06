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
names no shelf searches whichever are ready). Match by:

- **Niche register**: finance/tech → clean electronic, minimal keys;
  motivation → building orchestral or hybrid; documentary → sparse
  piano/strings; history → era-adjacent; gaming → synth energy.
- **Length vs runtime**: the track must outlast the piece or loop
  gracefully; `maxSeconds` filters at search.
- **Vocal-free** for narration pieces, always.

Listen with the numbers the search returns (category is the shelf's own
word — "upbeat", "late-night", "scoring"); trust but verify with one
`preview_frame` of the mix at a speech moment.

## Placement and the grid

`analyze_beats` on the chosen track gives `{bpm, beatsMs[],
downbeatsMs[]}`. Align:

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

Keyframes as `clip_audio_levels_db {keyframes: [{at, gainDb, ease}]}`
with `at` a fraction of the clip — a duck that anticipates the speech by
~150 ms (the key BEFORE the word) sounds engineered; one that lands
after sounds like a mistake.

## Longform structure (the music has chapters too)

One track can carry eight minutes, but it should not carry them flat:

- Build across chapters: resting level rises a dB per chapter, or a
  second track enters at chapter 3.
- The mid re-hook (50–60% dip) gets a dip-and-swell: bed to −15 under
  the promise, then up.
- The outro resolves: either the bed's own resolution bar or a fade to
  the outro card's signature sound.
- Chapter changes can re-drop the track (a beat of silence into the
  chapter chip's sound, then the bed returns) — the strongest structural
  signal music can send short of stopping.

## Licensing

The two shelves carry two different licences, and the difference
matters. **FreePD** (1,025 tracks) is whole-track CC0 — Kevin MacLeod's
public-domain library; attribution welcome, never required. **Lo-Fi**
(166 tracks) is Moby's MobyGratis — free for **non-commercial use
only**: MobyGratis's own licence governs what Moby granted, so never put
one of these tracks in monetized or client work, and credit
"Moby (MobyGratis)" where one is used. A voiceover that already carries
music is a copyright question, not a bed: `identify_music` describes
the track, the user is warned before publishing, and the licence is
theirs to confirm. Stickers are Fluent Emoji, MIT — usable in
commercial work; every magick_search result says so in its license field.
