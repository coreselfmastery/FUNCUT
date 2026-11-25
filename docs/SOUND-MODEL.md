# The sound model layer: what a sound means, from two small models

`sound_sense.py` measures a sound. This layer names it. The two answer
different questions, and the Director needs both:

```
   sound.ogg
      |
      |-- sound_sense.py ........ length, peak, attack, decay, brightness,
      |                           bass, transients, silence        (facts)
      |
      |-- sound_model.py
      |     YAMNet .............. 521 AudioSet classes: whoosh, explosion,
      |                           applause, engine              (what it is)
      |     CLAP ................ 512-d audio/text space: does it match
      |                           "a deep cinematic braam"?     (what it means)
      v
   one record per asset, cached, with the evidence behind every field
```

Neither model places a sound or sets a level. Timing and loudness are
measurements, and the Director decides. A model that answers "is this a
whoosh" will answer cheerfully about a file of silence; that is why the
measurement layer exists and why it gates the models rather than the other
way round.

## 1. The tools

`sound_model.py <analyse|selftest> --spec -`, the same contract as every
other sense script: one JSON spec in, one JSON object out.

| Tool | Answers |
| --- | --- |
| `describe` | one file: identity, YAMNet labels, CLAP embedding + best-matching description, Sound Sense measurements, loudness/quality, roles, sync offset, trim advice |
| `index` | the whole shelf, one record per file, cached by size, mtime and model version |
| `rank` | a creative description and a role: candidates scored by CLAP fused with the measured role fit |
| `plan` | every signal at once — the beats' emphasis and pauses, the visual events and cuts, the arc's acts, the bed's structure — into a per-moment accent design, held silences and the music plan, each choice with its evidence |
| `selftest` | nineteen checks, no network: the filterbank, the front-end, the planner, and - when models are installed - that YAMNet calls a whoosh a whoosh |

`numpy` and `onnxruntime` are used when installed and never required, the
same rule `voice_quality.py` follows. Without them every tool still answers,
with `"model": {"state": "absent", "reason": ...}` and measurement-only
records. A Director without the models is less certain, not broken.

## 2. The model choice, and what was rejected

The question was "is CLAP the right model, or is there better research?".
These are the facts that decided it, taken from the model cards and the
Hugging Face API rather than from a blog post:

| Model | Licence | Weights | ONNX ready | Verdict |
| --- | --- | --- | --- | --- |
| **LAION CLAP `htsat-unfused`** | Apache-2.0 | 34 MB audio + 127 MB text, int8 | **yes** (`Xenova/clap-htsat-unfused`, 9.5 k downloads) | **chosen** |
| LAION CLAP `larger_clap_music_and_speech` | Apache-2.0 | 78 MB audio + 127 MB text | yes | the upgrade, for BGM |
| LAION CLAP `larger_clap_general` | Apache-2.0 | 78 MB audio | yes | no better than `htsat` here |
| MS-CLAP | MIT | ~30-100 M params | no official export | not worth a conversion |
| MuQ / MuQ-MuLan | CC-BY-NC | music SSL, strong music retrieval | no | licence blocks shipping |
| MERT | CC-BY-NC | music-only, 95 M-330 M | no | licence blocks shipping, and it cannot see SFX |
| T-CLAP | Apache-2.0 | better music retrieval than CLAP | no | worth revisiting when an export exists |
| Qwen-Audio / Audio Flamingo | various | 7 B+ | n/a | not a laptop model |

Three things decided it beyond size: **Apache-2.0** (the repo records a
licence per model in `concat-vision/src/models.rs` and in
`THIRD_PARTY_NOTICES.md`, and a non-commercial weight would block shipping),
**an existing ONNX export from the same publisher the repo already uses**
(`Xenova`, who also publish the SlimSAM models), and **one shared space for
audio and text**, which is the whole point: the Director writes a
description, and the description is compared with the audio.

YAMNet is Apache-2.0 from Google, 521 classes, 3.7 M weights, 16 MB as ONNX.
It is an event classifier and this layer treats it as one: it says what is in
a file, never what to do with it. **YAMNet is not the music analyzer.** Music
structure (tempo, beats, sections, drops) comes from the existing Rust music
analysis; CLAP only adds the semantic match on top of it.

## 3. Does the model actually beat the measurements? Measured, not assumed

Same 530 shipped sounds, same labels (the curator categories), same metric
(P@64: of the top 64 ranked, how many are the right category). Base rate is
what random selection would give.

