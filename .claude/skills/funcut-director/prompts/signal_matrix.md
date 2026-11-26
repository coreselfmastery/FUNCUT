# The signal matrix — spend every reading

The runtime `voice_signals` catalogue is the source of truth: it currently
contains **71 named readings across 14 families**. The eight sense doors dispatch 83 actions,
75 of which read the material (8 install, index, bench or self-test):
**35** on the voice side - repair, captions, prosody style, voice
quality, discourse - **7** sound and **33** picture (12 that need
nothing installed, 21 behind a door). **145 readings in all**, spent the
same way: a reading is spent only when it changes something on the
timeline. This matrix uses the
catalogue's exact identifiers, not aliases or guessed field paths. Reading
the profile is not spending it. The plan's `signalLog` must name the exact
catalogue reading in its evidence note and use the accepted `signalLog`
family name shown in the third column, the
decision it changed, its confidence and its evidence. A reading that was
not measured is named unmeasured; never cite a value that was not returned.

| # | Exact catalogue reading | `signalLog` name accepted by `plan_validate` | Decision it informs |
|---:|---|---|---|
| 1 | `loudness.integratedLufs` | `loudness` | final mix gain against the platform loudness target |
| 2 | `loudness.loudnessRangeLu` | `loudness` | compression amount and preservation of dynamic contrast |
| 3 | `loudness.truePeakDbfs` | `loudness` | limiter ceiling and clipping prevention |
| 4 | `loudness.meanLufsM` | `loudness` | relative line emphasis, caption weight and accent balance |
| 5 | `loudness.relativeLu` | `loudness` | which lines deserve stronger visual or sound emphasis |
| 6 | `loudness.momentaryLufs` | `loudness` | the bed's volume ride: the duck follows the voice's measured envelope, not a span average |
| 7 | `pauses.classes` | `pause` | cut only dead air; preserve breaths, beats and dramatic holds |
| 8 | `pauses.speechRatio` | `pause` | BGM level, silence design and visual density |
| 9 | `speechRate.wordsPerMinute` | `speechRate` | shot duration, caption group size and pacing |
| 10 | `speechRate.syllablesPerSec` | `speechRate` | caption reveal duration and reading-time protection |
| 11 | `words.confidence` | `words` | caption inclusion and word-timing acceptance |
| 12 | `language.script` | `language` | caption script, font, direction and language-specific layout |
| 13 | `sentence.type` | `sentence` | caption grouping, question holds and transition timing |
| 14 | `entities.cards` | `entities` | accurate number, money, date, place and name graphics |
| 15 | `motifs.repeats` | `motifs` | recurring visual callbacks when a phrase returns |
| 16 | `fillers.list` | `fillers` | safe filler cleanup without deleting meaningful speech |
| 17 | `arc.emotion` | `arc` | emotional progression, music, pacing and grade by act |
| 18 | `prosody.emphasisWords` | `prosody` | exact caption emphasis, graphic beats and SFX accents |
| 19 | `prosody.pitchRangeSemitones` | `prosody` | intensity of motion, typography and performance emphasis |
| 20 | `prosody.pitchMeanHz` | `prosody` | voice character and conservative voice/tonal treatment |
| 21 | `prosody.finalRise` | `prosody` | question detection, punctuation and reveal hold |
| 22 | `prosody.voicedRatio` | `prosody` | distinguish speech from pauses and protect spoken words |
| 23 | `prosody.rhythmLayers` | `prosody` | rhythm grid, section pacing and optional beat-aligned cuts |
| 24 | `prosody.phrases` | `prosody` | phrase boundaries, caption chunks and visual phrase changes |
| 25 | `prosody.pitchContourHz` | `prosody` | rising/falling delivery and motion/easing direction |
| 26 | `forensic.noiseFloorDbfs` | `forensic` | noise repair, room tone and BGM headroom |
| 27 | `forensic.snrDb` | `forensic` | voice usability, repair strength and music ducking |
| 28 | `forensic.noiseType` | `forensic` | choose the right noise-removal treatment |
| 29 | `forensic.spectralFlatness` | `forensic` | distinguish broadband noise from tonal material |
| 30 | `forensic.formantsHz` | `formants` | voice identity and consistency checks; never infer identity beyond the audio task |
| 31 | `forensic.hnrDb` | `forensic` | voice clarity and whether repair is needed |
| 32 | `forensic.jitter` | `forensic` | vocal roughness diagnosis and conservative repair |
| 33 | `forensic.shimmer` | `forensic` | amplitude instability diagnosis and conservative repair |
| 34 | `forensic.fluxMean` | `forensic` | energy changes and where a visual beat may earn emphasis |
| 35 | `forensic.fluxOnsetsSec` | `forensic` | measured transient positions for cuts and SFX accents |
| 36 | `forensic.rmsEnvelope` | `forensic` | dynamic shape, loudness rides and duck automation |
| 37 | `forensic.samplePeakDbfs` | `forensic` | clipping prevention and gain ceiling |
| 38 | `forensic.clippingRatio` | `forensic` | whether the take needs clipping repair or rejection |
| 39 | `forensic.dcOffset` | `forensic` | DC correction before mixing |
| 40 | `forensic.humHz` | `forensic` | identify hum frequency for targeted removal |
| 41 | `forensic.sibilanceRatio` | `forensic` | de-essing and high-frequency voice treatment |
| 42 | `forensic.spectralRolloffHz` | `forensic` | tonal balance and EQ/grade direction |
| 43 | `forensic.spectralBandwidthHz` | `forensic` | voice clarity, bandwidth and EQ decisions |
| 44 | `forensic.onsetRatePerMin` | `forensic` | cut density and sound-accent restraint |
| 45 | `forensic.reverbTailSec` | `forensic` | room-tail handling, silence design and transition tails |
| 46 | `forensic.musicLikelihood` | `forensic` | detect existing music and prevent a conflicting second bed |
| 47 | `speaker.turns` | `speaker` | speaker-change cuts, labels and lower-third timing |
| 48 | `speaker.shares` | `speaker` | speaker attribution confidence and conservative labeling |
| 49 | `speaker.utterances` | `speaker` | speaker-specific shot and caption segmentation |
| 50 | `speaker.meanUtteranceSec` | `speaker` | speaker pacing and conversational shot duration |
| 51 | `speaker.longestSilenceSec` | `speaker` | long-gap review: preserve intentional holds, investigate dead air |
| 52 | `motion.emphasisHits` | `motion` | kinetic word hits: the exact stressed word and its scale pop |
| 53 | `motion.kineticScale` | `motion` | how far a card or caption grows across its line |
| 54 | `motion.pitchDirection` | `motion` | up-screen entry vs a settling move, from the measured slope |
| 55 | `motion.pitchTravel` | `motion` | keyframe distance in bounded pixels |
| 56 | `motion.rateTempo` | `motion` | animation speed and card hold from words per minute |
| 57 | `motion.pauseDwell` | `motion` | holds, next-hero entrances and dwell frame counts |
| 58 | `motion.onsetSync` | `motion` | hit and flash timing on spectral-flux onsets |
| 59 | `motion.rhythmStagger` | `motion` | multi-layer entrance stagger from the rhythm grid |
| 60 | `motion.sentenceEntry` | `motion` | question, exclamation and fragment entry presets |
| 61 | `motion.emotionEase` | `motion` | spring, emphasis, ease-out or linear easing |
| 62 | `motion.arcTurn` | `motion` | background and hero swap at an arc turn |
| 63 | `motion.entityCard` | `motion` | hero-card timing for a named entity |
| 64 | `motion.topicChapter` | `motion` | chapter-card entry at a topic change |
| 65 | `motion.motifReturn` | `motion` | callback visual when a repeated phrase returns |
| 66 | `motion.speakerTurn` | `motion` | lower-third flip at a speaker change |
| 67 | `motion.confidenceGate` | `motion` | no glitch or pop on a low-confidence word |
| 68 | `motion.sibilanceTexture` | `motion` | shimmer intensity from measured sibilance |
| 69 | `motion.reverbSpace` | `motion` | background blur and parallax depth from the room tail |
| 70 | `motion.noiseGrain` | `motion` | grain texture matched to floor and flatness |
| 71 | `motion.levelPulse` | `motion` | scale-pulse strength from line lift |

