# The signal matrix — spend every reading

The runtime `voice_signals` catalogue is the source of truth: it currently
contains **50 named readings across 13 families**. This matrix uses the
catalogue's exact identifiers, not aliases or guessed field paths. Reading
the profile is not spending it. The plan's `signalLog` must name the exact
reading (or a documented family alias accepted by `plan_validate`), the
decision it changed, its confidence and its evidence. A reading that was
not measured is named unmeasured; never cite a value that was not returned.

| # | Exact catalogue reading | Decision it informs |
|---:|---|---|
| 1 | `loudness.integratedLufs` | final mix gain against the platform loudness target |
| 2 | `loudness.loudnessRangeLu` | compression amount and preservation of dynamic contrast |
| 3 | `loudness.truePeakDbfs` | limiter ceiling and clipping prevention |
| 4 | `loudness.meanLufsM` | relative line emphasis, caption weight and accent balance |
| 5 | `loudness.relativeLu` | which lines deserve stronger visual or sound emphasis |
| 6 | `pauses.classes` | cut only dead air; preserve breaths, beats and dramatic holds |
| 7 | `pauses.speechRatio` | BGM level, silence design and visual density |
| 8 | `speechRate.wordsPerMinute` | shot duration, caption group size and pacing |
| 9 | `speechRate.syllablesPerSec` | caption reveal duration and reading-time protection |
| 10 | `words.confidence` | caption inclusion and word-timing acceptance |
| 11 | `language.script` | caption script, font, direction and language-specific layout |
| 12 | `sentence.type` | caption grouping, question holds and transition timing |
| 13 | `entities.cards` | accurate number, money, date, place and name graphics |
| 14 | `motifs.repeats` | recurring visual callbacks when a phrase returns |
| 15 | `fillers.list` | safe filler cleanup without deleting meaningful speech |
| 16 | `arc.emotion` | emotional progression, music, pacing and grade by act |
| 17 | `prosody.emphasisWords` | exact caption emphasis, graphic beats and SFX accents |
| 18 | `prosody.pitchRangeSemitones` | intensity of motion, typography and performance emphasis |
| 19 | `prosody.pitchMeanHz` | voice character and conservative voice/tonal treatment |
| 20 | `prosody.finalRise` | question detection, punctuation and reveal hold |
| 21 | `prosody.voicedRatio` | distinguish speech from pauses and protect spoken words |
| 22 | `prosody.rhythmLayers` | rhythm grid, section pacing and optional beat-aligned cuts |
| 23 | `prosody.phrases` | phrase boundaries, caption chunks and visual phrase changes |
| 24 | `prosody.pitchContourHz` | rising/falling delivery and motion/easing direction |
| 25 | `forensic.noiseFloorDbfs` | noise repair, room tone and BGM headroom |
| 26 | `forensic.snrDb` | voice usability, repair strength and music ducking |
| 27 | `forensic.noiseType` | choose the right noise-removal treatment |
| 28 | `forensic.spectralFlatness` | distinguish broadband noise from tonal material |
| 29 | `forensic.formantsHz` | voice identity and consistency checks; never infer identity beyond the audio task |
| 30 | `forensic.hnrDb` | voice clarity and whether repair is needed |
| 31 | `forensic.jitter` | vocal roughness diagnosis and conservative repair |
| 32 | `forensic.shimmer` | amplitude instability diagnosis and conservative repair |
| 33 | `forensic.fluxMean` | energy changes and where a visual beat may earn emphasis |
| 34 | `forensic.fluxOnsetsSec` | measured transient positions for cuts and SFX accents |
| 35 | `forensic.rmsEnvelope` | dynamic shape, loudness rides and duck automation |
| 36 | `forensic.samplePeakDbfs` | clipping prevention and gain ceiling |
| 37 | `forensic.clippingRatio` | whether the take needs clipping repair or rejection |
| 38 | `forensic.dcOffset` | DC correction before mixing |
| 39 | `forensic.humHz` | identify hum frequency for targeted removal |
| 40 | `forensic.sibilanceRatio` | de-essing and high-frequency voice treatment |
| 41 | `forensic.spectralRolloffHz` | tonal balance and EQ/grade direction |
| 42 | `forensic.spectralBandwidthHz` | voice clarity, bandwidth and EQ decisions |
| 43 | `forensic.onsetRatePerMin` | cut density and sound-accent restraint |
| 44 | `forensic.reverbTailSec` | room-tail handling, silence design and transition tails |
| 45 | `forensic.musicLikelihood` | detect existing music and prevent a conflicting second bed |
| 46 | `speaker.turns` | speaker-change cuts, labels and lower-third timing |
| 47 | `speaker.shares` | speaker attribution confidence and conservative labeling |
| 48 | `speaker.utterances` | speaker-specific shot and caption segmentation |
| 49 | `speaker.meanUtteranceSec` | speaker pacing and conversational shot duration |
| 50 | `speaker.longestSilenceSec` | long-gap review: preserve intentional holds, investigate dead air |

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
