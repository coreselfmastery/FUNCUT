# Sound design (SFX)

Sound is half the edit and the first thing an amateur edit skips. The
rules are short because the discipline is short: variety, placement,
restraint.

## The anti-repetition law

**Never the same sound id twice within 30 s.** Rotate at least 3 ids per
role, and alternate soft and hard variants (a `whoosh-swish-2` followed
by `whoosh-swish-2` is a loop; a swish followed by an impact is a
conversation). The journal (`memory.md`) is how you check — it lists
every SFX id with its time.

## The semantic map (the library's own families)

The sound library is categorized semantically — use the categories as
intent, not as a bucket:

| The motion on screen | Search |
| --- | --- |
| A number counting up (stat card, ticker) | `data-countup-short/medium/long` |
| A chart building, bars growing | `data-bar-grow`, `data-chart-reveal` |
| A tick, a meter, data tension | `data-tick` |
| Coins, cash, register, payment | `money-coin-drop`, `money-coins-pour`, `money-register`, `money-card-tap`, `money-notes` |
| A zoom, a slide, camera move | `whoosh-swish-1..6`, `whoosh-*` |
| A claim landing, a card appearing | `impact-*` (soft and metal variants) |
| UI beats, notifications | `ui-*`, `notification-*` |
| Typing, writing, working | `keyboard-*`, `writing-*` |
| Room, place, mood under a section | `ambience-*` |
| Build-ups into a reveal | risers (`impact` + `whoosh` layered) |

Queries are ANDed and prefix-matched: **two or three words, no sentences**
— "coin drop", not "coins dropping onto a wooden table in slow motion".

## Placement: the peak, not the start

The transient lands ON the cut, not after it. `sounds_add` places the
clip's START at the time you give — so give `peakAt − lead`, where lead
is the sound's attack (~80–150 ms; a riser's whole body leads its drop).
The plan carries `sfx.peakAt` per hit; do the subtraction at placement,
then set the level with `clip_audio_levels_db`.

## Levels

| Role | dB |
| --- | --- |
| Whooshes, motion ticks | −10 to −14 |
| Impacts on claims | −6 to −10 |
| UI/notification beats | −14 to −18 |
| Ambience under a section | −18 to −24 |
| Counter/chart sounds with their comp | −10 to −12 |

(`clip_audio_levels_db` is absolute: 0 dB unchanged, −60 the floor.)

## Density

- Energetic pacing: one design sound per cut, only where they add
  information — a sound on every single cut of a 30-cut minute is noise,
  and the viewer stops hearing all of them.
- Calm pacing: section changes and key moments only.
- Every transition gets its pair (whip → whoosh; dissolve → soft swell;
  glitch → digital hit).
- The re-hook and the outro card each get one signature sound the piece
  has not used yet.

## What "robotic" means and how it stops

Robotic SFX is three failures: the same id back to back, every sound at
the same level, every hit exactly on the grid with no musicality. The
fixes are the three laws above — rotate ids, vary levels ±2 dB around
the table, and let a few cuts land dry (no sound) so the next one is
heard.