## Consumption rules

- The catalogue is the authority. If an identifier is absent from the
  catalogue, do not invent it; if a catalogue entry changes, update this
  matrix and its test together.
- A reading informs a decision only when its returned value is actually
  used. Merely listing a reading in `signalLog` is not consumption.
- Record the exact reading identifier, timestamp, decision, confidence,
  evidence note and resulting action. If the tool did not return it, mark
  it unmeasured and use a conservative fallback.
- Do not force every reading into every edit. Use all applicable measured
  readings; explain why an irrelevant reading did not affect this story.
- A low-confidence reading must not overrule a stronger measurement or
  the user's explicit creative brief.
- Voice-analysis measurements do not fact-check financial claims. Numbers,
  dates, comparisons and claims still need source evidence before they are
  presented as facts.

The decision recipes that turn measurements into a cut seam, shot density
and cut type are in `prompts/signal_recipes.md`.

## Beyond the voice: the sound doors' readings

This matrix is the VOICE catalogue. The sound shelf has its own measured
readings behind two more family doors, spent the same way — every
placement cites the reading that earned it:

- `sense_sound` measures what a sound file IS: the six role fits
  (`quick_transition`, `cinematic_reveal`, `action_impact`,
  `gentle_entrance`, `emotional_texture`, `tension_build`), the measured
  `peakAt` (where the file's peak actually falls — the manifest's
  `syncAt` claim is wrong by >100 ms in 30% of the shelf), attack, body,
  decay and spectral shape. Consumers: SFX selection, placement sync,
  placement proof (`check_placement`).
