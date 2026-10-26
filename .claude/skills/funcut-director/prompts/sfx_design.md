# SFX design

Sound effects are punctuation. One per cut for energetic pacing, only where
they add information — an SFX on every single cut in a calm edit is noise.

## The vocabulary (12 real categories)

`Whoosh` (transitions, camera moves) · `Impact` (hits, reveals, text pops) ·
`Riser` (build into a cut) · `Motion` (slides, ticks, mechanical) · `UI`
(clicks, pops — great for text cards) · `Notification` (dings, alerts —
finance/tech) · `Keyboard` (typing montages, how-to) · `Digital` (glitches,
data — tech) · `Foley` (real-world: doors, steps, paper — history, how-to) ·
`Writing` (pens, markers — history, listicles) · `Ambience` (beds: rooms,
weather, crowds) · `Music` (stingers, notes)

## The peak rule

`sounds_add` starts the file at `start`. Peaks go ON the cut — the frame
the new visual appears — never after. The search result's `syncAt` is where
each sound's peak lands inside its file: place the peak on the cut by
starting the file `syncAt` seconds before it (`start = cut - syncAt`). A
riser must ARRIVE at its cut: it starts 1–2 s earlier and its peak is at the
boundary. An impact
peaks exactly at the reveal. ±1 frame; if you must choose, one frame early.

## The design pass (per segment, from the plan)

| Segment type | Sound |
| --- | --- |
| Hook (0–3 s) | Riser→Impact combo into the first cut, −8 dB |
| B-roll cut, calm | Soft Whoosh −16 dB or nothing |
| B-roll cut, energetic | Whoosh or Motion tick −12 dB |
| Text card pop | UI click or Impact −10 dB, peaked at the pop |
| Number reveal | Impact −8 dB |
| Section transition | Whoosh peaked on the transition |
| Ambience | One bed per WORLD (ocean, city, archive) at −20 to −24 dB, cut when the world changes |

## Levels (gain staging)

- Speech sits at full scale: VO is the loudest element, always.
- SFX −8 to −16 dB relative to the VO at the moment they fire (hook hits −8;
- deep-texture beds −20).
- BGM under speech: −12 dB (see `prompts/music_and_duck.md`).
- The one loudness effect the engine has is the `concat.loudness` filter
  ("Normalize Loudness") — apply on export only, never mid-edit, and never
  twice. Target −14 LUFS in the plan's export notes; do not stack gain effects
  on top.

## Search recipe (sounds_search)

Category first, then the object: "Whoosh fast", "Impact boom deep", "Riser
tension", "Keyboard mechanical", "Foley paper rustle", "Ambience ocean
waves". Keep queries two or three words — the 430-sound library is small
enough that "Riser" alone is already browsable. If a search misses, go
CATEGORY-only and pick by name from the results rather than inventing sound
ids.

## Rules

1. Never two SFX at the same instant (a whoosh AND an impact ON the same cut
   → pick one, or offset the whoosh to peak 100 ms earlier).
2. Every transition has its sound; every text pop can have one; every cut
   does not NEED one.
3. Ambience beds are worlds, not wall-to-wall muzak: cut them when the scene's
   world changes.
4. If a planned SFX is missing from the library, drop it — silence is a
   choice, a wrong SFX is a mistake.
