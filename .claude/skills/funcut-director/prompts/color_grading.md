# Color grading

One look per video, applied to the whole runtime, proven on frames. A video
with no grade is a rough cut; "the footage looked fine" is what a rough cut
always says about itself.

## How a grade is actually applied (the 5-second trap)

There are exactly two correct ways, and one broken one.

**Whole-video layer — the default.** One effect layer spanning the entire
runtime:

```json
effect_layer_add { "path": "...", "start": 0.0,
  "duration": 128.0,   // ← the WHOLE runtime, WRITTEN OUT
  "effectId": "concat.teal-orange", "name": "The look" }
```

**`duration` is the trap**: when it is absent the layer lives **5 seconds**
— the default — and grades five seconds of a two-minute video before
stopping. Write the number. Take it from `timeline_read`'s total, not from
the script's estimate.

**Per-clip chain — when footage sources vary.** The same `videoEffects`
list on every clip, as one `edit_apply` batch:

```json
clip_update { "path": "...", "clipId": "c7",
  "patch": { "videoEffects": [ { "id": "concat.teal-orange", "params": { "amount": 70 } } ] } }
```

**Broken:** `effect_layer_add` on the first clip with no duration, or
different looks on different sections "for variety". One video, one look.

## The intensity key is the package's own

`effects_list` names each package's parameters and ranges. The commonly
used recipes:

| Package | Key | Range | Default |
| --- | --- | --- | --- |
| `concat.teal-orange` | `amount` | 0–100 | 70 |
| `concat.warm` | `temperature` | 3000–6000 K | 4600 |
| `concat.cool` | `temperature` | 3000–6000 K | — |
| film-look packages | see `effects_list` | | |

A key the package does not have is silently ignored — set `intensity` on
`concat.teal-orange` and nothing happens. Check the key in `effects_list`
before writing the plan.

## Recipes by niche

| Niche | Recipe | Notes |
| --- | --- | --- |
| Finance | `concat.teal-orange` amount 60–75, or cool steel look | numbers pop against cool shadows; keep skin warm |
| Tech | `concat.cool` ~5600 K or a clean contrast look | clinical, not blue-mud |
| Motivation | warm, lifted contrast | golden-hour adjacent |
| Documentary | gentle `concat.warm` ~4800 K | honesty over style |
| History | sepia-adjacent warm, slight fade | era, not costume |
| Gaming/vlog | punchy contrast + saturation | the footage's own energy, disciplined |
| True crime | desaturated, cool, crushed blacks | dread, not darkness |
| Luxury | low-sat, warm highlights | restraint reads as price |
| Education | neutral + slight warmth | nothing between the viewer and the idea |
| Fitness | high contrast, saturated | sweat should gleam |

Accent with texture only when the niche earns it: film-grain and halation
packages at low intensity for history/luxury/motivation; never in tech or
education.

## Aspect and fit (the grade's neighbor — do it in the same pass)

Vertical 9:16 delivery with landscape footage: `timeline_background
{kind: "blur"}` is the deliberate style choice; `clip_frame {framing:
"fill"}` on every clip is the default (cover and crop). A letterboxed or
pillarboxed frame at review is a defect to fix, not a look.

## Prove it

Two `preview_frame`s, at 25% and 75% of runtime, comparing against the
ungraded intent: the same recipe, the same strength, both frames. A grade
that shows at 0:30 but is gone by 1:30 is the 5-second trap; a grade that
changes character mid-video is two recipes fighting.