- `sense_sound_model` adds what it MEANS: YAMNet labels, CLAP similarity
  to the description the read gave, and the `plan` action that turns the
  edit's own beats, arc, visual events and bed structure into a
  per-moment accent design. Consumers: SFX selection, BGM candidate
  ranking, the accent plan, the music change points.

Their schemas and exact arguments are in `prompts/tool_reference.md`;
their doctrine is `prompts/sfx_design.md` (SFX) and
`prompts/music_and_duck.md` (BGM).

## Beyond the voice and the sound: the picture's readings

`sense_visual` is 38 actions: **33 read**, 5 install (`models`,
`download`, `hf_search`, `hf_files`, `bench`). Twelve of the 33 need
nothing installed; twenty-one are doors, backed by 30 checkpoints.

A picture reading is spent only when it changes one of seven things: the
**stock/B-roll pick**, the **cut seam**, the **caption band**, the
**crop/reframe**, the **motion preset**, the **grade**, or the
**critique**. A reading that is measured and changes nothing is
"recorded, not spent" and must be labelled so.

### The twelve that need nothing installed

`measure_clips` runs all twelve in one call and returns them per
candidate; the leaves in brackets are the ones the runtime parses
(`visual_read.rs`).

| Reading | What it answers | The decision it earns |
|---|---|---|
| `motion` [`peakAt`, `mean`, `still`] | where the clip moves most, and whether it moves at all | the **in-point** (enter 0.25 s before `peakAt`), the still-frame veto |
| `camera` [`kind`, `travel`, `cameraMoving`, `subjectMoving`] | is the camera panning/tilting, which way, and is the subject moving | **continuity** (alternate direction), the drift veto |
| `shot_scale` [`scale`] | wide / medium / close | **cut rhythm** (never three wides), duplicate-scale flag |
| `visual_quality` [`verdict`] | soft, noisy, letterboxed, flat, dark, blown | the **veto**: a failed frame is not a candidate |
| `saliency` [`focus`] | where the eye lands in the frame | the **caption band** side, the crop centre |
| `scene` [`cuts`] | cuts inside the clip itself | the **montage veto** (a clip that cuts inside itself cannot be an entrance) |
| `palette` | colour temperature, saturation, contrast | the **grade** match between shots |
| `composition` | rule-of-thirds placement, headroom | the crop and the band (inside the ranking today) |
| `motion_blur` | how much smear the motion carries | the veto: a smear is not an entrance |
| `tracks` | subject path across frames | **subject continuity** across a cut |
| `visual_sync` | picture change against the beat grid | **cut on the change**, not on the clock |
| `measure_clips` | all of the above, per clip, plus `visualScore` | the candidate **ranking** |

### The twenty-one that need a door

Each is a tool action; the door column names the checkpoint that backs
it, and the alternates are listed so a 4 GB machine and a 24 GB one run
the same matrix.

