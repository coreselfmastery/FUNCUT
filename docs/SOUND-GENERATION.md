# Sound generation: three small models that write audio

FUNCUT reads sound with two models (YAMNet and CLAP) and now writes it
with three. All five live in the same folder, are downloaded by the same
sheet, and are run by the same Python: `sound_model.py`'s `generate`
action. Nothing here needs a GPU, a network at edit time, or a model
bigger than a few hundred megabytes.

## The three

| kind | model | params | on disk | writes | licence |
| --- | --- | --- | --- | --- | --- |
| `sfx-small` | TinyAudio-MF | 87 M | ~0.36 GB | 10 s mono @ 44.1 kHz | MIT |
| `sfx-large` | Stable Audio Open Small | 341 M | ~0.54 GB | 11.9 s stereo @ 44.1 kHz | Stability AI Community |
| `bgm` | TinyMusician (MusicGen small, ONNX) | ~135 M | ~0.45 GB | a music bed @ 32 kHz | CC-BY-NC 4.0, inherited from MusicGen-small |

`sfx-small` is the baseline: one mean-flow step, eleven seconds of work
for three seconds of sound on a four-core laptop. `sfx-large` is the
hook - a distilled diffusion model, eight steps, twenty-three seconds for
four seconds of stereo. `bgm` is the slow one: a MusicGen decode with
four codebooks, roughly ten seconds of compute per second of music, and
the reason a bed is written for a short cut and not for a feature.

The numbers were measured on the machine this was built on
(`sfx-small` 3 s → 11.5 s, `sfx-large` 4 s → 23.5 s, `bgm` 3.9 s → 38.3 s,
four threads).

## Downloading

Settings > Models carries three rows beside the reader's own - one per
generator - and `models_list` / `models_download` do the same headless:
`models_download { "kind": "sound", "id": "sfx-small" }` (the ids are
`sound-models`, `sfx-small`, `sfx-large`, `bgm`), then `models_list` to
watch the job. The sheet and the API read one table
(`concat_host::soundmodel::set_by_id`), so a row and a headless download
fetch the same bytes.

The files come from this repository's release mirror, are checked against
the digests in `models/manifest.toml`, land in the same `cutout-models`
folder the reader uses (`.part` until verified, then renamed), and are
removed by the same button. A set is ready only when every file in it is
present: half a generator is not a generator.

The files are renamed on the way out of their upstream repositories so
the table can hold three models without three directory layouts - except
the three `sfx-large` weight files, which keep the names their graphs
were exported with (ONNX stores the external-data location inside the
graph, so renaming the file would leave a graph that cannot find its own
bytes):

```
sfx-small-pipeline.json  sfx-small-tokenizer.json
sfx-small-text.onnx  sfx-small-flow.onnx  sfx-small-vae.onnx
sfx-large-meta.json  sfx-large-tokenizer.json  sfx-large-seconds.json
sfx-large-t5.onnx  t5_text_encoder.onnx.data
sfx-large-dit.onnx  dit_step.onnx.data
sfx-large-vae.onnx  vae_decoder.onnx.data
bgm-config.json  bgm-tokenizer.json
bgm-text.onnx  bgm-decoder.onnx  bgm-codec.onnx
```

## Writing a sound

```json
{ "action": "generate", "kind": "sfx-small",
  "prompt": "a heavy wooden door slams shut in a stone hall",
  "seconds": 2.5, "seed": 7, "project": "/home/you/Videos/Cut" }
```

`monetized: true` is the caller's own assertion, and it can only tighten
the project's brief, never loosen it - a run that knows its plan is
monetized says so even when the folder carries no brief yet.

The reply names the file:

```json
{ "ok": true, "kind": "sfx-small", "model": "tiny-audio-mf",
  "path": "/home/you/Videos/Cut/generated-audio/sfx-small-a-heavy-...-7.wav",
  "seconds": 2.5, "rate": 44100, "channels": 1,
  "peak": 0.89, "rms": 0.2279, "elapsedSec": 11.45 }
```

- `project` keeps the write inside the folder the edit owns; without it
  the file lands in a temporary folder.
- The output is checked before it is placed: a generation that comes
  back near-silent (its loudest sample 40 dB under full scale) is
  answered without a `path` - `"generated": false` - so the Director
  shops the shelf instead of placing a silent file.
- The output is peak-normalised to 0.89 with a 10 ms fade at each end, so
  a placed accent neither clips nor clicks.