| Role | CLAP alone | Measurements alone | Base rate |
| --- | --- | --- | --- |
| quick_transition | **0.62** | 0.38 | 0.23 |
| gentle_entrance | **0.73** | 0.64 | 0.26 |
| action_impact | 0.59 | **0.64** | 0.16 |
| tension_build | 0.31 | **0.44** | 0.07 |
| emotional_texture | 0.31 | **0.42** | 0.12 |
| cinematic_reveal | 0.23 | 0.23 | 0.05 |

So the answer to "is the model better?" is: **it depends on the role, and
that is the useful finding.** CLAP wins the transitions by a wide margin -
which is exactly the role the measurements were weakest at (1.7x lift),
because foley and whooshes measure alike on every static reading. The
measurements win where physics is the point: a riser must climb, a texture
must sustain.

That makes the weight between them a per-role number, not a design opinion.
Sweeping it (weight `w` on the measurement term, CLAP fixed at 1):

| Role | w=0 | 0.25 | 0.5 | 1.0 | 2.0 | 4.0 | chosen |
| --- | --- | --- | --- | --- | --- | --- | --- |
| action_impact | 0.59 | 0.67 | **0.72** | 0.66 | 0.66 | 0.67 | 0.5 |
| gentle_entrance | 0.73 | 0.73 | **0.75** | 0.72 | 0.70 | 0.70 | 0.5 |
| tension_build | 0.31 | 0.38 | 0.41 | **0.44** | 0.44 | 0.44 | 1.0 |
| emotional_texture | 0.31 | 0.33 | 0.36 | 0.41 | 0.44 | **0.45** | 4.0 |
| cinematic_reveal | 0.23 | 0.22 | 0.22 | 0.22 | 0.23 | **0.25** | 4.0 |
| quick_transition | **0.62** | 0.58 | 0.45 | 0.42 | 0.41 | 0.39 | 0.0 |

Fusion beats both parents on four of six roles. On transitions the
measurements do not merely fail to help - they **drag the model down** from
0.62 to 0.42, which is why the weight there is 0: the model alone decides.

Honest limits of this table. The labels are the curator's categories, which
are coarse and occasionally wrong, so these are weak-label numbers. The
queries are three hand-written phrases per role; a better phrase bank would
move the CLAP column. 530 files is small enough that a fitted weight can
overfit, which is why the weights are four coarse steps rather than a
continuous fit, and why `scripts/sound-model/bench.py` ships: the numbers
above are reproducible, and a change to the model or the queries must be
re-measured against this table rather than argued about.

## 4. The record per asset

Every field is either measured, modelled or derived, and the record says
which. `index` writes one of these per file to `sound-model-index.json`.

SFX:

```json
{
  "file": "reverse-whoosh-medium-strong.ogg",
  "identity": {"duration": 0.84, "sampleRate": 48000, "channels": 2},
  "yamnet": {"frames": 1, "labels": [
      {"label": "Whoosh, swoosh, swish", "score": 0.305},
      {"label": "Sound effect", "score": 0.271}]},
  "clap": {"dim": 512, "embedding": [0.021, -0.014, "..."],
           "description": [{"text": "a fast swish transition", "similarity": 0.591}]},
  "soundSense": {"attackMs": 448.0, "decaySec": 0.064, "peakAt": 0.6379,
                 "peakFraction": 0.7654, "brightnessHz": 2969.2,
                 "rolloffHz": 5359.4,
                 "bands": {"sub": 0.0043, "low": 0.0765, "mid": 0.3342,
                           "presence": 0.4727, "air": 0.1124},
                 "transients": 1, "flatness": 0.4451, "pitchHz": 406.25,
                 "silent": false, "duration": 0.8333},
  "quality": {"peakDbfs": -14.3831, "rmsDbfs": -32.8262, "crestDb": 18.4431,
              "flags": []},
  "roles": {"quick_transition": 0.9843, "cinematic_reveal": 0.0635, "...": 0.0},
  "role": "quick_transition", "confidence": 0.95,
  "syncOffset": 0.6379,
  "trim": {"usableStart": 0.0, "usableEnd": 0.702,
           "advice": ["tail after 0.702s adds nothing"]}
}
```

`syncOffset` is where the peak is: a cut that should land on the transient
starts the clip this far in. `trim.advice` is only ever written from a
measurement - a tail is cut because the energy is gone, never because a
label said so.

