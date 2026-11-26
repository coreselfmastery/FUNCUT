# The AI path and the button: what each one actually spends

**Tree audited:** `main @ 7b91fc5` (2026-11-26), plus the five open
branches (#222, #224, #225, #226, #227) read as patches, not as done
work.
**Scope:** the two ways FUNCUT makes an edit without a human cutting it -
the **AUTO EDIT button** (the deterministic spine,
`director_program::auto_edit_program`, 17 stages) and the **AI path**
(the Forge Director: an LLM driving the same server through
`funcut-director`) - measured against every signal, door and tool the
repository already ships.
**Method:** every number below was read out of this tree with `grep` and
`python`, never taken from a doc. The commands are in
[the appendix](#appendix-how-to-re-measure), so a later reader can
re-run them and find the doc lying if it is.

Companion documents: [`AUDIT-2026-11-26.md`](AUDIT-2026-11-26.md) (the
fifty-item signal-to-consumer audit), [`DIRECTOR-INTEGRATION-PLAN.md`](DIRECTOR-INTEGRATION-PLAN.md)
(the file-by-file wiring plan), [`VOICE-SIGNALS.md`](VOICE-SIGNALS.md)
and [`SOUND-SENSE.md`](SOUND-SENSE.md) (the two signal catalogues).

---

## 0. The answer, in one paragraph

The **audio** half of the button is genuinely intelligent and the
**picture and sound** halves are not, and the reason is arithmetic, not
judgement: of the **82 actions** the eight sense doors offer, the spine
calls **10 (12%)** - and eight of those ten are voice or audio readings
the run genuinely spends (four audio-repair, four discourse). The three
doors the repository spent 2026-11-25 building and merging
(`sense_sound` #213, `sense_sound_model` #215, the generated-audio
models #218) are **9 actions, 1 of which the button calls**
(`generate`). The picture door is **1 of 38**. So the answer to "is the
new work used?" is: **the generators are, the instruments are not** -
and five open PRs (#222, #224, #225, #226, #227) are precisely the
wiring that turns three of those instruments on
(`check_placement`, `rank_frames`, `describe`), taking the spine from
**10 to 17 of 75 readings** (§12). That is a start, not the finish: the doors stay
mostly unasked until Batch 2 below. All five PRs are `MERGEABLE` today;
none is merged. The
**Director** is a different story: it is *taught* every door (its skill
names them, with actions, in three places) but **nothing enforces it** -
a Director run that skips every door still earns `Accepted`, because the
acceptance gate only re-measures `voice_signals`, `emotion`,
`plan_validate`, `watch_render` and `collision_check`.

### The scorecard

| Layer | What exists | Button spends today | After #222/#224/#225/#226/#227 | Director (taught / enforced) |
| --- | --- | --- | --- | --- |
| Voice signals | 70 readings, 12 catalogue families | ~all 70 parsed, every family but `formants` has a named consumer | + `loudness.momentaryLufs` (the M-series curve), + speaker-split captions | taught / re-measured by the acceptance gate |
| Emotion | 7 labels + act + arc + template + gate | `extract` + `gate` (gate is **lexical**) | unchanged | taught / arc re-measured |
| Picture (`sense_visual`) | 38 actions; ~44 readings per clip; 30-door shelf | `measure_clips` **only**; 12 of ~44 fields parsed | + enrich doors parsed (`shotType`, `depth`, `action`, `aesthetic`); + `rank_frames` (CLIP pair) | taught / **not enforced** |
| Sound (`sense_sound`) | 4 actions (describe, rank, check_placement, index) | **0** | `check_placement` on every accent (#222) | taught / **not enforced** |
| Sound model (`sense_sound_model`) | 5 actions (describe, index, rank, plan, generate) | `generate` **only** | + `describe` on the bed (#226) | taught / **not enforced** |
| Voice quality | 6 actions | **0** | 0 | taught / **not enforced** |
| Prosody style | 6 actions | **0** | 0 | taught / **not enforced** |
| Captions | 5 actions | **0** | 0 | taught / **not enforced** |
| Audio repair | 8 actions | 4 (`enhance_speech`, `diagnose_take`, `repair_by_diagnosis`, `detect_music_segments`) | unchanged | taught / **not enforced** |
| Discourse | 10 actions | 4 (`analyze_discourse`, `segment_topics`, `detect_speech_acts`, `extract_claim_graph`) | unchanged | taught / **not enforced** |
| AI generation | 3 models (TTA-SFX, Stable Audio pad, TinyMusician bed) | **yes** - accents, ≤8 s bed, licence ladder | unchanged | taught |

The two rows to stare at are `sense_visual` and `sense_sound`: the
picture and the sound are the two halves of an edit that a keyword
search cannot fake, and they are the two the button barely touches.

---

## 1. The two paths, and how each is judged

| | AUTO EDIT button (deterministic) | Forge Director (AI path) |
| --- | --- | --- |
| Engine | `auto_edit_program` - 17 stages, `Runner::walk` | an LLM in a harness (`forge run`), `funcut-director` skill |
| Code | `director_program.rs:8097`, `auto_edit.rs:35` | `forge.rs:3797` (`run_auto_edit` is the spine; the Director is the sibling thread) |
| Decides with | Rust: `dead_air_cuts`, `choose_direction`, `signal_brief`, `critic_fix_commands` | prose: the skill's 25 phases |
| Judged by | stage gates + `acceptance::check` | exit code **+** `acceptance::check` |
| Evidence | `auto-edit/report.json`, receipt | the same receipt, plus whatever the agent wrote |
| Cost | minutes, CPU only | tokens, a model key, a skill install |
| Fails how | a stage gate fails the run; degraded readings **warn** (after #225) | any tool can be skipped silently; the acceptance probes seven things (plan, per-take voice, emotion, loudness, motion, collision, timeline) |

Both paths share the acceptance (`concat-mcp/src/acceptance.rs`): it
re-asks `plan_validate` on the saved `edit-plan.json`, re-measures
`voice_signals` per narration take, re-runs `emotion extract` for the
arc, then `timeline_read`, `watch_render` and `collision_check` - and,
only when an export was asked for, `motion_probe` and
`measure_loudness`. That is a strong gate for **the voice**, and no gate
at all for **the picture and the sound**.

### What the button gets right (so the plan below is not a rewrite)

- **Cut.** `dead_air_cuts` (`director_program.rs:305`) spends the VAD's
  `dead_air` class, refuses to tighten a take whose longest silence never
  reached 0.9 s, holds a beat after a hard line (`held_beat`), pads by
  80 ms, and cuts the talking-head A-roll **in lockstep** in the video's
  own span so picture and words never drift; every other clip is rippled
  by the removed timeline time.
- **Captions.** Grouped from the transcript's own words, fillers
  stripped, emphasis styled, and the band moved off the salient subject
  (`visual_focus_at`, `:1146`); #224 splits a group at a measured speaker
  change.
- **BGM.** `bed_query_chain` + `best_bed`, licence-gated, ducked with a
  shaped keyframe span, and a **generated** bed for cuts ≤ 8 s with the
  licence ladder (music model refuses a monetized plan → `sfx-large`
  pad) - `docs/SOUND-GENERATION.md:112`.
- **SFX.** Emphasis peaks only, family rotation, the manifest's
  `syncAt` offset, a gain per role, and generated accents when a model
  is installed.
- **Motion.** `motion_specs` turns the emotion read into presets,
  `motion_bake` lands them, continuity gates check the result.
- **Stock.** Licence gate (Pexels/Pixabay only), contact sheet, the
  pixel read, a continuity/drift/duplicate veto, and a lexical emotion
  gate.
- **Plan.** Brief, sections, shot budget, a three-direction bake-off
  (`choose_direction`), retention breaks, visual variety, the YouTube
  package.

Every one of those is real and tested. The gap is not "the button is
dumb"; it is "the button spends the voice's intelligence and not the
picture's".

---

## 2. The sense inventory: 82 actions, 10 spent

The eight doors and their actions, read out of each module's `TOOLS`
table (`selftest` is machinery, not a reading, and is not counted, so
the dispatch total is 83 and the counted total 82):

| Door | Actions | Button calls | Which |
| --- | ---: | ---: | --- |
| `sense_visual` | 38 | **1** | `measure_clips` (`:9174`, `samples:6, width:64, maxClips:3`) |
| `sense_sound` | 4 | **0** | - |
| `sense_sound_model` | 5 | **1** | `generate` (×3: accents, bed, pad) |
| `sense_voice_quality` | 6 | **0** | - |
| `sense_prosody_style` | 6 | **0** | - |
| `sense_captions` | 5 | **0** | - |
| `sense_audio_repair` | 8 | 4 | `enhance_speech`, `diagnose_take`, `repair_by_diagnosis`, `detect_music_segments` |
| `sense_discourse` | 10 | 4 | `analyze_discourse`, `segment_topics`, `detect_speech_acts`, `extract_claim_graph` |
| **Total** | **82** | **10** | 12% |

The spine's whole vocabulary is **84 call sites over 32 distinct
tools** (`auto_edit_program`, `director_program.rs:8097-11965`). The
rest of the 117-tool catalogue is served to a Director (109 names in
`DIRECTOR_ALLOWLIST`, 25 in `DIRECTOR_REQUIRED`) and untouched by the
button.

---

## 3. The voice: 70 readings, and the four families that vanish quietly

The voice is the half that works. 70 catalogue entries in 12 families
(`voice_signals.py:159`), parsed into `LineRead`/`TakeRead`
(`voice_read.rs:33`, `:187`), and held to a consumer by three Rust
guards that pass on this tree:

- `every_signal_family_has_a_reader` - every family in
  `SIGNAL_VOCABULARY` (`tools.rs:3751`, 33 names) has a reader or a
  stated reason. Only `formants` is declared `NOT_SPENT`.
- `every_reading_inside_a_family_has_a_consumer` - every `pub field`
  `voice_read.rs` parses is named by the program or read through a
  helper.
- `every_vocabulary_family_names_the_decision_it_informs` -
  `decision_for` (`:7277`) maps all 33 to a gate-accepted decision.

That is a genuine, machine-held contract - the strongest thing in the
repository - and it is why the voice work keeps landing without rotting.

**Where it still leaks:**

1. **Four families disappear without the Rust `voice-signals` binary.**
   Prosody, forensics, speakers and NER entities (54 of the 70 readings)
   are absent, the Python profile says so (`status: degraded`,
   `degradations`), `VoiceRead` parses it (`voice_read.rs:372`), and
   `signal_brief` carries it to the critic (`:7239`) - but the **stage**
   completes and the receipt says nothing. Every SFX accent, every duck,
   every tighten guard rides on the missing half. #225 and #224 both fix
   this (see §8).
2. **The loudness curve is measured and thrown away.** ebur128's
   M-series at 100 ms is folded per line; the duck keys on merged line
   spans. #226 travels `loudness.momentaryLufs` and keys the ride on it.
3. **Speaker identity had no consumer** - `.speaker` matched
   `.speaker_seconds` in the guard's substring matcher. #224 makes the
   matcher demand a whole access and gives the readings a real consumer
   (a caption split at the speaker change).
4. **`wordConfidence` cannot be verified.** The plan may cite it;
   `measured_voice_family` (`receipt.rs:940`) maps `words` →
   `wordConfidence` but **not** `wordConfidence` → itself, so a plan
   citing the family name is skipped by the evidence gate (§9, item 2).

---

## 4. The picture: ~44 readings a clip, 12 parsed, one door

`measure_clips` (`visual_sense.py:1612`) returns, per clip:

| Block | Leaves | Rust parses | Spent by |
| --- | ---: | --- | --- |
| `motion` | 5 | `peakAt`, `mean`, `still` | the in-point (`in_point()`, `visual_read.rs:140`, slides 0.25 s before `peakAt`); `mean`/`still` veto a still frame |
| `camera` | 11 | `kind`, `travel`, `cameraMoving`, `subjectMoving` | continuity direction, drift veto |
| `shotScale` | 5 | `scale` | cut rhythm, drift, duplicate-scale flag |
| `quality` | 7 | `verdict` | stock veto |
| `palette` | 3 | - | nothing (stated: "recorded, not spent") |
| `saliency` | 3 | `focus` | caption band |
| `scene` | 3 | `cuts` | montage veto |
| `composition` | 4 | - | inside `visualScore` only |
| `blur` | 2 | - | inside `visualScore` only |
| `models` (4 enrich doors) | 4-8 | - | nothing (#224 parses it) |
| `visualScore` | 1 | ✓ | the sheet's ranking |

So **12 of ~44** readings cross into Rust (`VisualRead::parse`,
`visual_read.rs:62-113`: twelve field paths), and the 30-door shelf is a
**4-door shelf** for the button (`ENRICH_DOORS`,
`visual_sense.py:1710`: `dover-mobile`, `film-shot-classifier`,
`depth-anything-v2-small`, `movinet-a0`) - and their answers are dropped
until #224.

Two honesty defects in the map that is supposed to be the honesty layer:

- `CONSUMERS["motion.onsets"] = "the visual sync the SFX stage spends"`
  (`visual_sense.py:126`). **Nothing reads picture onsets.** `visual_read.rs`
  has no `onsets` field, and the SFX stage's onsets are the *voice*
  profile's `TakeRead.flux_onsets`. Either parse and spend it, or say
  "recorded".
- `CONSUMERS["composition"] = "the stock ranking's taste term **and the
  caption band**"`. The first half is true (inside `_visual_score`); the
  second is not - the caption band reads `saliency_focus` only.

`sense_visual`'s own selftest holds every measured key to a *named*
consumer, so these two claims are exactly the kind the selftest cannot
catch: a name is not a call site. The Rust side has no such map at all
(#42 in the fifty-item audit).

### Do the new signals get the old signals' discipline?

No. This is the cleanest way to state the whole gap, so it is worth a
table:

| Discipline | Voice (the old signal) | Picture (new) | Sound (new) |
| --- | --- | --- | --- |
| A Rust guard that every family has a reader | yes | no | no |
| A Rust guard that every parsed field has a consumer | yes | no | no |
| A guard that every family names its decision | yes | no | no |
| A reading→decision matrix in the docs, held by CI | yes (`signal_matrix.md`) | no | no |
| The acceptance re-measures the family | 19 of 33 names | no | no |
| The plan cannot cite a reading the run did not take | yes | no | no |

The vocabulary a plan may cite (`SIGNAL_VOCABULARY`, 33 names,
`tools.rs:3751`) was built for the voice's twelve catalogue families;
the picture's six and the sound's were added beside them without the
discipline that makes the voice trustworthy travelling with them. That
is why the same sentence keeps being true in every audit since #209:
*the extraction layers are real and the consumption is not.* The guards
in §11 are the fix, and they are cheaper than any new model.

---

## 5. The sound: nine actions, one spent

`sense_sound` (4) and `sense_sound_model` (5) are the 2026-11-25 work
(#213, #215). What the button does with them:

- **`generate`** - yes, three times, and the licence ladder is
  genuinely careful (`docs/SOUND-GENERATION.md:112`).
- **`check_placement`** - no. The accent is placed by the manifest's
  `syncAt` (`director_program.rs:11519`), and the docs themselves say
  **61 of the 530 bundled sounds carry a `syncAt` the file
  contradicts**. #222 fixes exactly this, and does it the right way: no
  new tool, `check_placement` → `clips_move` (drift) or `clips_remove`
  + re-shop (`peak_cut` / `wrong_sound`), and an unanswered check is
  said out loud rather than assumed.
- **`describe_sound` / `rank_sounds`** - no. SFX selection is a
  hardcoded emotion→family table plus `sounds_search` keyword order plus
  "first unspent hit". The CLAP+measurement ranker and the 0.11 s
  library index exist and are unspent (#9 in the fifty-item audit).
- **`plan`** - no. The action that designs the *whole* accent set from
  beats + arc + visual events + bed structure is never asked; the
  spine's own peak loop is the substitute.
- **`describe` on the bed** - no (#10). #226 lands it: a bed that is not
  music, or not the mood asked for, comes off and the next
  duration-fitting candidate imports, at most three listens.
- **`index`** - no. A cold library scan is 148 s; the index answers in
  0.11 s.

---

## 6. The seven lanes: today's decider vs the instrument it doesn't ask

| Lane | What decides it today | Instrument that exists and is unspent |
| --- | --- | --- |
| **Cut** | voice pauses (`dead_air`), held beats, lockstep A-roll | the bed's downbeat grid (#226), `suggest_cuts`/`analyze_scenes`, `detect_vocal_events` (a cough is a cut candidate), `measure_rhythm_meter`'s beatSync |
| **Caption** | transcript lines, fillers, emphasis, saliency band | `align_captions` (drift correction), `clean_subtitles`, `evaluate_tts_quality`/`detect_ai_voice` (a TTS bed is not a human take), speaker-labelled captions |
| **Audio** | `enhance_speech`/`diagnose_take`/`repair_by_diagnosis`, per-clip levels, duck spans | `classify_recording_quality` (a gate before the read), `remove_hiss_hum_rumble`, `normalize_loudness`, the measured M-series ride (#226), `measure_loudness` on the *take* |
| **SFX** | emotion→family tables, keyword search, manifest `syncAt`, gain | `rank_sounds` (CLAP + measurement), `describe_sound` (family check), `check_placement` (#222), `plan` |
| **BGM** | query chain + duration fit, duck span | `describe` (is it music, is it the mood) (#226), tempo fit against the voice pace, `analyze_music_structure` |
| **Motion** | emotion → preset → bake, continuity gates | `motion_frame` review **before** bake (the spine bakes blind), `sense_visual tracks` (subject identity across a cut), `motion_preview` |
| **Stock** | licence gate, keyword page, one frame per candidate, 13-field read, lexical gate | CLIP pair on the fetched frames (#226), embedding near-duplicate suppression (#226), the enrich doors (#224), a register check that is not token-set intersection (#5) |

---

## 7. Is the merged work used? PR by PR

| PR | What it shipped | Button spends it? |
| --- | --- | --- |
| #209 | story, retention, variety direction | **yes** - `choose_direction`, `retention_analysis`, `visual_variety` |
| #213 | `sense_sound`: describe, rank, check_placement, index | **no** - 0 of 4 (#222 lands check_placement) |
| #215 | `sense_sound_model`: describe, index, rank, plan, generate | **1 of 5** - `generate` only |
| #218 | three text-to-audio models + licence ladder | **yes** - accents, ≤8 s bed, pad |
| #219 | Director taught the sound doors, claims held | **Director: taught**; button: no |
| #220 | the three integrated, Director taught its own doors | as above |
| #223 | the fifty-item audit (docs) | n/a |

So: **the generators are spent, the instruments are not.** That is the
single clearest sentence in this audit, and it is why #222/#224/#225/#226
exist.

---

## 8. The five open PRs: state, coverage, and merge order

Read live on 2026-11-26 (`gh pr list`), all five `MERGEABLE`:

| PR | Title | Closes (fifty-item audit) | Files |
| --- | --- | --- | --- |
| #222 | the accent is placed, measured, corrected or dropped | 15 (the `syncAt` clause), 44 (picture set headless) | `director_program.rs`, `tools.rs`, `concat-api/{message,speech}.rs`, `concat-host/visualmodel.rs`, `settings.rs` |
| #224 | the headless run fans its takes, and preflights its transcriber | **1, 2, 3, 7, 13, 39** | `auto_edit.rs`, `backend.rs`, `director_program.rs`, `visual_read.rs`, `receipt.rs`, `runner.rs`, `stage.rs`, `forge.rs`, 26 locales |
| #225 | surface handled tool degradations in receipts | 3 (the warning half) | `concat-director/src/runner.rs` (only) |
| #226 | the signals meet their consumers - CLIP look, listened bed, measured ride, downbeat cut | **4, 6, 10, 11, 12** | `director_program.rs`, `visual_read.rs`, `voice_read.rs`, `sound_model.py`, `visual_sense.py`, `voice_signals.py`, `analysis/rerank.py`, skill docs |
| #227 | the stock clip's origin outlives the reply | 15 (provenance) | `concat-api`, `concat-project`, `director_program.rs`, `receipt.rs` |

**Merge order.** #225 and #224 both rewrite `concat-director/src/runner.rs`
and both claim item 3 from opposite ends (#224 makes the *gate* read
`degradations`, #225 makes the *warning list* carry them), and #224 also
touches `receipt.rs`. Land **#225 first** (it is one file, +tests), then
rebase #224 onto it - the two are compatible in intent and overlapping in
lines. #226 and #227 both touch `receipt.rs` (asset records) but
different sections. #222 is independent.

**Check status, read job by job from the latest CI run on each branch
(not from the aggregate badge, which mixes runs):**

| PR | Latest CI run | Failing job | The actual error |
| --- | --- | --- | --- |
| #222 | 7/7 pass | - | - |
| #224 | fail | `Format, locales and models` + `Engine tests and lints` | `cargo fmt` diff in `backend.rs:309`; clippy `cloned_ref_to_slice_refs` (lib test) |
| #225 | 4 pass, `Engine tests and lints` in flight | - | the dependency lane passes (7m3s) |
| #226 | fail | `Engine tests and lints` | clippy: dead field `StockCandidate.seconds`, `collapsible_if` at `director_program.rs:11335` |
| #227 | fail | `Engine tests and lints` | **`error[E0063]`: missing field `stock` in initializer of `MediaItem`** - three test fixtures were not updated, so the branch does not compile |

Three of the five need a fix before merge; #222 is clean and #225 is
waiting on its last lane. #224 and #226 are one-line fixes (a `fmt`, a
clippy lint, a dead field). #227 is not: it adds a field to
`MediaItem` and leaves three construction sites behind, so it cannot be
merged before those are filled - the same breakage any other caller of
that initializer would take. None of the five is a design disagreement.

---

## 9. The gap that remains **after** the five PRs land

This is the part the five PRs do not close, ordered by what it costs the
output.

### P0 - the edit is called "verified" when only the voice was verified

1. **The picture and the sound are never gated.** The acceptance
   re-measures the voice, the plan, the picture *as frames* (craft) and
   the lanes. It never asks whether the B-roll was measured, whether the
   accents were placed on measured peaks (after #222 the spine does it,
   but nothing *checks* the receipt for it), or whether the bed was
   listened to. **Wiring:** the run's own log already holds every
   `sense_visual`/`sense_sound`/`sense_sound_model` reply - add a
   `doors` section to the receipt that lists, per door, the actions
   asked, the actions answered and the actions absent, and make
   "0 accents checked" a visible fact rather than an invisible one.
   No new tool.
2. **The signal-evidence gate verifies 19 of the 33 vocabulary names and
   skips the other 14 silently.** `measured_voice_family`
   (`receipt.rs:940`) maps only voice families plus `arc`;
   `signal_evidence_gate` (`:976`) does `let Some(family) = ... else {
   continue }`. Unverified today: `emotion`, `topics`,
   `discourseMarkers`, `speechActs`, `claims`, `musicSegments`,
   `wordConfidence`, `motion`, and all six picture families
   (`visualMotion`, `shotScale`, `camera`, `visualQuality`, `saliency`,
   `scene`). A plan citing `camera` gets no check at all. **Wiring:** the
   evidence for each already exists in the log - the gather's
   `measure_clips` reply for the picture families, the `emotion extract`
   reply for `emotion` (the gate already reads it for `arc`), the four
   discourse replies for `topics`/`claims`/`speechActs`. Extend the map
   and pass those replies in; skip nothing without a stated reason, the
   way `NOT_SPENT` states one.
3. **`Stage::Export` is registered and never scheduled**
   (`stage.rs:88`), so "the button exports" is a tool the user can call
   and a stage the receipt can name but the run never reaches. The
   acceptance already supports an export branch (`acceptance.rs:93`:
   `motion_probe`, `measure_loudness`, `watch_render`, the words
   verdict). **Wiring:** either schedule the export behind an explicit
   `render: true` (and the acceptance's export gates come alive for
   free), or delete the row from the register. Today's middle state is
   the one that lets "verified" mean two different things.

### P1 - the intelligence exists, is documented as spent, and is not

4. **`sense_visual` is a one-action door for the button** (1/38). The
   highest-value additions, cheapest first: `tracks` (subject identity
   across the cut - the gather already asks `footage_continuity`, which
   is a different thing), `shot_boundaries` (TransNetV2 on the montage
   veto instead of the 24-sample `_scene_profile`), the aesthetic door
   as its own term rather than folded into `visualScore`.
5. **`sense_sound`'s ranker and describers** (0/4 today; #222 adds one).
   `rank_sounds` replaces the emotion→family table's first-hit with a
   CLAP+measurement order over the same candidates; `describe_sound`
   rejects a wrong family before the add rather than after the
   placement check.
6. **`sense_sound_model plan`** - the whole accent design from beats,
   arc, visual events and bed structure, in one call, is the designed
   instrument for the loop the spine hand-rolls.
7. **Three whole doors never called:** `sense_voice_quality` (0/6),
   `sense_prosody_style` (0/6), `sense_captions` (0/5). The Director
   skill spends them - `classify_recording_quality` before the read
   (Phase 2), `align_captions` + `clean_subtitles` before the captions
   are burned. **Wiring:** call the three the skill already names, in
   the same order the skill names them - the button then does what the
   docs say the product does.
8. **The emotion gate is lexical** (`emotion.py:1290` says so in its own
   reply). #226's `rank_frames` CLIP door is the embedding the fix
   needs; the token gate can stay as the floor.
9. **The revise loop is hollow without a VLM.** Measured fixes carry
   `confidence: 0.5` and no `patch` by design; `critic_fix_commands`
   requires ≥ 0.6 **and** a patch (`:7812`, `:7878`). A VLM-less run
   therefore only ever gets the three pixel passes. **Wiring:** derive
   real patches for the measured fixes (letterbox, static runs and the
   black opening all have commands already) and lower the bar for a fix
   that carries one.
10. **The Watch gate sits at the heuristic ceiling.** With every axis
    supplied and every penalty at zero the heuristic maxima sum to
    **7.15** (`sense.py:1335-1372`: 7.8 + 6.8 + 7.4 + 7.4 + 6.9 + 6.6
    over six axes) against a **≥ 7.0** gate (`director_program.rs:11963`).
    One flaw anywhere and a VLM-less machine is "warned". **Wiring:** an
    engine-aware bar (the reply already names its `engine`), or a bar
    that reads the `revisions` count rather than only the mean.

### P2 - speed and scale (the "faster, smarter" half of the ask)

11. **The headless tool drops the event receiver** (`auto_edit.rs:95`,
    `forge.rs:1114` - both `let (runner, _events)`), so the tool has no
    progress and the pane's bar is stage-quantised. #224 gives the tool
    lanes, not events.
12. **`analysis_wait` is a blocking sleep-poll capped at 590 s**
    (`tools.rs:41`, `:4487`); a long transcription on a small laptop
    fails the gate at ten minutes.
13. **A cold FreePD shelf is polled for ten minutes inside Music** -
    ≤ 200 × 3 s (`director_program.rs:11003`).
14. **A fresh Python process per sense call**, and the visual shelf
    caches four sessions then clears all four (`visual_sense.py:1044`).
15. **The stock read measures 3 of the 6 candidates the shelf returns**
    (`stock_search limit: 6`, `LOOK_AT: 3`, `maxClips: 3`). After
    #226's CLIP rerank scores all six, the measurement can follow the
    ranking instead of the page order.
16. **The Director's evidence store has zero enforcement** (#43): no
    code writes it, reads it or gates on it. Either the spine writes it
    from its own log, or the contract is retired - today it is a prompt
    that cannot fail.

### P3 - the documentation that will drift next

17. `signal_matrix.md` is the **voice** catalogue only (`:108` says so);
    the six picture families have no reading→decision table, and
    `check-signal-matrix.py` checks prose, not code (#40, #42).
18. The `CONSUMERS` map's two false claims (§4) - `motion.onsets` and
    `composition`'s caption half.
19. The 17-stage spine vs the 25-phase Director contract (#46): one
    honest vocabulary, or a stated mapping.
20. `wordConfidence` is citable and unverifiable (§3.4).

---

## 10. The plan: five batches, existing tools only

Each batch is independently shippable, has its own evidence, and invents
nothing - every call below is a tool the catalogue already serves and
the Director skill already teaches.

### Batch 0 - land the five (this week)

Land #225 → #224 (in that order, the `runner.rs` overlap), then #227,
#226, #222. Before each merge: `cargo fmt` on #224's `backend.rs:309`
block, one clippy lint in #224's test, #226's dead `StockCandidate.seconds`
and `collapsible_if`, and #227's three `MediaItem` fixtures. **Evidence:**
the three required checks green on each exact head; this audit's §9 P0-1
and P1-4 … P1-6 lines marked closed by PR number.

### Batch 1 - the receipt tells the truth about the doors (P0-1, P0-2)

- A `doors` section in the receipt: per door, actions asked / answered /
  absent, read from the run's own log; a door that was never asked is a
  line, not a silence.
- Extend `measured_voice_family` + `signal_evidence_gate` to the 14
  unverified families, with each family's evidence named: the gather's
  `measure_clips` rows for the picture, the `emotion extract` reply for
  `emotion`, the discourse replies for `topics`/`claims`/`speechActs`.
- **Guard:** a test that walks `SIGNAL_VOCABULARY` and fails on any
  family that is neither verified nor in a `NOT_VERIFIED` list with a
  reason - the exact shape of the two Rust guards that already work.
- **Evidence:** a run whose plan cites `camera` with no picture read
  fails the gate; a run with the read passes.

### Batch 2 - the button spends the shelf (P1-4 … P1-7)

In this order, each one call site and one test:

1. `sense_sound {action: "rank_sounds"}` before the first
   `sounds_search` hit is placed (family tables become the fallback).
2. `sense_sound_model {action: "plan"}` as the accent design when beats
   and a bed exist; the hand-rolled peak loop stays as the no-bed path.
3. `sense_captions {action: "align_captions"}` + `clean_subtitles`
   immediately before `captions_add` (the skill's own order).
4. `sense_voice_quality {action: "classify_recording_quality"}` at the
   top of AudioRepair - the gate the skill runs in Phase 2.
5. `sense_visual {action: "tracks"}` on the chosen B-roll, feeding the
   continuity gate the subject identity it currently infers from scale.
6. `sense_visual {action: "shot_boundaries"}` as the montage veto's
   primary read (the 24-sample profile becomes the fallback).

- **Guard:** extend the field-forwarding guard #226 adds
  (`every_forwarded_field_is_in_its_familys_schema`) to the new actions -
  it already found a real `revision` gap - and add the door-coverage line
  to `check-signal-matrix.py` so a door the button stops calling fails CI.
- **Evidence:** the sense-action count rises from 10 to 17 (seven new
  actions at seven call sites), and each new call has a test that
  asserts the call site and the decision it changed.

### Batch 3 - the bar and the loop (P1-9, P1-10)

- An engine-aware craft bar: `heuristic` needs the revisions count to be
  zero *or* the score ≥ 7.0; a VLM/vision reply keeps the flat 7.0.
- Real patches for the measured fixes, and the confidence bar lowered
  for a fix that carries one (the letterbox, static-run and black-opening
  commands already exist).
- **Evidence:** a VLM-less run that enters the revise loop and *changes
  the timeline* (assert the edit_apply count grows), and a warned run
  whose receipt names the fix it could not apply.

### Batch 4 - the speed (P2-11 … P2-15)

- Drain the runner's events in both consumers; carry job fractions to
  the bar (#29, #30) - this is also what makes #12's 590 s cap
  unnecessary.
- Bound the FreePD poll to ~30 s, warn, move on (the pad path already
  answers).
- Session LRU instead of cap-and-clear; a measured case for one
  persistent sidecar.
- Measure the stock candidates the rerank scored, not the first three.

- **Evidence:** a 5-take headless run's wall clock before/after (lanes +
  events), and a cold-shelf run that no longer waits ten minutes.

### Batch 5 - the Director is held to the same bar (P2-16, P3)

- Either the spine writes `director/` from its own log and the Director
  path is gated on it, or the contract is retired. A prompt that cannot
  fail is not a contract.
- `DIRECTOR_REQUIRED` grows the doors a voiceover-led run must be able
  to call (`sense_visual`, `sense_sound`, `sense_captions`,
  `sense_voice_quality`) - `unserved()` already refuses a run that
  cannot reach a required tool, so this is a list, not a mechanism.
- The picture's six families join `signal_matrix.md`, generated from
  `CONSUMERS` so the two cannot disagree.
- **Evidence:** a Director run that skips every door is *warned* (the
  receipt's `doors` section names the silence), and the matrix check
  fails if a family leaves the map.

---

## 11. The guards to add, so this cannot regress

The repository's best idea is that a claim about a signal is a test, not
a sentence. Three guards exist for the voice; the picture, the sound and
the button have none. Add, in this order:

1. `every_vocabulary_family_is_verified_or_named` - the Batch 1 guard.
2. `every_sense_door_the_skill_teaches_is_called_by_the_spine` - reads
   the skill's `sense_* {action: ...}` mentions and the spine's call
   sites, and fails on a door taught and never asked (with a
   `NOT_ASKED` list and reasons, the same shape as `NOT_SPENT`).
3. `every_consumers_claim_names_a_call_site` - for `visual_sense.CONSUMERS`
   and `sound_sense`, a claim must name a Rust symbol that exists (the
   two false claims in §4 are the ones it would have caught).
4. `the_spine_calls_what_the_docs_say_it_calls` - a generated table in
   `docs/FEATURES.md` (door × action × called-by) built from the same
   scan, so the inventory cannot drift from the code.

---

## 12. Coverage 100: what the number can honestly be

The ask is "the auto editor uses 100% of everything". Here is the
arithmetic that makes that a specification instead of a slogan, and the
four mechanisms that hold it.

### 12.1 The denominator

The eight doors dispatch **83 actions** (the count in §2 said 82 because
`sound_model`'s `selftest` was excluded). Split by what they are:

| Kind | Count | Examples |
| --- | ---: | --- |
| **Door readings** - measure the material | **75** | `measure_motion`, `rank_sounds`, `align_captions`, `analyze_discourse` |
| **Door machinery** - install, index, bench, self-test | **8** | `models`, `download`, `hf_search`, `hf_files`, `bench`, `index_library`, `index`, `selftest` |
| **Voice profile readings** - `voice_signals`' own | **70** | `prosody.pitchRangeSemitones`, `speaker.turns`, `motion.levelPulse` |

So "100%" means **145 of 145 readings** - the 75 door readings plus the
profile's 70 - not 83 actions. The button spends **10** of them today
(7%): `measure_clips`, `generate`, `enhance_speech`, `diagnose_take`,
`repair_by_diagnosis`, `detect_music_segments`, `analyze_discourse`,
`segment_topics`, `detect_speech_acts`, `extract_claim_graph`.

`scripts/check-signal-coverage.py` measures exactly this and prints the
ledger; run it to see the number move.

### 12.2 The contract

> **Every one of the 145 readings is either asked in the run, or named in
> the receipt's coverage ledger with the reason it did not apply to this
> material.**

That is the whole specification. It is achievable; "call everything
always" is not, and would be dishonest - `analyze_music_structure` on an
8-second generated pad, `detect_code_switching` on a monolingual take
and `lip_sync` on a clip with no face are wasted minutes, not extra
intelligence.

The ledger entry is one of five states, and only one of them is silent:

| State | Meaning |
| --- | --- |
| `asked` | the call is in the program and it answered |
| `absent` | the call is in the program and the door answered "not installed" |
| `not-applicable` | **with the measured reason** - "no face in any sampled frame", "no music lane", "single language detected" |
| `skipped-budget` | **with the tier name** that skipped it and the seconds it would have cost |
| `missing` | a reading that applied, could have run, and was not asked - **this is the only failure** |

### 12.3 The four mechanisms

**1. A capability pass.** Preflight asks `sense_visual {action:"models"}`
and `models_list`, so the program knows what is installed before it
builds. This is what makes an install matter at all (§ above: the spine
never asks today).

**2. A capability-driven gather.** A `DerivedStep::react` over the
capability reply emits the calls for the doors that are `ready`. The
machinery exists - the revise loop already reads a reply and emits
calls. This is how 30 doors get spent without 30 hardcoded call sites,
and how the same program degrades by itself on a bare machine.

**3. A decision table per reading.** Every reading names the decision it
informs. The vocabulary needs five more names than the gate accepts
today, because the picture and the sound decide things the voice never
did:

| Decision | Readings that drive it |
| --- | --- |
| `cut` | pause spans, `measure_rhythm_meter`, bed downbeats, `shot_boundaries`, `visual_sync`, `scene_cuts` |
| `assemble` | `classify_recording_quality`, `classify_voice_quality`, `detect_restarts`, `classify_filler_type`, `evaluate_tts_quality`, `detect_ai_voice` (which take is usable) |
| `caption` | `align_captions`, `clean_subtitles`, `saliency`, `depth_map`, `face_emotion` (register), `speakers` |
| `select` | `video_embedding`/`visual_embedding`, `aesthetic`, `action_class`, `detect_objects`, `scene_graph`, `semantic_segments`, `describe_scene`, `film_shot_type`, `subject_mask`, `pose`/`hands`/`gaze`, `action_quality` |
| `mix` | `measure_loudness`, `normalize_loudness`, `remove_hiss_hum_rumble`, `detect_music_segments`, `identify_music`, `analyze_music_structure`, `describe_sound`, `rank_sounds`, `check_placement`, `plan` |
| `grade` | `palette`, `composition`, `blur_mask`, `video_saliency`, `motion_blur`, `visual_quality`, `aesthetic` |
| `motion` | `measure_motion`, `camera_motion`, `optical_flow`, `tracks`, `gaze`, `face_emotion` |
| `pacing` | `prosody`, `tempo_changes`, `accent`, `vocal_tract`, `pauses`, `arc` |
| `transition` | `motion`, `camera`, `shot_scale`, `scene_cuts`, `blur_mask` |
| `critique` | `score_readability`, `score_script_quality`, `detect_sarcasm`, `detect_cross_lingual_emotion`, `classify_speech_acts`, `describe_scene`, `action_quality` |

`assemble`, `select`, `mix`, `grade` and `critique` are the five names
the guard does not accept yet. Adding them is a **contract extension,
not a widened guard**: each must map to a real gate
(`assemble` → the take choice, `select` → the stock/B-roll veto,
`mix` → the loudness/duck gates, `grade` → the grade step,
`critique` → the craft bar).

**4. A coverage guard.** `every_usable_action_is_asked_or_named`: the 145
readings minus the ones the ledger names with a reason must be zero.
Run it on the program's own log, so a reading that goes silent fails CI
the way a missing reader does in the voice guard.

### 12.4 Depth tiers, so 100% stays fast

100% of the readings on 40 clips is not free: the 30 ONNX doors cost
roughly 0.2-2 s per clip each, so a thorough pass over 40 clips is
minutes, and over 300 clips is an hour. The honest resolution is a tier,
chosen from the material and the install, **named in the ledger**:

| Tier | What it asks | Rough cost, 40 clips |
| --- | --- | --- |
| `fast` | classical reads only, `enrich: false` | ~40 s |
| `standard` | classical + the 3 default enrich doors + sound/captions | ~2 min |
| `thorough` | every installed door on every candidate that survives the veto | 5-15 min |

`thorough` is the default on a provisioned box for material under
~60 clips; `standard` above that; `fast` only when asked. The ledger
records which tier ran and what it skipped, so "100%" is always
qualified by the tier - never claimed falsely.

### 12.5 The matrix that now holds all 145

`prompts/signal_matrix.md` in the Director skill carried the 70 voice
readings and a sound section; it now carries the picture's 33 and the
other 42 as well -
the twelve classical reads with the leaves the runtime parses, the
twenty-one door reads with their checkpoints and alternates, the
arbitration rules for the ten questions two readings answer, and the
coverage rule ("cited, or named with the reason"). That file is the
Director's own prompt, so the AI path reads the same 145 the button
does - which is the point: one matrix, two callers.

### 12.6 What 100% still does not mean

- **No reading is a decision.** 75 readings feed 10 decisions; the
  arbitration (which reading wins when two disagree) is the value, and
  it has to be written down per lane - the classical/model agreement
  confidence `visual_sense.py:976` is the existing pattern.
- **Two doors that disagree are information, not a problem.** Where a
  classical read and a model read differ, the ledger records both and
  the lane uses the conservative one; that is how `visual_quality` and
  `dover-mobile` should meet.
- **The Director is a second caller of the same 75.** Coverage is not
  reached by the button alone: `DIRECTOR_REQUIRED` must name the doors a
  voiceover-led run has to reach, or the AI path can still skip
  everything and call it "Accepted".
- **A generated asset is not a reading.** `generate` and `seal_voice`
  produce material; they are counted as spent only when the artefact
  lands in the project, not when the call returns.

---

## Appendix: how to re-measure

```sh
# the catalogue and the served surface
python3 - <<'EOF'
import re
s = open('src/crates/concat-mcp/src/tools.rs').read()
i = s.index('pub fn catalogue()'); j = s.index('\npub fn ', i + 10); b = s[i:j]
rows = {k: len(re.findall(r'^\s{8}%s\(' % k, b, re.M)) for k in ('method', 'command')}
rows['Tool'] = len(re.findall(r'^\s{8}Tool \{', b, re.M))
print(rows, 'total', sum(rows.values()))          # {'method': 47, 'command': 22, 'Tool': 48} 117
s = open('src/crates/concat-mcp/src/protocol.rs').read()
for k in ['DIRECTOR_ALLOWLIST', 'DIRECTOR_REQUIRED']:
    i = s.index(k + ': &[&str] = &[') + len(k) + 16
    d, j = 1, i
    while d:
        d += {'[': 1, ']': -1}.get(s[j], 0); j += 1
    print(k, len(re.findall(r'"[a-z0-9_]+"', s[i:j])))   # 109 / 25
EOF

# the spine's own vocabulary
python3 - <<'EOF'
import re, collections
L=open('src/crates/concat-mcp/src/director_program.rs').read().split('\n')
b='\n'.join(L[8096:11965])                     # auto_edit_program
calls=collections.Counter(re.findall(r'call\(\s*"([a-z0-9_]+)"', b))
print(len(calls), 'distinct tools,', sum(calls.values()), 'call sites')
EOF

# the eight doors' action counts (selftest is machinery, not a reading)
for f in visual_sense sound_sense sound_model voice_quality prosody_style captions audio_repair discourse; do
  printf '%-16s ' "$f"
  python3 -c "
import re,sys
s=open('src/crates/concat-mcp/sense/$f.py').read()
m=re.search(r'^TOOLS = \{(.*?)^\}', s, re.S|re.M)
keys=re.findall(r'\"([a-z0-9_]+)\":', m.group(1)) if m else []
print(len([k for k in keys if k != 'selftest']))"
done

# the 70 voice readings, by family
python3 -c "
import re,collections
s=open('src/crates/concat-mcp/sense/voice_signals.py').read()
i=s.index('SIGNAL_CATALOGUE = ['); j=s.index('\n]\n', i)
print(collections.Counter(re.findall(r'\"family\":\s*\"([A-Za-z]+)\"', s[i:j])))"

# what the evidence gate can verify
grep -n 'fn measured_voice_family' -A30 src/crates/concat-director/src/receipt.rs

# the craft ceiling (heuristic, every penalty zero, loudness supplied)
python3 -c "print((7.8+6.8+7.4+7.4+6.9+6.6)/6)"   # 7.15 against a 7.0 gate
```