- The same `seed` and prompt reproduce the same file.
- `steps` and `threads` are the performance knobs: more steps is slower,
  not better (`sfx-small` is trained for one). Threads default to half the
  cores, at most four, so the editor keeps a core to draw with; name
  `threads` to override.
- The graphs are built inside the call and released when it ends
  (`_release_sessions`), so nothing is held between calls - an editor left
  open for hours holds no model memory.
- Before a load, the model's own bytes are measured against the machine's
  free memory; a kind that would not fit is refused with `reason:
  "memory"` and a suggestion (`sfx-small` is 0.35 GB), instead of pushing
  the editor into swap.

Then place it with the tools that already exist:

```
sounds_add  { "path": "<project>", "file": "<the path above>", "start": 2.0 }
bgm_import  { "path": "<project>", "file": "<the path above>", "start": 0.0 }
```

Both copy the file into the project's own `audio` or `bgm` folder, add it
to the bin and place it - the same undoable step a shelf sound takes. A
local file skips the shelf's *catalogue*, because it is not the shelf's -
but a generated one is not unlabelled: `generate` writes
`<file>.licence.json` beside it, and `sounds_add` / `bgm_import` read that
record and refuse a non-commercial bed in a monetized edit, the same line
MobyGratis is held to. The record travels with the project, so the licence
of a file is answerable long after the run that wrote it.

## What AUTO EDIT does with them

Two stages ask the generators before they shop:

- **Accents.** Every peak that earns an accent asks for one: the hook
  (the first three seconds) with `sfx-large`, every other peak with
  `sfx-small`, one to two and a half seconds each. A model that is not
  installed *answers* - `{"ok": true, "model": "absent", "generated":
  false}` with no `path`, in about a second - and the shelf takes over
  for the rest of the run, so an install without the models behaves
  exactly as it did before. (The absence is an answer and not an error
  on purpose: the runner only records replies, so an error would never
  reach the stage and it would end warned, with no accents at all.)
- **The bed.** The music stage writes one when the cut is 8 seconds or
  shorter - the length a written bed can cover inside the call's budget -
  and keeps the shelf for longer cuts, which the shelf carries at every
  length. It asks the music model first, with its own plan's
  `monetized: true`; the model refuses on licence (the plan says
  monetized, and MusicGen-small's weights do not allow it), and the stage
  asks `sfx-large` for the same words as a warm ambient pad - no drums,
  no melody - which the Stability Community Licence allows. The written
  bed is imported into the project's `bgm` folder and then trimmed to the
  cut and ducked under the voice by the same code that ducks a shelf
  track. A run whose plan is not monetized gets the music model's own
  bed, so the third model is spent wherever the licence lets it be.
  (The licence refusal - like an absent model and a near-silent
  generation - is an *answer*, `ok: true` with `reason: "licence"` and no
  `path`: the runner only records replies, so an error would never reach
  the stage and the pad fallback would never run. The answer is what
  makes the whole ladder reachable.)

The signal contract is unchanged: a generated accent is placed on the
same measured peak, at the same level, as the shelf accent it replaces.

## Licences

- **TinyAudio-MF** - MIT.
- **Stable Audio Open Small** - the Stability AI Community License: free
  for research and for commercial use under $1M annual revenue, with the
  licence's own attribution terms. Read it before shipping client work.
- **TinyMusician** - the repository carries no licence file of its own,
  but its model card names `facebook/musicgen-small` as its base, and
  MusicGen-small's weights are **CC-BY-NC 4.0**. An ONNX export of those
  weights is a derivative of them, so the export is non-commercial
  whatever the repository says. That is not "unclear" and it is not
  "unlicensed": it is a known, narrow grant.

The code enforces what the table says:

- `generate` reads the project's own `creative-brief.json` and refuses
  `bgm` in a monetized project (`reason: "licence"`), before it loads
  anything. A non-monetized project gets the bed.
- `sounds_add` and `bgm_import` read the licence record beside a generated
  file and refuse a non-commercial one in a monetized edit, even when the
  caller's request says otherwise - the brief is the authority, as it is
  for MobyGratis.
- AUTO EDIT's own plan is monetized by default, so its written bed is
  asked of `sfx-large` (a warm ambient pad, the community licence) after
  `bgm` refuses; the music model still writes the bed for a project marked
  non-monetized.

The generator's reply carries its `licence` and `commercial`, so a run can
record which model wrote which file and what that allowed.