Music gets the same identity and CLAP embedding plus moods, and is marked
`"kind": "music"` when YAMNet's music classes carry the file. Tempo, beats
and sections are **not** in this record: they come from the Rust music
analysis, which already exists, and joining the two is the wiring step in
§7.

## 5. Cost, measured on this machine

| What | Time | Notes |
| --- | --- | --- |
| `index` over 530 sounds, both models | **136 s** (0.26 s/file) | includes the Sound Sense measurement pass and two decodes per file |
| `rank` over the 530 from the cache | **0.1 s** | no inference: one text embedding, then dot products |
| `describe` one file | ~0.3 s | the Director's on-demand path |
| index file | 3.2 MB | 512 floats per file at 4 dp |

That is the whole performance argument: inference happens **once per asset**,
never on the playback or scrubbing path, and the ranking the Director asks
for mid-session is a dot product against a cached matrix.

The cache is stamped with the model files' size and mtime **and** with
`MODEL_INDEX_VERSION`, which must be bumped whenever a record means something
different. Changing the YAMNet aggregation in this session is exactly such a
change, and the first index built with the old meaning was silently reused
until the version moved. Same lesson as `sound_sense.py`'s stamp: a cache
that cannot see a change lies.

## 6. Laptop now, better hardware later, without a rewrite

The models are files in a directory, addressed by name:

```
$FUNCUT_MODELS, or ~/.local/share/funcut/models/
    clap_audio.onnx        34 MB   int8, htsat-unfused
    clap_text.onnx        127 MB   int8
    clap_tokenizer.json   2.1 MB
    yamnet.onnx            16 MB
    yamnet_class_map.csv
```

Swapping to the larger music model is replacing one file and re-indexing; the
sidecar, the record shape and the Director's calls do not change. That is the
"do not replace it when the hardware improves" requirement: the interface is
the model's *role*, not its weights.

| Tier | Audio encoder | Why |
| --- | --- | --- |
| laptop (now) | `htsat-unfused` int8, 34 MB | measured above, 0.26 s/file |
| bigger machine | `larger_clap_music_and_speech` int8, 78 MB | music-first; re-run the benchmark before adopting |
| either | YAMNet, 16 MB | small enough that there is no reason to tier it |

Do not ship a bigger model because it is newer. Ship it when the table in §3
moves.

## 7. What is wired, and what is not

