# Sound design (SFX)

Sound is half the edit and the first thing an amateur edit skips. The
discipline used to be guesswork — categories, filenames, a peak position
the manifest claimed. It is measured now: index the shelf, plan the
sound design from every signal, place on the measured peak, verify each
placement, and generate only what the shelf cannot supply.

## Why measured beats metadata

The shelf's manifest carries `syncAt`, a claim about where a sound's
peak lands. Measured against the files: the median error is 14 ms, but
128 files are wrong by more than 100 ms and **61 of the 530 shipped
sounds by more than 300 ms** — the worst is 3.02 s. Every one of those
was placeable at `moment − syncAt`, putting the accent seconds from the
cut it was chosen for, with nothing downstream able to tell. The
waveform does not lie; the metadata sometimes does. The peak a
placement uses is the measured one.

## Index once, then plan

Two indexes, both cached by size and mtime, both built once per shelf:

- `sense_sound {action: "index_library"}` — the measured shelf: level,
  crest, attack, body, transients, spectral centroid, band balance,
  flatness, pitch movement, energy trend, and the peak's real position
  per file. It also reports every `syncAt` the file contradicts.
- `sense_sound_model {action: "index"}` — the semantic shelf: YAMNet's
  521 AudioSet labels and a CLAP 512-d embedding per asset.

## The plan is the source of SFX decisions

SFX are not browsed into place. `sense_sound_model {action: "plan"}`
takes the beats (emphasis words, pauses, sentence type, pitch
direction), the visual arc (cuts, reveal events, motion hits), the BGM
candidates, the style (`maxAccentsPer10s`, `minAccentGapSec`),
`durationSec` and `voiceLufs`, and returns one sound design for the
whole piece. Per moment it gives: `at`, `role` and the context that
earned them; the selected `id` with its `why` — the signals that named
the moment, the fused score with its CLAP and measured parts;
`startAt`, already computed as `moment − measured syncOffset`; `gainDb`
as `target − measured peak`, clamped to −24..+6 dB against the voice's
own level; `duration` with `fitsGap`, the voice-overlap rule — a 4 s
boom never sits in a 1 s gap to the next word. Each file is spent once,
a stronger moment evicts a weaker one inside the density floor (the
plan says which gave way), `silence[]` marks a pause ≥ 0.5 s before a
reveal — hold the room, land the sound on the reveal itself — and
`warnings[]` names every moment refused and the constraint that refused
it. No candidate for a moment is a fact to design around, not an error
to retry blindly.

The fusion weights were fitted on the 530 shipped sounds: the CLAP
model wins transitions (0.62 against 0.38), the measurements win risers
(0.44 against 0.31) and textures — which is why both indexes are spent,
never one alone.

**Queries are ANDed and prefix-matched** — two or three words, no
sentences: "coin drop", not "coins dropping onto a wooden table in slow
motion".

## The role vocabulary

Six jobs. The tools accept the plain word or the role name:

| Plain word | Role | The job |
| --- | --- | --- |
| whoosh | `quick_transition` | move air across a cut, a slide, a fast pan |
| impact | `action_impact` | the claim lands — heavy, sharp, short |
| riser | `tension_build` | climbs into the reveal |
| reveal | `cinematic_reveal` | the boom, braam or sub drop under THE moment |
| foley | `gentle_entrance` | UI beats, chimes, small physical detail — soft, short |
| ambience / texture | `emotional_texture` | the room under a section — tonal, sustained |

A category describes a sound; a role describes the job — only 34 of
the 64 whooshes top out as transitions, because the reverse ones are
risers and read as `tension_build`. `rank_sounds` returns the evidence
terms behind every fit, so a disagreement is readable, not a coin
flip; the ranked shortlist behind each plan pick is the alternatives
list.

## Placement: the measured peak, not the start

`sounds_add` places the file's START at the time given; the transient
must land on the moment. Start = target − measured `peakAt` — the
plan's `startAt` already carries that subtraction, computed from the
index's measured peak position, never the manifest's `syncAt`. A
riser's whole body still leads its drop; that is in the measurement,
not in a guessed 80–150 ms attack. Apply the plan's `gainDb` to the
placed clip with `clip_audio_levels_db`.

## Verify every placement

After `sounds_add`, run `sense_sound {action: "check_placement"}` on
each placed accent — `path`, `clipStart`, `intendedAt` (the moment),
the `gainDb` applied, the role meant, `inPoint` when the clip was
trimmed, and `recent` with the accents already placed. It measures the
placed file and answers with evidence:

- `sync_off` — the peak sits N ms off the moment (tolerance 40 ms
  default), with the `edit_apply` move that fixes it;
- `peak_cut` — the transient was trimmed out of the span: the sound is
  not late, it is gone; re-shop the role with `rank_sounds`;
- `wrong_sound` — placed as an impact, measures as a texture, with the
  measured terms that say so;
- `too_loud` — at this gain the accent peaks over the voice's own
  ceiling, with the `clip_audio_levels_db` level that answers it;
- `overused` — the fourth whoosh inside 12 s, with the `clips_remove`
  that thins it: repetition flagged, not heard.

An accent that was never verified is a claim, not a placement.

## Generate when the shelf cannot

When the brief names a sound the 530 do not hold — a specific object,
a texture the shelf lacks — `sense_sound_model {action: "generate"}`
writes it. `sfx-small` (TinyAudio-MF, MIT) is the default for accents;
`sfx-large` (Stable Audio Open Small) spends its longer call on the
hook; `bgm` (TinyMusician) never reaches a monetized project — the
action itself answers `ok: true, generated: false, reason: "licence", model: "tinymusician"`, because TinyMusician
ships no licence file. An absent model and a near-silent generation
both answer without a `path`: shop the shelf, do not retry. The file
lands in `<project>/generated-audio/`; `sounds_add {file}` places it
like a shelf sound, and `check_placement` still verifies it.

## Repetition, density, silence

**Never the same id twice near itself.** The plan spends each file
once and `check_placement` flags the repeat. Rotate at least 3 ids per
role and alternate soft and hard variants — a swish followed by an
impact is a conversation, a swish followed by the same swish is a
loop. The journal (`memory.md`) lists every id with its time.

Density is a knob, not a feeling: `maxAccentsPer10s` (default 4) and
`minAccentGapSec` (0.45). A sound on every cut of a 30-cut minute is
noise the viewer stops hearing. Silence is a design choice — the
plan's `silence[]` marks the held room before a reveal; let a few cuts
land dry so the next one is heard.

**Prove every placed accent**: `sense_sound {action:
"check_placement", path, clipStart, intendedAt, gainDb, voiceLufs,
recent}` — the measured peak against the moment it was meant to land
(a `sync_off` issue names the exact `moveBy`), the level under the
voice, the role against the moment, and repetition inside the window.
A placement nobody checked is a guess with a good filename.

## Levels

The plan computes `gainDb` from each asset's measured peak against the
voice's loudness. Planning by hand, the table:

| Role | dB |
| --- | --- |
| Whooshes, motion ticks | −10 to −14 |
| Impacts on claims | −6 to −10 |
| UI/notification beats | −14 to −18 |
| Ambience under a section | −18 to −24 |
| Counter/chart sounds with their comp | −10 to −12 |

(`clip_audio_levels_db` is absolute: 0 dB unchanged, −60 the floor.)
Vary ±2 dB around the table — every hit at the same level is half of
what "robotic" names, and the grid-perfect landing is the other half.
