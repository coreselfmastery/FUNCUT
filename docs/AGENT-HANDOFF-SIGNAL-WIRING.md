# Handoff: wire the remaining sense readings into AUTO EDIT

You are picking up a measured, documented gap and closing it. This file is
the whole brief: the state, the rules, the batches, the verification, and
the traps. Read it end to end before touching code.

---

## 1. The repo and the current state

- Repo: `coreselfmastery/FUNCUT`, a fork of Concat: Rust + Slint video
  editor, ~220k lines, crates in `src/crates/*`. Read `AGENTS.md` and
  `docs/AI-PATH-AUDIT-2026-11-26.md` first; the second is the audit this
  work implements (§12 is the specification).
- Work on a new branch off `main`. **PR #229**
  (`forge/signal-coverage-ledger-2026-11-26`) already landed the map and
  the measuring instrument; if it is not merged yet, branch from it.
- The gap, measured, not guessed:

```
$ python3 scripts/check-signal-coverage.py
sense readings: 75
asked by AUTO EDIT: 10  (13%)
missing: 65
taught in signal_matrix.md: 75/75
```

- Total readings in the product: **145** = 70 voice-profile readings
  (`voice_signals`) + 75 door readings (8 doors, 83 dispatched actions
  minus 8 machinery). The button asks 10. **Your job is to take that
  number to 75/75 with every reading wired to a decision, and to keep the
  70 voice readings fully spent.**
- The Director path reads `.claude/skills/funcut-director/prompts/signal_matrix.md`,
  which now teaches all 145. It is taught, **not enforced**: see Batch 9.

## 2. Build environment (do this first, it takes ~20 minutes)

The box has 4 cores and 15 GB RAM. **Build with `-j2`**; higher
parallelism OOMs and the container can be restarted mid-build.

```sh
# toolchain: exactly the pin in src/rust-toolchain.toml (1.93.1)
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
  --default-toolchain 1.93.1 --component rustfmt clippy
. "$HOME/.cargo/env"

# system packages bindgen/cmake/openssl need
sudo apt-get update && sudo apt-get install -y build-essential pkg-config \
  cmake libclang-dev libssl-dev

# the pinned FFmpeg 8 dev bundle and the ONNX Runtime, from the repo's own
# digest-checked fetcher (never from upstream's latest)
cd /workspace/project/FUNCUT
scripts/ci/fetch.sh ffmpeg-linux64 /tmp/ffmpeg.tar.xz
mkdir -p ffmpeg-dev && tar -xf /tmp/ffmpeg.tar.xz -C ffmpeg-dev --strip-components=1
scripts/ci/fetch.sh onnxruntime-linux-x64 /tmp/ort.tgz
mkdir -p .ort-shared && tar -xzf /tmp/ort.tgz -C .ort-shared --strip-components=1
```

Export these in every shell you build in:

```sh
export FFMPEG_DIR=$PWD/ffmpeg-dev
export PKG_CONFIG_PATH=$PWD/ffmpeg-dev/lib/pkgconfig
export ORT_LIB_LOCATION=$PWD/.ort-shared/lib ORT_PREFER_DYNAMIC_LINK=1
export LD_LIBRARY_PATH=$PWD/ffmpeg-dev/lib:$PWD/.ort-shared/lib
export LIBCLANG_PATH=/usr/lib/llvm-19/lib     # adjust to your libclang
```

Baseline before you change anything:

```sh
cd src
cargo check -p concat-mcp -j2      # the fastest "does it build"
cargo test -p concat-mcp -j2       # the guards must pass before you start
cd .. && python3 scripts/check-signal-coverage.py
python3 scripts/check-signal-matrix.py
python3 src/crates/concat-mcp/sense/visual_sense.py selftest
```

If the link step fails with `rust-lld: error: duplicate symbol: onnx::...`,
read `AGENTS.md` ("Building and testing"): use the shared runtime and, if
it persists, remove the conflicting static library from the link set.
**Never** mask it with `--allow-multiple-definition`.

## 3. The architecture you are editing

`src/crates/concat-mcp/src/director_program.rs` builds the AUTO EDIT
program as an ordered list of stages. Each stage is a `Step` (a fixed
call) or a `DerivedStep` (a function that reads the run log and returns
the next calls - `DerivedStep::react` is the machinery you will use most).
Approximate stage order (line numbers drift; search the `Stage::` names):

