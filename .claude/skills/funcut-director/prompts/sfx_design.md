# Sound design (SFX)

Sound is half the edit and the first thing an amateur edit skips. The
rules are short because the discipline is short: variety, placement,
restraint — and every placement decided by a MEASUREMENT, not a
filename.

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
| A trailer-grade hit, a title card, THE moment | `impact-cinematic-slam`, `impact-trailer-hit`, `impact-final-blow`, `stinger-logo-hit` |
| A sub drop, a bass accent under a reveal | `impact-sub-drop-hit`, `impact-bass-cannon`, `stinger-sub-hit` |
| Tension climbing into a cut | `riser-cinematic-long`, `riser-braam-rise`, `riser-tension-build`, `riser-pre-drop-tension` |
| The suck before the hit | `whoosh-reverse-cinematic`, `impact-reverse-slam`, `riser-reverse-cymbal` |
| Something flying past, a fast pan | `whoosh-doppler-pass`, `whoosh-dragon-flyby`, `whoosh-fast-air-cut` |
| A glitch or digital moment | `transition-zap-digital`, `transition-glitch-stutter`, `transition-pixel-dissolve` |
| A portal, magic, transformation | `transition-portal-open`, `transition-magic-shimmer`, `stinger-magic-reveal` |
| Dread or scale holding a section | `drone-dark-tension`, `drone-horror-sub`, `drone-epic-swell` |
| Water, ice, fire, mechanical beats | `transition-water-drop-reveal`, `transition-ice-crackle`, `transition-fire-whoosh`, `transition-mechanical-clunk` |
| Powering down, rewinding, shutting off | `transition-power-down`, `transition-tape-rewind`, `impact-dramatic-stop` |

Queries are ANDed and prefix-matched: **two or three words, no sentences**
— "coin drop", not "coins dropping onto a wooden table in slow motion".
The map is the QUERY vocabulary — which candidates you pull. What decides
between them is the measurement below.

## Measured selection (rank the file, not the name)

The shelf's own audit (docs/SOUND-SENSE.md) measured the 530 files and
found the manifest's `syncAt` peak claim wrong by more than 100 ms in
30% of them — a name-led pick that trusts it lands its accent on the
wrong frame. So the ears decide:

- **A role, shopped by hand**: `sense_sound {action: "rank_sounds",
  role, dir|paths, limit}` — candidates scored on the six measured role
  fits (`quick_transition`, `cinematic_reveal`, `action_impact`,
  `gentle_entrance`, `emotional_texture`, `tension_build`), each with
  the evidence behind it and `measuredPeakAt` beside the manifest's
  `syncAt` so the drift is visible.
- **A description, shopped semantically**: `sense_sound_model {action:
  "rank", index, query, role}` — CLAP similarity ("dark cinematic
  braam") fused with the measured fit. Without the models installed it
  says `degraded` and ranks by measurement alone — a weaker answer it
  admits, never hides.
- **The whole design at once**: `sense_sound_model {action: "plan",
  index, beats, visual, arc, bgm, durationSec, voiceLufs}` — the edit's
  own signals (emphasis words, pauses, reveals, cuts, motion hits, the
  arc's acts, the bed's structure) become a per-moment accent plan:
  file, `startAt`, `peakLandsAt`, `syncOffset`, `gainDb` under the
  voice's LUFS, held silences, and the why per accent. Plan first, then
  place its accents.

The one measured decision the name-led path already had — the level
from the prosody ear's peak strength — stays; measurement replaces the
SELECTION and the SYNC, not the gain discipline.

## Placement: the measured peak, not the start

`sounds_add` places the clip's START at the time you give, and the
transient must land ON the cut, not after it. The number that decides
is the MEASURED `peakAt` — from `rank_sounds`' candidates or one
`describe_sound` call — never the manifest's `syncAt` (wrong by >100 ms
in 30% of the shelf): give `start = peakAt_target − measuredPeakAt`
(and the same subtraction as the head trim if the file needs one),
then set the level with `clip_audio_levels_db`.

**Prove every placed accent**: `sense_sound {action:
"check_placement", path, clipStart, intendedAt, gainDb, voiceLufs,
recent}` — the measured peak against the moment it was meant to land
(a `sync_off` issue names the exact `moveBy`), the level under the
voice, the role against the moment, and repetition inside the window.
A placement nobody checked is a guess with a good filename.

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
