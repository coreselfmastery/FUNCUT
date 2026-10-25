# Music and ducking

BGM is the emotional bed, not a fifth wheel. `bgm_search` reaches FreePD
(1,025 Kevin MacLeod tracks, CC0 — categories like upbeat, epic, scoring,
comedy, world) and the MobyGratis/OpenLo-Fi pack (166 tracks — chillhop,
late-night, ambient-lofi). Sorts: `relevance`, `shortest`, `longest`,
`surprise`.

## Choosing per niche (mood → query)

| Niche | Query ideas | Register |
| --- | --- | --- |
| History | "scoring documentary tension", "epic drums orchestral" | orchestral, low |
| Tech | "synth pulse minimal", "electronic ambient" | pulsing, mid |
| Luxury | "elegant strings", "cinematic piano" | slow, lush |
| Motivation | "epic inspiring build", "driving rock" | rising |
| Finance | "tension pulse", "corporate upbeat" | tight |
| Science | "ambient wonder", "soft pads exploration" | airy |
| Horror | "dark drone tension", "creepy atmosphere" | drone, low |
| Fitness | "workout electronic drive", "hype beats" | driving |
| Gaming | "electronic arcade energy", "synthwave" | bright, fast |
| How-to | "ukulele light", "clean background" | friendly, quiet |

Rules: the track must survive 50+ listens (you will hear it every export) —
avoid earworm melodies in the top octave under speech; prefer instrumental
beds with one idea. `tempo` from the search result is your beat grid source:
`analyze_beats` on the fetched track for `beatsMs[]`.

## Beat alignment

1. `bgm_fetch` → `bgm_import` → `analyze_beats` on the imported track.
2. Downbeats ≈ every 4th beat from `beatsMs[]`. When `bpm` ≥ 90, half-time
   (every 8th) for section changes.
3. Move section boundaries (segment group starts) to the nearest downbeat
   when the drift is ≤ 300 ms — if the narration would have to stretch more,
   SPEECH WINS and the music simply lands near the beat.
4. SFX peaks stay on their cuts (cuts already sit near beats if you aligned
   sections).
5. End on a downbeat: let the last chord/beat ring 0.5–1 s after the final
   word.

## Ducking (the one automation you hand-keyframe)

Music under speech: `keyframe_set {property:"volume"}` on the BGM clip's
audio:

- Speaking starts: ease volume to `duckDb` (−12 default; −9 tight/motivation,
  −15 calm/history) over ~300 ms (`ease [0.25,0.1,0.25,1]`).
- A narration gap ≥ 1.5 s: ease back up to −4 dB over ~500 ms — the bed
  breathes between sentences (the silence pass left those breaths in — use
  them).
- Hook and payoff: music at FULL for the first 1–2 s (before the first word)
  and the last phrase — the intro swell and outro ring are the emotional
  bookends.

Compute the keyframe `at` values as fractions of the BGM clip's length
(`at` is 0–1 of the clip), from the word timings.

## Levels

- BGM full: −4 to −6 dB; ducked: −12 dB (relative, under speech).
- Bed changes when the WORLD changes (an ambience-style track) or never (a
  through-score). Pick one approach per video.
- Export loudness norm −14 LUFS via the `concat.loudness` filter at export
  only (see `prompts/sfx_design.md`).

## Self-check

- [ ] One track, one mood; loop-friendly; no vocals under speech.
- [ ] Sections land on downbeats (or speech won, within 300 ms).
- [ ] Duck automation has no hard jumps (all eased ≥ 250 ms).
- [ ] Music rides up in gaps ≥ 1.5 s; full at hook in and payoff out.
- [ ] Last beat rings; nothing clips.
