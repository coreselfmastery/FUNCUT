# The signal matrix — spend every reading

`voice_signals` answers 25 families and at least 50 readings. Reading the
profile is not spending it. This matrix names the decision each reading
informs; the plan's `signalLog` must name the reading (or its family) and
the confidence and basis for every decision it drove. A reading that was
not measured is named as unmeasured — never cited with a confidence of 0.

The recipes that turn a reading into a cut seam, a density and a cut type are in
`prompts/signal_recipes.md`.

| # | Reading | It informs |
|---|---|---|
| 1 | `pause.start` | where a cut, hold or breath begins |
| 2 | `pause.end` | where the next line or visual can start |
| 3 | `pause.class` | `dead_air` is cut, `breath`/`beat`/`dramatic` are kept |
| 4 | `pause.keep` | the protected hold the edit must not remove |
| 5 | `emphasisWords` | caption emphasis, graphic beats, SFX and cut accents |
| 6 | `speechRate.wordsPerMinute` | caption group length and cut density |
| 7 | `speechRate.syllablesPerSec` | syllable-timed caption animation |
| 8 | `rhythmLayers` | beat grid and section pacing |
| 9 | `motifs` | recurring phrase, callback and visual motif |
| 10 | `words` | exact caption text and word-timed reveals |
| 11 | `language` | caption language, font and reading direction |
| 12 | `sentence` | sentence boundaries and caption grouping |
| 13 | `entities` | names, numbers and places that earn a card |
| 14 | `speechActs` | question, promise, command and CTA beats |
| 15 | `claims` | evidence slot and the graphic that proves it |
| 16 | `forensic.integratedLufs` | mix target and loudness repair |
| 17 | `forensic.noiseFloorDbfs` | noise gate, room tone and repair |
| 18 | `forensic.snrDb` | whether a take is usable under music |
| 19 | `forensic.spectralFlatness` | noise type and repair choice |
| 20 | `forensic.crest` | transient preservation and compression |
| 21 | `forensic.dynamicRangeDb` | dynamic contrast and ducking depth |
| 22 | `musicSegments` | BGM gate: never stack a bed on a musiced take |
| 23 | `speechRatio` | how much air a section can carry |
| 24 | `speakerChanges` | cut, caption speaker and lower-third changes |
| 25 | `topics` | section map and chapter turns |
| 26 | `discourseMarkers` | transition and turn placement |
| 27 | `loudness.integratedLufs` | platform target and final mix proof |
| 28 | `loudness.truePeak` | peak ceiling and limiter choice |
| 29 | `prosody.pitchSlope` | rising/falling line and cut energy |
| 30 | `prosody.pitchRange` | emphasis strength and type scale |
| 31 | `prosody.relativeLu` | relative loudness and accent placement |
| 32 | `prosody.finalRise` | question detection and caption punctuation |
| 33 | `prosody.voicedRatio` | pause/breath classification |
| 34 | `prosody.hnr` | voice clarity and repair |
| 35 | `prosody.jitter` | vocal roughness and repair |
| 36 | `prosody.shimmer` | vocal roughness and repair |
| 37 | `fillers` | cut decision and caption cleanup |
| 38 | `arc` | act structure, turn and payoff |
| 39 | `emotion` | grade, music, pace and stock mood |
| 40 | `formants` | voice character and speaker identity |
| 41 | `speaker` | speaker label and turn |
| 42 | `noiseFloor` | room tone and silence design |
| 43 | `take.duration` | runtime, shot budget and section length |
| 44 | `take.samplePeak` | clipping repair and limiter |
| 45 | `take.clippingRatio` | damaged-word detection |
| 46 | `take.sibilanceRatio` | de-esser and caption timing |
| 47 | `take.humHz` | hum repair and room tone |
| 48 | `take.dcOffset` | DC repair and low-end cleanup |
| 49 | `take.spectralRolloff` | brightness and grade direction |
| 50 | `take.spectralBandwidth` | clarity and noise repair |
| 51 | `take.onsetRate` | cut grid and SFX accents |
| 52 | `take.musicLikelihood` | BGM gate and licence warning |
| 53 | `take.reverbTail` | room character and tail handling |
| 54 | `take.loudnessRange` | dynamic arc and ducking |

The matrix is deliberately wider than the 25 families: a family can carry
several readings, and a plan that cites only the family name has not spent
the reading inside it. If a reading is absent, write it in the report as
unmeasured and use the nearest served reading; do not invent a value.
