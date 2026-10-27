# Music and ducking

Music is structure you can hear. This file is how it is chosen, placed
and automated.

## Choice

`bgm_search` (shelves: freepd ~1,000 tracks, lofi 166; a search that
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

Both shelves are whole-track CC0 (FreePD is Kevin MacLeod's public
domain library; Lo-Fi is Moby's MobyGratis instrumentals). Attribution
welcome, never required. MAGICK stickers are the ones with the NC clause
— those are pictures, not music, but the same rule applies there.
