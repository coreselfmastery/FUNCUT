# The style bible — one film, not five videos

A cut feels amateur when each shot, caption, graphic and grade was chosen
on its own. The fix is not a filter. It is one written style bible the
whole pipeline obeys, from the first stock search to the last sound
effect.

Set it before the edit plan. It is a `style` object in the plan; a plan
without it is structurally valid but **not `creativeReady`**.

## The contract

```json
{
  "name": "basalt / cold-warm / vertical",
  "palette": ["#0b0d12", "#f2e9dc", "#c96a3a"],
  "grade": "concat.teal-orange",
  "captions": {
    "font": "Inter",
    "weight": 700,
    "emphasisColor": "#f2e9dc",
    "position": "lower-third"
  },
  "motion": {
    "language": "measured push-ins, paper cuts, one kinetic type system",
    "easing": "spring-pop",
    "intensity": 0.62
  },
  "sound": {
    "duckDb": -14,
    "texture": "dry room, low synth pulse, airy transitions"
  },
  "footage": {
    "orientation": "vertical",
    "treatment": "fill",
    "continuity": "one camera height, no letterbox, colour follows the act"
  },
  "continuityRules": [
    "fill every frame; never letterbox a vertical reel",
    "one accent colour per section",
    "no two consecutive shots share a camera height",
    "every generated or stock still carries the same grain and contrast"
  ]
}
```

## The seven decisions

1. **Palette** — two or three colours, in order of dominance. Every
   generated graphic, caption accent, grade tint and motion plate draws
   from them. A fourth colour is a different film.
2. **Type** — one display face and one workhorse face, one caption weight,
   one accent colour. Do not let a preset choose a new font.
3. **Grade** — one filter, one intensity curve. The grade can breathe with
   the acts; it cannot become a different grade.
4. **Footage treatment** — `fill` (cover and crop) or a deliberate
   `blurPlate`. Never letterbox by accident. A 16:9 clip in a 9:16 reel is
   filled and reframed, not shrunk with bars.
5. **Motion hand** — one easing family, one entrance vocabulary, one
   intensity. A comp can be a chart, a card, a 3D camera move or a kinetic
   caption; it cannot feel like it came from another product.
6. **Sound world** — one room, one low-end, one transition texture. Duck
   depth is part of the look.
7. **Continuity rules** — the sentences a reviewer can check. If a rule is
   not checkable, it is a mood, not a rule.

## Where it is enforced

- `plan_validate` compares `style.grade` to `grade.filter`, the caption
  accent to `captions.emphasisColor`, the duck depth to
  `audio.bgm.duckDb`, and the footage orientation to `aspect`.
- `footage_continuity` checks the filled manifest for colour jumps,
  reversed travel, repeated scales and one-category runs. Treat every flag
  as a style-bible violation, not a suggestion.
- `craft_critique` sees the frames; ask it specifically whether the film
  reads as one look.
- The final `review.md` pass checks the style bible against the finished
  timeline, not against the plan's promises.

## Stock, AI stills and generated video

One style does not mean one source. It means one treatment:

- grade every source through the same look;
- crop every source to the same frame treatment;
- put the same grain/contrast pass on stock, generated stills and
  generated video;
- keep one camera-height and movement rule;
- keep one accent colour, even when the model generates a new image;
- reuse a generated **style key** image as the reference for every later
  image and as the first frame for image-to-video.

If a source cannot be made to match, do not use it. One mismatched shot is
more visible than one missing shot.

## Vertical reels

A vertical film is a different composition, not a cropped horizontal one:

- `footage.orientation` is `vertical`; shop vertical stock first;
- keep the subject in the middle 60% and the caption above the platform's
  bottom UI;
- use `fill`, never a letterboxed 16:9 frame;
- push-ins read stronger vertically; lateral whips read weaker;
- one idea per screen, because the viewer sees one screen at a time.
