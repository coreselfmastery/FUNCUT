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
beds with one idea. The beat grid comes from `analyze_beats` on the fetched
track: `beatsMs[]` and `downbeatsMs`.

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

## Ducking (dB-aware clip automation)

Use `clip_audio_levels_db` on the BGM clip after `bgm_import` (get its actual
`clipId` from the import result or `timeline_read`). `keyframe_set` and
`clip_update.patch.volume` use linear gain multipliers, not dB; do not pass
`duckDb` to those fields. `clip_audio_levels_db` converts the plan's absolute
dB levels to the engine's linear values in one undoable batch.

- Set `baseDb` to the bed's resting level (typically −4 to −6 dB).
- Supply absolute `gainDb` targets: speaking starts at `duckDb` (−12 default;
  −9 tight/motivation, −15 calm/history); a narration gap ≥1.5 s returns to
  the resting level. Use `ease [0.25,0.1,0.25,1]` at the boundaries.
- Keep music at the resting level for the first 1–2 s before the first word
  and the last phrase when the edit calls for an intro swell/outro ring.
- Use `replaceAutomation: true` only when replacing the full existing volume
  curve; it clears old volume keys before writing the new curve. Targets are
  absolute dB levels, not offsets from `baseDb`.

Compute `at` as a 0–1 fraction of the BGM clip's length. Use measured
transcript-segment times or other accurate sync data; the current `transcribe`
tool does not supply per-word timings.

## Levels

- BGM resting: −4 to −6 dB; ducked: typically −12 dB absolute clip level (adjust by ear against the narration).
- Bed changes when the WORLD changes (an ambience-style track) or never (a
  through-score). Pick one approach per video.
- If the platform specifies a loudness target (for example, −14 LUFS), record
  it as a goal, not a verified result: export does not measure integrated
  LUFS/true peak, and `concat.loudness` is not a master-bus guarantee. Measure
  the final exported mix externally before claiming compliance.

## Self-check

- [ ] One track, one mood; loop-friendly; no vocals under speech.
- [ ] Sections land on downbeats (or speech won, within 300 ms).
- [ ] Duck automation has no hard jumps (all eased ≥ 250 ms).
- [ ] Music rides up in gaps ≥ 1.5 s; full at hook in and payoff out.
- [ ] Last beat rings; nothing clips.
