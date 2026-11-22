# Signal recipes — from measurement to a cut

A signal is not a field to cite. It is a fork in the edit. These recipes are
the Director's decision layer: read the profile, score the seam, choose the
cut, then write the choice into `signalLog` with its confidence and basis.

## 1. Score a cut seam

For every candidate boundary, build a score from the readings that are
actually present:

```
cutScore =
    pauseClass(dead_air=+3, breath=+1, beat=+2, dramatic=-3, scene_turn=+2)
  + emphasisWord(+2)
  + sentenceBoundary(+1)
  + discourseMarker(+1)
  + beatDistance(<=0.12s:+2, <=0.25s:+1, else:0)
  + speakerChange(+2)
  + topicTurn(+1)
  + emotionalTurn(+2)
  + energyRise(+1)
```

Cut when the score is **≥ 4**, hold when a `dramatic` pause or a `hold`
decision outweighs it, and never cut a word the transcript gates. A score of
3 is a judgment call: keep the take if the performance is the reason to watch.
This is why the Director is not a silence remover: it cuts on meaning first,
then lands the cut on the nearest measured seam.

## 2. Set the cut density

```
targetCPM = registerBase
          * speechRateFactor      // 150 wpm = 1.0, 180 = 1.2, 110 = 0.8
          * energyFactor          // arc energy and emotion intensity
          * sectionFactor         // hook 1.15, body 0.9, turn 1.1, cta 0.85
```

Register bases: calm 6–10, standard 10–16, energetic 16–24, hype 24+.
Then **clamp to the material**: a measured `speechRatio` under 0.55 means the
take is mostly air; do not fill it with cuts, design the silence. A take with
`rhythmLayers` at 0.4–0.8 s and a strong `beat` can run 20% denser; a
`slow`/`editorial` pace class must not be cut at hype density. The number is
a target, not a quota: the 4-second law is the ceiling, the story is the floor.

## 3. Choose the cut type

| Signal | First choice | Second choice | Never |
| --- | --- | --- | --- |
| `speakerChanges` | `j` or `reaction` | `cutaway` | a hard cut that straddles the change |
| `emphasisWords` | `smash` or `beat` | `contrast` | a cut two words after the peak |
| `claims` / `entities` | `insert` | `cutaway` | a generic pretty clip |
| `sentence` boundary | `hard` or `match` | `l` | a flash on every sentence |
| `discourseMarkers` / `topics` | `l` or `whip` | `cross` | a hard cut with no change |
| `rhythmLayers` | `beat` | `whip` | beat-cutting a slow editorial |
| `emotion=fear` / `anger` | `jump` / `smash` | `contrast` | a soft dissolve on a threat |
| `emotion=joy` | `match` / `action` | `beat` | a cold smash on warmth |
| `emotion=sadness` | `cutaway` / hold | `l` | a whip on grief |
| `emotion=surprise` | `freeze` / `flash` | `smash` | more than one flash per section |
| `arc` turn | `contrast` / `cross` | `smash` | the same cut as the setup |

A run of six `hard`s is a slideshow; a run of six `smash`es is a strobe. The
variety floor is four types across a piece, and the type must answer the
change, not decorate it.

## 4. Caption, graphic, sound and grade

- **Caption group** = `sentence` + `speechRate` + `emphasisWords`: 2–4 words
  at conversational pace, 1–3 at punchy, one clause at editorial; the
  stressed word gets the accent treatment the plan names.
- **Graphic** = `entities` + `claims` + `sentence type`: a number, percentage,
  comparison, ranking, trend or process becomes a comp; a name becomes a
  lower third; a question becomes a reveal, not a text card.
- **SFX** = `emphasisWords` + `pause.class` + `rhythmLayers`: land on the
  peak, leave the breath, never machine-gun. A `dramatic` pause gets air, not
  a riser.
- **BGM** = `musicSegments` + `speechRatio` + `noiseFloor`: a take that
  already carries music gets no second bed; a dry voice gets a bed ducked by
  the act's `duckDb`; a high speech ratio means a lower bed.
- **Grade** = `emotion` + `colourTemp` + `take.spectralRolloff`: one look per
  film, pushed per act; never a different LUT per clip.
- **Repair** = `forensic.*` + `take.*`: noise, clipping, hum, sibilance and
  loudness are fixed before they are cited; an un-repaired take poisons every
  later signal.

## 5. Confidence and absence

- `confidence >= 0.75`: spend the reading directly.
- `0.5–0.75`: spend it with a conservative fallback (hard cut, hold, plain
  caption) and say so.
- `< 0.5`: use the nearest served reading, choose the plain option, and log
  the uncertainty.
- **Absent**: name it unmeasured in the report and use the conservative
  fallback. Never write a confidence of 0 as if it were a measurement.