| Reading | Door(s) | What it answers | The decision it earns |
|---|---|---|---|
| `action_class` | movinet-a0 | the verb (600 Kinetics classes) | **select**: B-roll that matches the *verb*, not just the noun |
| `shot_boundaries` | transnetv2 | where a cut can land | the **cut grid**: a pause is where a cut must land, a boundary is where it may |
| `film_shot_type` | film-shot-classifier | wide/medium/close/two-shot/insert | select + cut rhythm, at film-label precision |
| `video_embedding` | vjepa2-vitl | temporal similarity | select, near-duplicate suppression |
| `visual_embedding` | dinov2-small · clip-vit-b32 · siglip2-base | frame similarity / text-image match | select by the spoken words |
| `scene_graph` | scenegraph-vlm | subject-predicate-object per scene | select + entity cards |
| `depth_map` | depth-anything-v2-small · midas-v21-small | the subject's depth plane | band placement, reframe crop |
| `subject_mask` | fastsam-s | pixel mask of the subject | reframe crop, band clearance |
| `semantic_segments` | segformer-b0 | what each region is (sky, road, face) | grade (sky), band, crop |
| `detect_objects` | bytetrack-yolo · grounding-dino-tiny | persistent objects; open-vocabulary by text | select + continuity |
| `optical_flow` | raft-small | the direction of motion | **transition direction**, motion preset |
| `aesthetic` | dover-mobile · laion-aesthetic | taste and technical quality | the ranking term - never the veto alone |
| `video_saliency` | vinet-s | attention over time | band side, in-point |
| `blur_mask` | smblurdetect | motion-blur probability | the veto |
| `pose` | movenet-pose | 17 keypoints | headroom crop, gesture-timed cuts |
| `hands` | mediapipe-hands | 21 landmarks + gesture | select (a gesture answers a beat), motion |
| `gaze` | eyegaze · mobilegaze | where the subject looks | band side, cut on a look |
| `face_emotion` | microexpnet | the footage's own register | select: B-roll that matches the arc's emotion |
| `lip_sync` | syncnet | lip-sync score | assemble (talking head) and export verify |
| `action_quality` | aqa-transformer | how well the action was performed | select: the better take of the same shot |
| `describe_scene` | smolvlm2-500m · florence-2-base · uform-gen2 · smolvlm2-256m | the frame in words | select by meaning, and the critic's evidence |

### Two reads, one question — never two answers

The matrix is not a call list. Where two readings answer the same
question, the rule below decides, and the loser is recorded as
corroboration rather than cited as a second reason:

| One question | Two readings | Rule |
|---|---|---|
| Which side for the caption band | `saliency.focus` · `video_saliency` · `depth_map` · `subject_mask` | focus picks the side; depth confirms the plane; the mask clears the subject. video_saliency only when no face is found |
| Is the picture good enough | `visual_quality.verdict` · `aesthetic` | the verdict is the **veto** (pass/fail); aesthetic is the **ranking**. Never veto on a taste score |
| What framing is this | `shot_scale` · `film_shot_type` | agree → use it; disagree → the model wins when the classical read says `unknown`, otherwise the classical read wins and the disagreement is logged |
| Where may a cut land | `scene.cuts` · `shot_boundaries` | `scene.cuts` is *inside one clip* (montage veto); `shot_boundaries` is *the timeline's* grid. Different questions, both spent |
| Who is on screen | `tracks` · `detect_objects` | both track; the model read when installed, the classical one otherwise; the continuity gate takes the stricter of the two |
| Which embedding | `visual_embedding` (dinov2 / clip / siglip2) | one per run: siglip2 if installed, else clip, else dinov2. Never compare vectors across doors |
| Which depth | `depth_map` (depth-anything / midas) | one per run: depth-anything, midas as the fallback |
| Which caption model | `describe_scene` (4 doors) | one per run, chosen by the machine's memory: 256m for 4 GB, 500m above it |
| How much motion | `motion` (from `measure_clips`) · `measure_motion` · `optical_flow` | `measure_clips` for the in-point and the veto; `optical_flow` for direction; `measure_motion` only for a targeted re-measure of one clip |
| What is the emotion | `face_emotion` (picture) · `emotion` (voice) | different subjects: the voice's arc drives pacing and music; the face's register drives which B-roll matches. A clash is a mismatch flag, not a correction |

### The coverage rule

Every reading in this matrix is either **cited in `signalLog`** with its
evidence, or **named with the reason it did not apply** to this
material - "no face in any sampled frame", "no music lane", "single
language", "door not installed". Silence is the only failure. A reading
that applied, could have run, and was not asked is a defect, and the
run's coverage ledger is where it shows.

### Cite a picture reading by the family the gate accepts

`plan_validate` takes six picture families. A reading in this section
rolls up into one of them, and the evidence note names the reading
itself:

| Accepted family | Readings that roll up into it |
|---|---|
| `visualMotion` | `motion`, `measure_motion`, `optical_flow`, `motion_blur`, `blur_mask` |
| `shotScale` | `shot_scale`, `film_shot_type` |
| `camera` | `camera_motion`, `camera` |
| `visualQuality` | `visual_quality`, `aesthetic` |
| `saliency` | `saliency`, `video_saliency`, `depth_map`, `subject_mask`, `gaze` |
| `scene` | `scene_cuts`, `scene`, `shot_boundaries`, `scene_graph`, `semantic_segments` |

## The rest of the shelf: the forty-two the voice table does not name