| Stage | Line | What it does |
|---|---:|---|
| `HandoffVoiceover` | 8114 | copies the project, fans the takes |
| `AudioRepair` | 8178 | `diagnose_take`, `enhance_speech`, `repair_by_diagnosis` |
| `Transcribe` | 8206 | words + times |
| `VoiceSignals` | 8230 | the 70 voice readings |
| `Understand` | 8241 | `emotion`, discourse, `detect_music_segments` (:8251) |
| `Plan` | 8339 | writes `edit-plan.json`, `plan_validate` |
| `Timeline` | 8673 | the cut |
| `Gather` | 8900 | stock: `measure_clips` (:9174, `samples:6, width:64, maxClips:3`) |
| `Captions` | 9890 | `caption_groups()` (:925) |
| `Motion` | 10340 | presets from the emotion read |
| `Music` | 10641 | bed choice, duck |
| `Sfx` | 11311 | accents, `sounds_search` (:11474, :11555, :11677) |
| `Preview` | 11699 | `watch_render`, `craft_critique` (:11752) |
| `Revise` | 11790 | `edit_apply` batch, re-render, `craft_critique` (:11851) |
| `TimelineReady` | 11875 | saves |
| `Verify` | 11891 | `collision_check` |
| `Watch` | 11911 | final `craft_critique` (:11946) |

Helpers you will need: `visual_reads(log)` and `visual_read_of(&reads,
path)` (`src/crates/concat-mcp/src/visual_read.rs`), `voice_read_of(voice,
take)` (`voice_read.rs`), `line_of`/`lines_of` for the transcript.

## 4. The rules - these are the deliverable, not the paperwork

1. **A reading is spent only when it changes something.** Adding a call
   and ignoring the reply is the bug this whole effort exists to fix. If
   a reading cannot change anything for this material, it is
   `not-applicable` **with the measured reason**.
2. **Parse defensively.** Every new reply is `serde_json::Value`. Read
   fields with `.get(..).and_then(..)`; when a field is missing or the
   reply is `{"ok": false}`, **fall back to the current behaviour**. A
   missing model must never fail a run - it must leave the edit exactly
   as it is today.
3. **Never widen a guard to make it pass.** The three guards in
   `director_program.rs` (`every_signal_family_has_a_reader`,
   `every_reading_inside_a_family_has_a_consumer`,
   `every_vocabulary_family_names_the_decision_it_informs`) are the
   contract. When you add a reading, wire it to a decision and log it with
   its confidence. When you deliberately leave one unspent, add it to
   `NOT_SPENT` **with the reason**.
4. **`signalLog` must name an accepted family.** `SIGNAL_VOCABULARY` is in
   `src/crates/concat-mcp/src/tools.rs:3751`; the picture families
   (`visualMotion`, `shotScale`, `camera`, `visualQuality`, `saliency`,
   `scene`) are at :3779. Extending the vocabulary with `assemble`,
   `select`, `mix`, `grade`, `critique` is allowed **only** together with
   a real gate for each, and with the matching change in
   `scripts/check-signal-matrix.py` so the matrix stays in sync.
5. **One batch per PR.** Each batch must leave `main` buildable, tested,
   and measurably better. Do not bundle.
6. **Do not touch the ten calls that already work** (`measure_clips`,
   `generate`, `enhance_speech`, `diagnose_take`, `repair_by_diagnosis`,
   `detect_music_segments`, `analyze_discourse`, `segment_topics`,
   `detect_speech_acts`, `extract_claim_graph`) except to make them
   adaptive (Batch 1).
7. **Every new call is logged in the receipt** with: reading, decision,
   confidence, evidence. `src/crates/concat-mcp/src/receipt.rs` is where
   the run's evidence lives (`measured_voice_family` at :940).

## 5. The batches, in order

### Batch 0 - baseline (half a day)

Build, run the tests, run the three checks above, and record the numbers
in the PR description. If `cargo test -p concat-mcp` is red on `main`,
stop and report it - do not build on a red tree.

### Batch 1 - capability pass (highest value: makes every download count)

**Problem.** The program never asks what is installed: `models_list`
appears 0 times in `director_program.rs`; `sense_visual {action:"models"}`
0 times. A downloaded checkpoint is indistinguishable from a missing one.

**Do.**
1. Add `Stage::Preflight` as the first stage, before `HandoffVoiceover`.
   It issues two calls:
   `models_list {}` and `sense_visual {"action": "models"}`.
   Both are cheap; both answer with `present`/`ready` per model.
2. In `Stage::Gather`, make the existing `measure_clips` call adaptive by
   reading that reply from the log:
   - `enrichDoors`: the number of `ENRICH_DOORS`
     (`sense/visual_sense.py:1710`) whose `ready` is true, capped at 6;
     today the Python default is 4, so only lower it when a door is
     absent.
   - `maxClips`: 6 when any door is ready (the doors are per-clip and
     cheap), 3 otherwise.
   - `samples`: 8 when doors are ready, 6 otherwise.
