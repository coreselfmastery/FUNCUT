# The ear the Director was missing

FUNCUT can search 530 sounds by what their metadata says, and 1,191
tracks by title, artist and category. Until now nothing in the editor had
ever **opened the file**. This page is the audit that proves it, and the
measurement engine that closes it.

Status: **shipped, registered, allowlisted and taught.** `sound_sense.py`
is a sense family (`sense_sound`, four tools, 37 selftest checks), it is
on the Director's default allowlist, and the Director skill spends it —
Phase 18 verifies every placed accent with `check_placement`, and the
measured `peakAt` is what a cut lands on, not the manifest's `syncAt`
(this page's own audit found 61 of the 530 shipped sounds carry a
`syncAt` the file contradicts). What §6 planned is now the doctrine in
`prompts/sfx_design.md`, and `prompts/tool_reference.md` carries its
full schema.

## 1. What was verified in the tree

Every line below was read out of `bd1de9d` with `grep`, not taken from a
document. Where a claim in circulation is wrong, the correction is the
row.

| Capability | Where it actually lives | What it really does |
| --- | --- | --- |
| SFX search | `concat-host/src/sounds.rs:400` `Catalogue::search`, `:476` `Listed::score` | Ranks **text**: name > tags > id/category > use-for > description. No file is decoded. |
| SFX shelf entries | `concat-host/src/sounds.rs:293-360` `Listed` | `tags`, `description`, `useFor`, `syncAt` - all curator claims. |
| SFX waveform | `concat-host/src/sounds.rs:259` `envelope` | A real RMS envelope, computed **for the browser card**. It is not consulted when a sound is chosen. |
| SFX registers | `director_program.rs:3688` `sfx_queries_for_emotion`, `:3731` `sfx_queries_for_line` | Hand-written word lists per emotion/register, in one comment's own words chosen because "texture" matched **zero** of the 530. |
| SFX level | `director_program.rs:3710` `accent_gain_db` | Measured: the prosody ear's peak strength sets the gain in sigma buckets. Correct, and the one sound decision already driven by a measurement. |
| SFX sync | `director_program.rs:10258-10272` | The clip is placed at `moment - syncAt` - the **manifest's claim** about where the file's peak lands. |
| BGM search | `concat-host/src/bgm.rs:472`, free `search` at `:1317` | Ranks **text** over title/artist/category, plus a length filter and the order switch. |
| BGM mood words | `director_program.rs:2198` `bgm_words` | Words the emotion read emits, fed to `bgm_search` as a text query. |
| Music structure | `sense/audio_repair.py:1269` `analyze_music_structure` | Real and good: onset autocorrelation tempo, bar-phase search, 2-bar energy self-similarity, sections, drops, `cutSafePoints`. **Reachable only by handing it a file somebody already chose.** |
| Craft critic's sound axis | `sense/sense.py:1302` `AXES`, `:1361-1375` | `sound` is scored from silence fraction, dynamic range and LUFS error. Loudness, not appropriateness: no SFX role, no masking, no repetition, no musical continuity. |
| The sidecar door | `concat-mcp/src/sense.rs:31-44` `include_str!`, `:86` `runtime()`, `tools.rs:1145` `Action::SenseFamily` | Ten stdlib-only Python files unpacked per process and called over a pipe. This is the shape a new ear takes. |
| Audio embeddings | — | `grep -ri clap` finds one MP3 called `clap-single.mp3`. There is no audio-text model anywhere in the tree. |

## 2. The four gaps this leaves

**G1 - no file is ever listened to.** "Cinematic reveal" and "quick
transition" are different sounds in the ear and the same string in the
manifest. The search cannot tell a 40 s drone from a 300 ms whip if a
curator tagged both `cinematic`; the sfx stage then filters on duration
≤ 3 s, which is the only measurement in the whole path.

**G2 - no audio-side choice among music candidates.** The structure
analyser can say where a track's drop is, but it is run on the track
*after* selection. Nothing ranks the 1,025 FreePD tracks by whether one
of them actually does what the beat needs.

**G3 - `syncAt` is a claim.** The Director offsets every accent by it.
Nobody has ever checked one against the file. §4 reports what the 530
actually measure.

**G4 - the correction loop cannot see sound.** `craft_critique` can fail a
mix for loudness. It cannot fail "the fourth whoosh", "the peak is 200 ms
late", or "this cheerful bed is under a bereavement".

## 3. What ships: `concat-sound-sense`