The voice catalogue's 70 rows are the profile's own readings. The eight
doors add 75 more; 33 are the picture's (above) and 42 are these. Same
rule: cite the reading, name the decision, or say why it did not apply.

### Voice quality - is this take usable at all

| Reading | What it answers | The decision it earns |
|---|---|---|
| `classify_recording_quality` | the room and the chain (phone, laptop, treated) | which repair chain runs first |
| `classify_voice_quality` | clarity, level, usability | `assemble`: keep, re-take or reject |
| `measure_reverb` | the room's tail | silence design, transition tails, background blur |
| `detect_restarts` | false starts and repeated phrases | `cut`: which attempt survives |
| `classify_filler_type` | "um" versus a meaningful "so" | `caption`: safe cleanup without deleting speech |
| `detect_vocal_events` | laughs, sighs, coughs, breaths | keep or cut, and an accent where a laugh lands |

### Prosody style - how the words are delivered

| Reading | What it answers | The decision it earns |
|---|---|---|
| `score_emphasis_type` | which kind of emphasis a stressed word carries | `caption` emphasis style, `graphic` beat |
| `detect_speaking_style` | the delivery's register | typography and motion preset |
| `measure_rhythm_meter` | the speech's grid and meter | `cut` and `pacing` against the measured grid |
| `detect_tempo_changes` | where the delivery speeds up or slows | section boundaries, chapter cards |
| `detect_accent` | the accented syllable's position | accent and word-hit timing |
| `measure_vocal_tract` | timbre | conservative EQ, never a voice change |

### Audio repair - the take before it is measured

| Reading | What it answers | The decision it earns |
|---|---|---|
| `enhance_speech` | the repaired take | `assemble`: the audio everything downstream is measured on |
| `remove_hiss_hum_rumble` | hum, hiss, rumble, DC | `mix`: remove before measuring, never after |
| `diagnose_take` | what is wrong, with numbers | which repair runs |
| `repair_by_diagnosis` | the repair applied to the diagnosis | `mix` |
| `normalize_loudness` | the platform target | `mix`: the final gain |
| `detect_music_segments` | music already inside the take | `bgm`: never add a second bed over it |
| `identify_music` | what the existing music is | `bgm`: match or remove |
| `analyze_music_structure` | the bed's own sections and changes | `mix`: where the bed may change or drop |

### Captions - the words on screen

| Reading | What it answers | The decision it earns |
|---|---|---|
| `align_captions` | the drift between word times and audio | `caption`: correct the times before grouping |
| `clean_subtitles` | punctuation, casing, filler, line budgets | `caption`: the text itself |
| `evaluate_tts_quality` | whether a synthetic read is usable | `assemble`: a TTS take is a candidate, not a default |
| `detect_ai_voice` | whether the voice is synthetic | `assemble` and the honesty note on the deliverable |
| `seal_voice` | the voice's provenance | the receipt: which voice spoke |

### Discourse - what the script is doing

| Reading | What it answers | The decision it earns |
|---|---|---|
| `analyze_discourse` | the argument's shape | `pacing`, `transition`, the arc |
| `segment_topics` | where the subject changes | chapter cards, `cut` at a topic turn |
| `detect_speech_acts` | asserting, asking, promising | `caption` punctuation, `transition` on a question |
| `classify_speech_acts` | the act's treatment | shot choice and hold length |
| `score_readability` | how hard the script is to follow | caption density, shot duration |
| `extract_claim_graph` | claims and what supports them | `graphic`: which claim gets a card |
| `score_script_quality` | the script's own strength | `critique`: what the edit cannot fix |
| `detect_sarcasm` | irony | `caption` and `critique`: never read a sarcastic line straight |
| `detect_code_switching` | language switches | caption script and font per span |
| `detect_cross_lingual_emotion` | emotion across a language switch | `arc` continuity across languages |

### Sound - the file and the bed

| Reading | What it answers | The decision it earns |
|---|---|---|
| `describe_sound` | what a sound file is, in six role fits | `mix`: SFX selection |
| `rank_sounds` | which candidate fits the moment | `mix`: the pick, against CLAP and measurement |
| `check_placement` | where the file's peak actually lands | `mix`: correct `syncAt` when the manifest lies |
| `describe` | what a bed is and what mood it carries | `bgm`: accept or reject the bed |
| `rank` | which bed candidate fits the brief | `bgm`: the pick |
| `plan` | the per-moment accent design | `mix`: the whole SFX map |
| `generate` | a bed or accent that does not exist yet | `bgm` / `mix`: only when nothing on the shelf fits |