3. If the Preflight reply is missing entirely, keep today's exact values.
4. Log the capability answer in the receipt (`doors: {ready, absent}`) so
   the run says what the machine had.

**Done when** `cargo test -p concat-mcp` passes, a run on a machine with no
models behaves exactly as today, and the receipt names the doors.

### Batch 2 - the take before it is measured (AudioRepair)

**Do.** In `Stage::AudioRepair`, before `diagnose_take`, call
`sense_voice_quality {"action": "classify_recording_quality", "audio":
take}`. When the reply names a chain (`noisy`, `hum`, `rumble`, `phone`,
`clipped`), run `remove_hiss_hum_rumble` first and `normalize_loudness`
after the repair; otherwise keep today's `enhance_speech ->
repair_by_diagnosis` order. Add `measure_reverb` and `detect_restarts`
next to it and log their readings; `detect_restarts` feeds `assemble`
(which attempt survives) - use it only to *mark* the restart in the plan,
do not cut words without the pause span agreeing.

**Decision:** `mix` + `assemble`. **Family:** `forensic`, `loudness`.
**Done when** a noisy take gets the noise chain, a clean take gets the
same result as today, and the receipt cites the classification.

### Batch 3 - captions that do not drift (Captions)

**Do.** In `Stage::Captions`, before `caption_groups()` (:925):
1. `sense_captions {"action": "align_captions", "audio": take, "words": ...}`
   - when the reply carries corrected times, rebuild the line word times
     from it; when it does not, keep the transcript's times.
2. `sense_captions {"action": "clean_subtitles", "lines": ...}` - when the
   reply carries cleaned text, use it for display and keep the original
   for the `signalLog` evidence; otherwise keep today's text.
3. `sense_captions {"action": "detect_ai_voice"}` - record it; when it
   says synthetic, the receipt says so (honesty, not a gate).

**Decision:** `caption`. **Family:** `words`, `sentence`, `language`.
**Done when** a deliberately drifted take is corrected (make a fixture:
shift every word time by +0.4 s and assert the group boundaries move
back), and an absent door changes nothing.

### Batch 4 - sound that lands (Sfx)

**Do.** In `Stage::Sfx`:
1. `sense_sound_model {"action": "plan", ...}` with the edit's own beats,
   arc, visual events and bed structure - the accent map replaces the
   current keyword guess when it answers.
2. For each accent, `sounds_search` as today, then
   `sense_sound {"action": "rank_sounds", "query": ..., "candidates":
   [...]}` and take the top-scoring candidate; fall back to today's first
   hit when the reply is empty.
3. `sense_sound {"action": "describe_sound"}` on the winner; reject a file
   whose family contradicts the moment and take the next candidate.
4. `sense_sound {"action": "check_placement", ...}` after placing: when
   the measured peak differs from `syncAt` by more than 100 ms, shift the
   placement to the measured peak. This is the fix for the 61 of 530
   files that lie about `syncAt` (`director_program.rs:11519`).

**Decision:** `mix`. **Family:** `musicSegments`, `arc`.
**Done when** an accent lands on the measured peak in the rendered
timeline, and a shelf with no models behaves as today.

### Batch 5 - a bed that is music (Music)

**Do.** After the bed is imported and before the duck:
`sense_sound_model {"action": "describe", "audio": bed}`; reject a bed
whose description is not music or not the requested mood and take the
next candidate. Add `identify_music` and `analyze_music_structure` to get
the bed's own change points, and key the duck on the measured 100 ms
loudness curve (PR #226's change) rather than merged line spans.

**Decision:** `bgm`. **Done when** a non-music file is rejected and a
music file is accepted unchanged.

### Batch 6 - the other voice-side doors (Understand)

**Do.** In `Stage::Understand`, add the 6 `sense_prosody_style` reads
(`score_emphasis_type`, `detect_speaking_style`, `measure_rhythm_meter`,
`detect_tempo_changes`, `detect_accent`, `measure_vocal_tract`), the 6
`sense_voice_quality` reads not yet called, and the 6 discourse reads not
yet called (`classify_speech_acts`, `score_readability`,
`score_script_quality`, `detect_sarcasm`, `detect_code_switching`,
`detect_cross_lingual_emotion`). Each must feed a named decision:
`measure_rhythm_meter`/`detect_tempo_changes` -> `pacing` and the cut
grid; `detect_accent` -> accent timing; `score_emphasis_type` -> caption
emphasis; `detect_sarcasm` -> `critique` (never read a sarcastic line
straight); `detect_code_switching` -> caption script per span.

**Done when** every one of the 18 appears in the receipt with a decision
or a stated reason it did not apply.

### Batch 7 - the picture's doors (Gather)

**Do.** After `measure_clips`, a `DerivedStep::react` over the capability
reply issues the door calls for the doors that are `ready`, in this
priority (stop when the budget is spent):

1. `shot_boundaries` - the cut grid (the timeline's cut may land on a
   boundary, not only on a pause).
2. `action_class` - the verb, to match B-roll to what the sentence *does*.
3. `video_embedding`/`visual_embedding` - semantic match + near-duplicate
   suppression (one embedding door per run: siglip2 > clip > dinov2).
4. `depth_map`/`subject_mask` - the caption band's clearance and the
   reframe crop.
5. `tracks` - subject continuity across a cut.
6. `aesthetic` - the ranking term (never the veto).
7. `face_emotion`/`gaze` - the footage's register, and the band side.

Parse all of them in `VisualRead` (`visual_read.rs:62`), spend them in the
existing veto/rank/in-point functions, and cite the accepted family
(`visualMotion`, `shotScale`, `camera`, `visualQuality`, `saliency`,
`scene`). Respect the arbitration rules in `signal_matrix.md` - one
question, one answer, the loser recorded as corroboration.

**Done when** a stock candidate is chosen differently because a door said
so, and the run on a bare machine is byte-identical to today.

### Batch 8 - the receipt's coverage ledger (the proof)

**Do.** Add a `coverage` section to the receipt: for each of the 145
readings, `asked | answered | absent | not-applicable(reason) |
skipped-budget(tier)`; `missing` fails the acceptance. Compute it from the
run log, not from a static list, and unit-test it with a log that omits
one reading (it must report `missing`).

**Done when** `acceptance::check` fails a run that silently skips a
reading that applied.

### Batch 9 - hold the Director to the same bar

**Do.** Add the doors a voiceover-led run must reach to
`DIRECTOR_REQUIRED` (25 names today, `tools.rs`), and make
`docs/` + `SKILL.md` say it. Then flip the CI step to
`python3 scripts/check-signal-coverage.py --require-button` so the number
cannot regress.

### Batch 10 - depth tiers (speed without lying)

**Do.** A tier chosen from material size + install: `fast` (classical,
`enrich: false`), `standard` (classical + enrich doors + sound/captions),
`thorough` (every installed door on every surviving candidate). Default
`thorough` under ~60 clips, `standard` above. The tier is named in the
ledger, so "100%" is always qualified.

## 6. Verification - run all of it before every push

```sh
cd src
cargo fmt --check
cargo clippy --all-targets -j2 -- -D warnings
cargo test -p concat-mcp -j2
cd ..
python3 scripts/check-signal-coverage.py      # the delta must be positive
python3 scripts/check-signal-matrix.py        # must stay green
python3 src/crates/concat-mcp/sense/visual_sense.py selftest
```

CI (`.github/workflows/ci.yml`) must be green on: **Format, locales and
models**, **The analysis senses against real fixtures**, **Engine tests
and lints**. Those three block a merge; the others are informational.
A step with no conclusion and a log that stops mid-line is a runner
death - retry it, do not "fix" the code for it.

## 7. Traps that have already cost this repo time

- **Do not** raise build parallelism above `-j2` on a 4-core box.
- **Do not** use `--allow-multiple-definition` for the ONNX link error.
- **Do not** widen a guard, a timeout, or a threshold to make a run pass.
- **Do not** add a call whose reply nothing reads: that is the bug.
- **Do not** let a missing model change the edit. Fall back, log
  `absent`, continue.
- **Do not** renumber or reorder stages without updating the stage tests
  and the receipt's expectations.
- **Times are seconds, keyframe positions are 0-1 fractions, title sizes
  and offsets are frame fractions.** The tool descriptions repeat this;
  believe them.
- The open PRs #222/#224/#225/#226/#227 overlap with Batches 3, 4, 7 and
  the guards. Read each before writing code in its area: if it is merged,
  build on it; if it is open, do not duplicate it - finish it or wait.

## 8. Definition of done

1. `python3 scripts/check-signal-coverage.py` prints **75/75 asked** (or
   names every remaining reading with a measured reason), and the 70
   voice readings are all still spent.
2. Every reading that applies to a run appears in the receipt with a
   decision, a confidence and its evidence; `not-applicable` entries
   carry the measured reason.
3. A machine with no models produces the same edit as today; a machine
   with all models produces a visibly different, better one - and the
   receipt says which door decided what.
4. The three required CI checks are green, `cargo fmt` and
   `clippy -D warnings` are clean, and the new tests cover: fallback on a
   missing reply, the drift correction, the placement shift, the coverage
   `missing` case.
5. `docs/AI-PATH-AUDIT-2026-11-26.md` §12 is updated with the measured
   before/after numbers.