`sense/sound_sense.py`, stdlib only, the same `--spec -` contract as the other
ten modules in that directory, 32 kHz mono (the top octave is where a whoosh lives -
`voice_quality`'s 16 kHz would cap the air band at 8 kHz).

Four tools:

| Tool | Answers |
| --- | --- |
| `describe_sound` | What IS this file: level, crest, attack, body, decay, transients, centroid, rolloff, band balance, flatness, tilt, pitch movement, energy trend, the **measured** peak position - then the six role fits and the evidence behind each. |
| `rank_sounds` | Of these candidates, which is a `quick_transition` / `cinematic_reveal` / `action_impact` / `gentle_entrance` / `emotional_texture` / `tension_build`, best first, with `why` - the terms its own score was summed from, never a number the formula ignores. Pass `index` and it answers from the cached measurements (the whole shelf in 0.11 s instead of 148 s), re-measuring only files whose size or mtime moved; `source` says which of the two answered. |
| `check_placement` | Is this placed accent on the moment, the right sound, at a level the voice can carry, and not the fourth of its kind - each issue with the edit that fixes it. Takes the clip's `inPoint` (a trimmed head moves the peak), reports `syncChecked`, and names `peak_cut` when the transient was trimmed out of the span rather than pretending a move can fix it. |
| `index_library` | Measure a whole shelf once, cached by size+mtime, and report where the manifest's `syncAt` disagrees with the file. |

### The measurements, and what each one decides

| Reading | Unit | Decision it informs |
| --- | --- | --- |
| `peakAt`, `peakFraction` | s, 0-1 | Placement: where the sound must be *put* so its hit meets the cut. Replaces trusting `syncAt`. |
| `attackMs` | ms | Whether it can land on a hard cut at all (80 ms cannot). |
| `bodySec` | s | Transition vs impact vs texture; whether it fits a 0.8 s beat. |
| `transients` | count | One hit, or a cluster that will fight the voice. |
| `crestDb` | dB | Sharp (impact) vs soft (entrance). |
| `centroidHz`, `rolloffHz` | Hz | Bright air vs dark weight. |
| `bands` sub/low/mid/presence/air | 0-1 shares | The weight of the thing: sub-heavy braam vs mid-forward tick. |
| `flatness` | 0-1 | Noise (air, whoosh) vs tone (chime, braam, pad). |
| `tiltDbPerOctave` | dB/oct | How steeply it falls: a sub drop vs a flat bright wash. |
| `pitchHz`, `pitchRatio`, `pitchDirection` | Hz, x | Reported, never scored: the dominant partial jumps an octave whenever a stronger harmonic takes over (a power-up reads 16x), so it says "something moved up", not "the pitch rose". Gated on flatness, and `no pitch` when the sound is noise. |
| `energySlopeDbPerSec`, `slopeFullDbPerSec` | dB/s | Build vs one-shot, over the active span and over the whole file. |
| `gainDb`, `steadyRise` | dB, 0-1 | Whether it actually CLIMBS: head-to-tail level, and how few steps go down. This is what separates a riser from thirty seconds of birds with one crow at the end. |
| `centroidDriftHz` | Hz | A sweep opening or closing across the sound. |
| `flux` | 0-1 | How much the spectrum MOVES window to window - the physical difference between air in motion and a hit that just starts and stops. |

### The six roles, and why the gates exist

Each fit is a weighted sum of normalised evidence (`_roles` in the
module), never a filename lookup:

- `quick_transition` - bright, airy, **moving** (spectral flux: the
  spectrum has to change between windows) and **held for about a third of
  a second**. The length term is a curve, not "short wins", because a
  16 ms impulse is a click and a 3 s wash is a bed.
- `cinematic_reveal` - heavy at the bottom, sustained, and tonal or
  sweeping.
- `action_impact` - heavy, sharp-edged, not sustained.
- `gentle_entrance` - soft, tonal, short and **still** (low flux: a
  chime is an event, a whoosh is movement).
- `emotional_texture` - tonal, held, gentle.
- `tension_build` - **climbing** in level (a real gain from head to
  tail, walked steadily) and held.

A file whose peak is below -60 dBFS is not scored at all: its `role` is
`silence`, every fit is 0, and the reply says so. Without that gate half a
second of digital silence scored **0.70 as a gentle entrance** - perfectly
tonal, soft and still, which is exactly what silence measures like.

Five multiplicative gates encode what the weights alone let the others
borrow: a transition must **move air**, an impact and a reveal must be
**heavy**, a gentle entrance must be **light and still**, a texture
must be **held**. They
are there because the first build of this engine scored a 0.55 s sub
thud at **0.44 as a quick transition** on its attack and length alone -
every measurement right, the answer wrong. That is the failure mode this
page exists to refuse, so the gate stayed and the selftest pins it.

**The ranges behind the fits.** Every term is a measurement placed on 0..1
between two numbers. They are the same in code and here, and they are what
was calibrated on the shelf (`_roles` in the module):

| Term | From | 0 at | 1 at |
| --- | --- | --- | --- |
| `heavy` | sub + low band share | 0.25 | 0.75 |
| `bright` | spectral centroid | 400 Hz | 3 kHz |
| `airy` | presence + air share | 0.02 | 0.35 |
| `noisy` | spectral flatness | 0.00 | 0.30 |
| `short` | body length | 1.0 s | 0.15 s |
| `moved` | spectral flux | 0.03 | 0.45 |
| `sustained` | body length | 0.6 s | 2.5 s |
| `sharp` / `soft` | crest factor | 8 / 16 dB | 20 / 6 dB |
| `climbs` | level gain x steadiness | 2 dB x 0.45 | 10 dB x 0.80 |
| `sweeps` | centroid drift | 300 Hz | 2.5 kHz |
| `transitionLength` | body vs 0.35 s | 6x off | 0x (0.35 s) |

## 4. Evidence on the shipped library

`python3 sound_sense.py analyse --spec '{"tool":"index_library","dir":"audio"}'`
over the 530 bundled sounds (ffmpeg CLI required for `.ogg`; the selftest
needs none):

One pass over the whole shelf: 530 files in **141 s** (0.27 s each),
**57 MB** peak RSS, no failure - measured as the child's `ru_maxrss`, with
Python 3.13 and ffmpeg 7.1.5 on a shared 4-core box, so treat the time as
this machine's number rather than a specification: decode is ffmpeg's cost,
not this module's. The rest of the cost is one `ffmpeg` spawn per file and up
to 25 FFTs of 2048 points; nothing here is heavy, which is the point - it can
run as a background index while the timeline stays live. With that index
written, ranking all 530 against a role takes **0.11 s** instead of 148 s,
because nothing is decoded twice.

**What the shelf actually contains, by the curator's category against the
measured role** (top rows):

| Category | n | Measured roles |
| --- | --- | --- |
| Impact | 83 | `action_impact` 30, `cinematic_reveal` 23 (the long, heavy ones), `gentle_entrance` 17 |
| UI | 81 | `gentle_entrance` 47, `quick_transition` 20, `emotional_texture` 7 |
| Whoosh | 64 | `quick_transition` 34, `action_impact` 10 (the 50 ms ones), `gentle_entrance` 7 |
| Motion | 56 | `quick_transition` 29, `gentle_entrance` 14, `emotional_texture` 10 |
| Foley | 53 | `quick_transition` 33, `gentle_entrance` 12, `emotional_texture` 5 |
| Riser | 38 | `quick_transition` 19 (bright air in motion is a transition too), `cinematic_reveal` 9, `tension_build` 8 |
| Notification | 28 | `gentle_entrance` 25 |
| Ambience | 27 | `cinematic_reveal` 15 (drones), `emotional_texture` 7, `quick_transition` 5 |

Two disagreements are worth reading rather than fixing. Only 34 of 64
whooshes top out as a transition: the reverse ones are risers and read as
`tension_build`, because that is what they do. And `tension_build` is the
rarest read in the whole shelf (13 files) because a riser that is bright
and airy is also, physically, air in motion - so the sharper role wins
and the build is still visible one line down the ranking. A category name
describes a sound; a role describes the job, and one sound can do two.

**How well the ranking orders the shelf.** Each role was scored over all
530 and precision is measured at the top 64 against the curator's own
categories (a weak label - several categories serve one job), with the
uniform baseline for the same positive set:

| Role | Positive categories | P@64 | Baseline | Lift |
| --- | --- | --- | --- | --- |
| `action_impact` | Impact | 0.64 | 0.16 | **4.1x** |
| `tension_build` | Riser | 0.44 | 0.07 | **6.1x** |
| `cinematic_reveal` | Ambience | 0.23 | 0.05 | **4.6x** |
| `emotional_texture` | Ambience + Music | 0.42 | 0.12 | **3.7x** |
| `gentle_entrance` | UI + Notification + Keyboard + Writing | 0.64 | 0.26 | **2.4x** |
| `quick_transition` | Whoosh + Motion | 0.38 | 0.23 | **1.7x** |

`quick_transition` is the weakest and is reported as such: foley and
digital sweeps measure like whooshes on every static reading, and only
spectral flux separates them at all. That is also the role a CLAP
re-rank would fix first, and the reason the interface keeps a `fit` per
candidate rather than a label.

**Where the scale saturates.** Eleven of the 530 fit `cinematic_reveal` at
exactly 1.0: `heavy`, `sustained` and `sweepOrTonal` all top out on the
shelf's drones, so the score says "any of these does the job" and cannot
say which. That is a real limit of a weighted sum over saturating ranges,
not a bug to hide: the reply carries `tiesAtTop`, and the choice among
them is made on the slot's duration, the level the voice leaves and the
file's own peak. Spread the ranges and the ties would thin; recalibrate
and every number on this page moves, so it is a change to make with a
measurement in hand, not by taste.

**The top of the ranking, unedited** (`rank_sounds` with no text query at
all, only the measured fit):

| Role | First four, with the curator's category |
| --- | --- |
| `quick_transition` | `reverse-whoosh-medium-strong` (Whoosh), `money-notes` (Foley), `shimmer-medium-bright` (Motion), `ui-maximize` (UI) |
| `cinematic_reveal` | `riser-braam-rise` (Riser), `drone-dark-tension`, `ambience-dark-1`, `drone-epic-swell` (Ambience) |
| `action_impact` | `impact-metal-sci-fi`, `impact-plate-heavy` (Impact), `whoosh-bamboo-3` (Whoosh), `impact-heavy-hard` (Impact) |
| `tension_build` | `riser-orchestral-style`, `riser-pre-drop-tension`, `riser-reverse-swell`, `riser-noise-long` (all Riser) |

Those four reveal rows are tied at 1.0 and printed in the order the tool
returns them (alphabetical among equals), not in order of preference - see
the saturation note above. The rest of each column is ordered by fit.

**`syncAt` against the file.** 420 of the 530 carry a `syncAt`. Measured
against where the peak actually falls:

| | Value |
| --- | --- |
| median error | 14 ms (the curator's numbers are mostly honest) |
| 90th percentile | 453 ms |
| worst | **3.02 s** |
| wrong by more than 100 ms | **128 files (30%)** |
| wrong by more than 300 ms | **61 files (15%)** |

Every one of those 61 was placed by the Director's sfx stage at
`moment - syncAt`, so the accent lands up to three seconds away from the
cut it was chosen for, and nothing downstream could tell. Median
confidence in the top role across the shelf: 0.93.

## 5. The models, measured against these numbers

The model layer exists now: `sense/sound_model.py`, with YAMNet naming
what a file contains and CLAP ranking it against a written description.
It is a separate module for the reason this one is standard-library only:
**this file stays arithmetic over a decoded buffer**, so an issue it
reports always arrives with the measurement that proves it, while the
models - which need numpy and ONNX Runtime - are optional and say so
when absent.

The question the upgrade had to answer was whether a model beats these
measurements. On the same 530 sounds, same labels, P@64:

| Role | CLAP alone | These measurements |
| --- | --- | --- |
| quick_transition | **0.62** | 0.38 |
| gentle_entrance | **0.73** | 0.64 |
| action_impact | 0.59 | **0.64** |
| tension_build | 0.31 | **0.44** |
| emotional_texture | 0.31 | **0.42** |
| cinematic_reveal | 0.23 | 0.23 |

So the model wins the transitions by a wide margin - the role §4 called
the weakest, at 1.7x lift, because foley and whooshes measure alike on
every static reading - and loses where physics is the point: a riser has
to climb, a texture has to sustain. The fused ranking weights the two
per role, fitted on that table, and on transitions the weight on these
measurements is **zero**: they do not merely fail to help, they pull the
model down from 0.62 to 0.42.

`docs/SOUND-MODEL.md` holds the full table, the weight sweep, the model
licences and sizes, and the acceptance test that reproduces the numbers
(`scripts/sound-model/bench.py`). The rule from the first draft of this
section still stands and is now enforced in code: CLAP similarity is not
a calibrated probability and not a quality score, every reply names the
engine behind each number, and no measured number is ever replaced by a
modelled one.

## 6. Wiring it to the Director

Status update: the DIRECTOR-SKILL half of this section is done — Phase 18
of `.claude/skills/funcut-director/SKILL.md` plans with `sense_sound_model
{action: "plan"}`, proves every accent with `check_placement`, and shops
by measurement; `prompts/sfx_design.md` and `prompts/music_and_duck.md`
carry the doctrine; `prompts/tool_reference.md` carries the schemas. What
remains is the ENGINE half: the AUTO EDIT stages below still run the
name-led path, and each item names the function that gains a call, so
that change is mechanical rather than a redesign.

**The sidecar itself** - shipped as described: `sound_sense.py` is
embedded in `sense.rs`, registered as `Action::SenseFamily("sound")` and
served on the Director's allowlist as `sense_sound`. The tool takes
`tool`, and the four verbs above.

**The sfx stage** (`director_program.rs`, the `SfxPending::Pick` arm at
`:10220` onward):

1. `sounds_search` still finds candidates by text - it is a good filter.
2. `rank_sounds` over the top 3 with the register's role re-ranks them by
   what they measure as. The registers already exist
   (`sfx_queries_for_line`); what is missing is a role name for them.
3. Place with the **measured** `peakAt` instead of the manifest's
   `syncAt`, and record which was used in the signal log.
4. After `clip_audio_levels_db`, `check_placement` with the peak's
   intended moment and the take's own level. A `sync_off` or `wrong_sound`
   issue becomes another `edit_apply` step rather than a note.

**The music stage** (`Stage::Music`, `:9553`): after `bgm_search` returns
a page, `rank_sounds` with the beat's job (`emotional_texture` for a
setup, `tension_build` for the turn, `cinematic_reveal` for the reveal)
orders that page by measured fit; and `analyze_music_structure` on the
chosen bed - which the repair module already computes - puts the drop on
the reveal instead of wherever the bed happens to start.

**The verify stage**: `check_placement` per accent, and the rendered
mix's `measure_loudness` beside it. The two together are the sound half
of the quality gate; loudness alone is the half that cannot see taste.

**The signal contract** (`AGENTS.md`): every one of these decisions is a
`sfx` or `bgm` entry with a confidence. The tools return `confidence` and
`evidence` for exactly that reason - a decision logged without the
measurement behind it is the thing the repository's three guards exist to
catch.

## 7. What this does and does not claim

- It measures; it does not decide. Which sound a scene *should* have is
  the Director's judgment, and this returns the facts that judgment needs.
- `check_placement` never reports a fix as done. It returns the issue,
  the measurement and the edit that answers it. Only a re-measure after
  the edit can say it worked.
- `wrong_sound` fires below 0.35 fit. That threshold is a floor for
  "measurably not this job", not a taste line; the reply carries the
  actual fits so a caller can disagree with the threshold and see the
  gap.
- A fit saturates at 1.0, and the shelf holds several sounds that do some
  jobs exactly. `tiesAtTop` counts them and the order among them is
  alphabetical, not a preference: the slot's duration, its level and the
  file's own peak decide, not this module.
- `why` lists only the terms the asking role's formula sums, so it can
  never name a measurement that carried no weight - but it is still a
  sum of normalised evidence, not a proof that the sound is good.
- The role model is English-language craft vocabulary mapped onto
  measured sound. It knows nothing about cultural context, lyrics, or
  whether a track is any good.

## 8. Tests

`python3 src/crates/concat-mcp/sense/sound_sense.py selftest` - 37
checks, no ffmpeg, no network, no fixtures on disk beyond a scratch dir:

- six synthetic sounds whose answer is arithmetic (air → transition,
  short heavy hit → impact, rising tone → build, swelling triad →
  texture, deep sustained sweep → reveal) and one impulse for the peak
  test;
- the impulse's measured peak is within 25 ms of where it was written;
- a transition ranks the air first and refuses the impulse and the sub;
- an impact ranks the heavy hit first;
- a peak 200 ms late is reported with the move that fixes it, and
  with the project and clip named the fix is one runnable `moveClips`;
- a sub placed as a whoosh is named the wrong sound;
- an accent 6 dB over the voice is levelled down, with the number;
- a fourth transition in ten seconds is called overused;
- the index measures every file, reports a manifest `syncAt` that is
  200 ms from the real peak, and re-measures nothing the second time;
- a 30 ms tick reports its own 30 ms, not the 64 ms of padding one FFT
  window adds, and says it was padded;
- a silent file is `silence` with every fit 0, is never ranked as a job a
  cut can use, and says so in its note;
- a stereo file is downmixed rather than counted twice;
- `maxDuration` keeps only what fits the slot and says why each of the
  others was skipped;
- a trimmed head moves the peak with the clip instead of reading as late,
  and a peak trimmed out of the span is named `peak_cut`;
- a ranking served from the index agrees with a fresh one, and a file that
  changed under the index is measured again;
- a limited index run refuses to overwrite the shelf's index with a sample;
- a missing file is an error with a reason, never a zero reading;
- a fix never invents a clip id: without one it names the fields it needs,
  and `why` never names a term the score did not sum.

The test is in CI beside the other ten sidecar selftests.

## 9. Non-goals

Not another AUTO EDIT path. Not a replacement for the voice-signal
family. Not CLAP-driven creative decisions. Not one model per SFX
category. Not trusting a description of a sound over the sound.
