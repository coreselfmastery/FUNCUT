# Keyframes and motion (the entrance/exit grammar)

Every clip enters on purpose and leaves on purpose. "Raw in, raw out" is
the single biggest tell of a template edit — this file is the cure.

## The grammar

| Move | Duration | Curve | Via |
| --- | --- | --- | --- |
| Pop in | 120–180 ms | spring (stiffness 260, damping 15) | `keyframe_set scale 0.92 → 1.0` |
| Punch zoom | ~150 ms | sharp out-ease | `keyframe_set scale 1.0 → 1.08` on the claim word |
| Slide in | 200–250 ms | `cubic-bezier(0.16, 1, 0.3, 1)` | `keyframe_set offsetX/offsetY` |
| Dissolve | 200–300 ms | out-ease | `clip_update {fadeIn}` |
| Ken Burns drift | the clip's whole life | linear or gentle ease | two `keyframe_set` keys: scale 1.0→1.06, offset a few % |
| Exit fade/slide | 200–300 ms | in-ease | `clip_update {fadeOut}` or a late key |
| Speed ramp | through the cut | the curve's own | `edit_apply setClipSpeedCurve` |

Rules of the grammar:

- **One move a clip.** A clip that pops AND slides AND zooms is a
  screensaver. The move serves the word being said.
- **Stagger a family**: three stat cards pop 80 ms apart; two-line titles
  rise 60 ms apart. Nothing professional lands simultaneously.
- **Ken Burns is a floor, not a move**: a slow 4–8% drift keeps a long
  clip alive but is not an entrance. Add the entrance anyway.
- A deliberate `static` exists — one per piece at most, for effect, and
  you should be able to say why.

## Punch zooms (short-form's signature)

On the emphasized word: `keyframe_set {clipId, property: "scale", keys
at the word's time: 1.0 → 1.06–1.1 → settle}`. In a reel, one punch
every 3–6 s, each with its tick/SFX. Never two punches the same size
back to back — vary 1.05/1.10.

## Parallax (depth on stock footage)

Two B-roll layers of the same scene family, back one scaled 1.1 and
blurred slightly, front one drifting opposite: 4 keys total, a poor
man's 2.5D. Use for calm sections; it reads as craft instantly.

## Per-clip vs per-property

`keyframe_set` animates one property of one clip (`scale`, `offsetX`,
`offsetY`, `rotation`, `opacity`, `volume`, mask shapes). A full
entrance (slide + fade) is two `keyframe_set`s in one `edit_apply`
batch, timed to land together. The comp layer (`motion_bake`) is for
everything that is more than a property — text, cards, charts,
backdrops.

## What the floors demand

- Hard floor M: every clip has an entrance; exits or hand-offs on
  section boundaries.
- Hard floor F: `clip_frame {framing: "fill"}` comes BEFORE the
  keyframes (a zoom on an unfitted clip zooms the letterbox too).
- Keyed properties interpolate in the frame's own coordinates; offsets
  are frame fractions (±3 max), scale 0.05–8 — the engine clamps, but a
  plan that needs the clamp is a plan that mis-measured.
