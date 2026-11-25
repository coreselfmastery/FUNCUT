# The signal matrix — spend every reading

The runtime `voice_signals` catalogue is the source of truth: it currently
contains **70 named readings across 14 families**. This matrix uses the
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
| 6 | `pauses.classes` | `pause` | cut only dead air; preserve breaths, beats and dramatic holds |
| 7 | `pauses.speechRatio` | `pause` | BGM level, silence design and visual density |
| 8 | `speechRate.wordsPerMinute` | `speechRate` | shot duration, caption group size and pacing |
| 9 | `speechRate.syllablesPerSec` | `speechRate` | caption reveal duration and reading-time protection |
| 10 | `words.confidence` | `words` | caption inclusion and word-timing acceptance |
| 11 | `language.script` | `language` | caption script, font, direction and language-specific layout |
| 12 | `sentence.type` | `sentence` | caption grouping, question holds and transition timing |
| 13 | `entities.cards` | `entities` | accurate number, money, date, place and name graphics |
| 14 | `motifs.repeats` | `motifs` | recurring visual callbacks when a phrase returns |
| 15 | `fillers.list` | `fillers` | safe filler cleanup without deleting meaningful speech |
| 16 | `arc.emotion` | `arc` | emotional progression, music, pacing and grade by act |
| 17 | `prosody.emphasisWords` | `prosody` | exact caption emphasis, graphic beats and SFX accents |
| 18 | `prosody.pitchRangeSemitones` | `prosody` | intensity of motion, typography and performance emphasis |
| 19 | `prosody.pitchMeanHz` | `prosody` | voice character and conservative voice/tonal treatment |
| 20 | `prosody.finalRise` | `prosody` | question detection, punctuation and reveal hold |
| 21 | `prosody.voicedRatio` | `prosody` | distinguish speech from pauses and protect spoken words |
| 22 | `prosody.rhythmLayers` | `prosody` | rhythm grid, section pacing and optional beat-aligned cuts |
| 23 | `prosody.phrases` | `prosody` | phrase boundaries, caption chunks and visual phrase changes |
| 24 | `prosody.pitchContourHz` | `prosody` | rising/falling delivery and motion/easing direction |
| 25 | `forensic.noiseFloorDbfs` | `forensic` | noise repair, room tone and BGM headroom |
| 26 | `forensic.snrDb` | `forensic` | voice usability, repair strength and music ducking |
| 27 | `forensic.noiseType` | `forensic` | choose the right noise-removal treatment |
| 28 | `forensic.spectralFlatness` | `forensic` | distinguish broadband noise from tonal material |
| 29 | `forensic.formantsHz` | `formants` | voice identity and consistency checks; never infer identity beyond the audio task |
| 30 | `forensic.hnrDb` | `forensic` | voice clarity and whether repair is needed |
| 31 | `forensic.jitter` | `forensic` | vocal roughness diagnosis and conservative repair |
| 32 | `forensic.shimmer` | `forensic` | amplitude instability diagnosis and conservative repair |
| 33 | `forensic.fluxMean` | `forensic` | energy changes and where a visual beat may earn emphasis |
| 34 | `forensic.fluxOnsetsSec` | `forensic` | measured transient positions for cuts and SFX accents |
| 35 | `forensic.rmsEnvelope` | `forensic` | dynamic shape, loudness rides and duck automation |
| 36 | `forensic.samplePeakDbfs` | `forensic` | clipping prevention and gain ceiling |
| 37 | `forensic.clippingRatio` | `forensic` | whether the take needs clipping repair or rejection |
| 38 | `forensic.dcOffset` | `forensic` | DC correction before mixing |
| 39 | `forensic.humHz` | `forensic` | identify hum frequency for targeted removal |
| 40 | `forensic.sibilanceRatio` | `forensic` | de-essing and high-frequency voice treatment |
| 41 | `forensic.spectralRolloffHz` | `forensic` | tonal balance and EQ/grade direction |
| 42 | `forensic.spectralBandwidthHz` | `forensic` | voice clarity, bandwidth and EQ decisions |
| 43 | `forensic.onsetRatePerMin` | `forensic` | cut density and sound-accent restraint |
| 44 | `forensic.reverbTailSec` | `forensic` | room-tail handling, silence design and transition tails |
| 45 | `forensic.musicLikelihood` | `forensic` | detect existing music and prevent a conflicting second bed |
| 46 | `speaker.turns` | `speaker` | speaker-change cuts, labels and lower-third timing |
| 47 | `speaker.shares` | `speaker` | speaker attribution confidence and conservative labeling |
| 48 | `speaker.utterances` | `speaker` | speaker-specific shot and caption segmentation |
| 49 | `speaker.meanUtteranceSec` | `speaker` | speaker pacing and conversational shot duration |
| 50 | `speaker.longestSilenceSec` | `speaker` | long-gap review: preserve intentional holds, investigate dead air |
| 51 | `motion.emphasisHits` | `motion` | kinetic word hits: the exact stressed word and its scale pop |
| 52 | `motion.kineticScale` | `motion` | how far a card or caption grows across its line |
| 53 | `motion.pitchDirection` | `motion` | up-screen entry vs a settling move, from the measured slope |
| 54 | `motion.pitchTravel` | `motion` | keyframe distance in bounded pixels |
| 55 | `motion.rateTempo` | `motion` | animation speed and card hold from words per minute |
| 56 | `motion.pauseDwell` | `motion` | holds, next-hero entrances and dwell frame counts |
| 57 | `motion.onsetSync` | `motion` | hit and flash timing on spectral-flux onsets |
| 58 | `motion.rhythmStagger` | `motion` | multi-layer entrance stagger from the rhythm grid |
| 59 | `motion.sentenceEntry` | `motion` | question, exclamation and fragment entry presets |
| 60 | `motion.emotionEase` | `motion` | spring, emphasis, ease-out or linear easing |
| 61 | `motion.arcTurn` | `motion` | background and hero swap at an arc turn |
| 62 | `motion.entityCard` | `motion` | hero-card timing for a named entity |
| 63 | `motion.topicChapter` | `motion` | chapter-card entry at a topic change |
| 64 | `motion.motifReturn` | `motion` | callback visual when a repeated phrase returns |
| 65 | `motion.speakerTurn` | `motion` | lower-third flip at a speaker change |
| 66 | `motion.confidenceGate` | `motion` | no glitch or pop on a low-confidence word |
| 67 | `motion.sibilanceTexture` | `motion` | shimmer intensity from measured sibilance |
| 68 | `motion.reverbSpace` | `motion` | background blur and parallax depth from the room tail |
| 69 | `motion.noiseGrain` | `motion` | grain texture matched to floor and flatness |
| 70 | `motion.levelPulse` | `motion` | scale-pulse strength from line lift |

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