Wired and tested: the four tools (registered as the `sense_sound_model`
MCP tool, on the Director's default allowlist), the cache, the fused
ranking with the measured weights, the graceful absence path, and
nineteen selftest checks (the ONNX checks skip cleanly when numpy is
absent). The Director skill teaches all four actions: Phase 18 runs
`plan`, `prompts/sfx_design.md` is the doctrine and
`prompts/tool_reference.md` carries the full schema.

Not wired, and this is the honest gap:

1. **AUTO EDIT's own sfx stage.** `AUTO EDIT` (the one-click spine)
  still ranks sounds from shelf metadata and places on the manifest's
  `syncAt`; the Director skill's measured path (above) is the agent's
  route. Feeding AUTO EDIT's sfx stage through `rank` + `plan` +
  `check_placement` is the remaining integration.
2. **The BGM candidate page.** `bgm_search` still ranks text over
  title/artist/category; the shelves' audio-aware candidate ranking
  (CLAP over fetched candidates) is taught to the Director in
  `prompts/music_and_duck.md` but not built into the shelf's own
  search.
3. **The download UI.** The settings sheet already has exactly the row this
   needs: `ModelData` in `src/crates/concat/ui/dialogs/settings.slint` (id,
   name, note, size, accuracy pips, installed, active, downloading, progress,
   transferred, eta, problem, manual). A CLAP row is a row, not a new dialog,
   and the Rust side follows `concat-vision/src/models.rs`: pinned URL,
   licence, size, hash, download into the data directory.
4. **The music join.** Beat grid and sections from the Rust analysis, merged
   with the CLAP music embedding, into one BGM record.

Pinned sources for that work, all verified today:

```
clap audio   https://huggingface.co/Xenova/clap-htsat-unfused/resolve/main/onnx/audio_model_quantized.onnx
clap text    https://huggingface.co/Xenova/clap-htsat-unfused/resolve/main/onnx/text_model_quantized.onnx
clap tok     https://huggingface.co/Xenova/clap-htsat-unfused/resolve/main/tokenizer.json
yamnet       https://huggingface.co/zeropointnine/yamnet-onnx/resolve/main/yamnet.onnx
yamnet names https://huggingface.co/zeropointnine/yamnet-onnx/resolve/main/yamnet_class_map.csv
```

The YAMNet file is a **community ONNX conversion** of Google's Apache-2.0
weights. Record both the conversion and the original in
`THIRD_PARTY_NOTICES.md`, and check the conversion against the TF original
before shipping it as a download: a conversion that is subtly wrong still
produces plausible labels.

## 8. Porting the front-end, if it ever leaves Python

The CLAP front-end is pure numpy here and matches the reference extractor
(`transformers.ClapFeatureExtractor`) to **1e-5** on the shipped shelf. Three
details cost a debugging session each, and a port that gets any of them wrong
still produces embeddings - just meaningless ones:

1. The scale is `10*log10`, floored at **-100 dB**. Not `log10`.
2. The default (`rand_trunc`) path uses the **slaney** filterbank - librosa's,
   peaks around 0.018 - not the torchaudio table (`mel_filters`), which peaks
   near 1.0. Picking the wrong one gives features that look plausible and
   rank nothing.
3. Short inputs are tiled `floor(10 s / length)` times and then **zero
   padded** to 10 s. Tiling to length and truncating shifts every later frame.

Acceptance test for any port: 10 s of 48 kHz mono in, `(1, 1, 1001, 64)` out,
and `max |difference| < 1e-3` against the reference extractor on five shipped
files. `scripts/sound-model/bench.py` also prints the P@64 table, so a port
can be checked against the numbers in §3 and not just against its own output.

## 9. What would make this wrong

- Trusting the CLAP column on `tension_build` (0.31, barely above the 0.07
  base rate). The measurements carry that role and the weight says so.
- Trusting YAMNet's label as a decision. `impact-metal-sci-fi.ogg` comes back
  "Sound effect 0.38, Heart murmur 0.35" - a low-frequency hit looks like a
  heartbeat to an event classifier trained on everyday audio. The label is
  evidence, not an answer.
- Adding a third model before the first two are wired into the Director. The
  gap is not model quality; it is that nothing calls this yet.
- Believing the index is fresh because the file exists. It is fresh when the
  stamp matches.

## 11. The planner: every signal, one sound design

The model layer answered "what is this file". The planner answers the question the Director actually has: *what sound goes where, how loud, and why.*

`sound_model.py plan` takes one spec and returns one plan:

```json
{"tool": "plan", "index": "...", "durationSec": 45, "voiceLufs": -17.5,
 "arc":    {"emotion": "fear", "acts": [{"name": "turn", "start": 24.0}]},
 "beats":  [{"at": 13.0, "end": 22.5, "emotion": "surprise", "intensity": 0.8,
             "sentenceType": "question", "pitchDirection": "rising",
             "pauseBefore": 0.6, "emphasis": [{"word": "noise", "at": 18.4}]}],
 "visual": {"cuts": [5.6, 12.0], "events": [{"at": 26.4, "kind": "reveal"}],
            "motionHits": [18.4]},
 "bgm":    {"candidates": [{"file": "...", "bpm": 88, "cutSafePoints": [23.6]}]},
 "style":  {"maxAccentsPer10s": 3, "minAccentGapSec": 0.45}}
```

Every field of the reply cites the input that produced it:

| Reply field | Comes from |
| --- | --- |
| `accents[].why.signal` | the signal that named the moment (`beats.emphasis`, `visual.cuts`, `arc.acts.turn`, ...) |
| `accents[].role` | the context, the emotion, the intensity, the pitch direction, the pause |
| `accents[].startAt` / `peakLandsAt` | the asset's measured `syncOffset`, so the transient lands on the word |
| `accents[].gainDb` | the asset's measured peak against the voice's own loudness, clamped to -24..+6 dB |
| `silence[]` | a pause long enough to hold before a reveal |
| `warnings[]` | a moment refused, and which constraint refused it |
| `music.changeAt` | the nearest cut-safe point to the story's turn |
| `music.ducking[]` | the narration spans the bed must sit under |

### The rules, and the run that corrected them

The first real run (45 s story, 530-sound shelf) placed ten accents in 1.4 s and exposed four defects, each now a selftest assertion:

1. **Every accent got the same gain** — the ranked candidates did not carry `peakDbfs`, so the level fell back to a constant. The candidates carry the measurement now, and the gain is `target - measured peak`.
2. **Two signals naming one instant were counted as two accents** — a reveal the vision read and the stressed word both landed on produced a "density" warning instead of one accent citing both. They merge now, and the beat (which carries the pause) survives the merge.
3. **The payoff was dropped to protect a cut** — three accents inside ten seconds meant the story's reveal at 26.4 s lost to a cut at 22.5 s. A stronger moment now evicts the weaker one, and the plan records which one gave way and why.
4. **A reveal got no sound because of a cut 200 ms later** — the duration filter measured the gap to a moment that was about to be suppressed. The gap is now measured to the next moment that will survive, and never past the end of the line being spoken.

Measured on the shelf: `selftest` 21/21 with models, plan of a 45 s story in **1.4 s** (530 candidates, 15 moments, one query embedding each).

## 12. Parallel indexing

`index` measures every asset once, so it is the only slow step. It now runs a process pool:

| Workers | 530 sounds | Notes |
| --- | --- | --- |
| 1 | 140 s | serial, one decode at a time |
| 2 (default) | **91 s** | 1.54x, ~360 MB peak for the two model sets |

`workers: 1` disables the pool, `FUNCUT_SOUND_WORKERS` sets it, and the ceiling is the machine's core count. Two is the default because the models are ~180 MB of weights *per process*: a laptop that swaps mid-index is slower than one that does not. If the pool cannot start (no shared memory, a restricted sandbox), the reply says so in `poolError` and the index runs serially rather than failing.

## 13. Where the models come from, and where they land

The five files are in the app's own registry (`concat-vision/src/models.rs`, `ModelId::ClapAudio` and friends) with their exact sizes and SHA-256, so a download is verified the way the vision models are. `ModelId::kind()` splits the table into `vision` and `sound`, which is what a grouped settings list needs.

`_models_dir()` reads, in order: the spec's `models`, `FUNCUT_MODELS`, `<app dir>/portable/cutout-models`, `<data>/app.concat.editor/cutout-models` (where the settings sheet downloads), then the older `funcut/` and `concat/` layouts. A folder that holds only the vision models is not chosen for the sound models — the reader looks for `clap_text.onnx` or `yamnet.onnx` before it settles.

**Still open:** the settings sheet's model list is built from the speech models, not from `concat_vision::models::MODELS`, so the sound rows need a new section in `src/crates/concat/src/panes/settings.rs` plus a download worker for them. The registry entries, hashes and the sidecar's search order are in place; the sheet is the remaining piece.

## 14. What measuring the real shelf changed

The gates in §11 were calibrated on fixtures. Running them over all 530 shipped
sounds found two things the fixtures could not.

**A bed scored as a gesture.** `sustain` saturates at 2.5 s, so a 4.5 s dark
room tone and a 2 s boom read the same on it, and `ambience-dark-1` duly won
`cinematic_reveal` (0.91) ahead of a sub drop. The fix is a `gestureGate` that
gives full credit under three seconds of body and none past eight, applied to
the two roles that are accents rather than beds (`action_impact`,
`cinematic_reveal`). The ambience falls to 0.27 and reads as what it is,
`emotional_texture`; `bass-sub-drop-long` stays at 0.90.

**Two reads of one payoff became two booms.** An arc placing `reveal` at 22.0
and a vision read placing the reveal frame at 22.4 are the same moment. They
were merged only within 20 ms, so the plan emitted a boom on the frame and a
"no candidate fits" warning 400 ms earlier. Two reveals within 600 ms now merge
into one accent, keeping the stronger read (the measured frame over the planned
boundary), and the merged moment cites every signal that agreed.

Measured on the shelf, no models loaded:

| Step | 530 sounds |
| --- | --- |
| `index` | 19 s, 0.6 MB of JSON |
| `plan` (9 moments) | 1 s |

Every record now carries `evidence` — the normalised terms the fits were
computed from — so a Director can audit a fit instead of trusting it, and each
plan accent carries the shelf `id` the file is named after, so a plan can be
executed without a second lookup.

**One thing measurement cannot settle.** `riser-braam-rise` and
`bass-sub-drop-medium` are both heavy, tonal and sustained with an early peak;
without CLAP the two are the same numbers, and which one a reveal wants is a
semantic judgement. With no models the planner picks risers that peak on the
cut, which is a real choice (the rise lands on the frame), but it is not the
same as knowing a braam from a boom. That distinction is what the audio-text
model is for, and the plan's `why` block is what makes the difference visible
instead of hidden.

`MODEL_INDEX_VERSION` in `sound_model.py` must be bumped whenever a record means
something different, because the index cache keys on it: a measurement change
without a bump leaves every user with stale roles and a re-index that finishes
in 0.01 s.
