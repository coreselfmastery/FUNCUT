# Keyframes and motion

Static stock footage is dead weight; the Ken Burns family of moves is what
makes 4 still-ish clips feel directed. All of this is `keyframe_set` /
`clip_transform` / `setClipTransform` via `edit_apply`.

## The grammar

- Properties: `scale` (0.05–8, 1 = native), `offsetX`/`offsetY`
  (frame-fractions, ±3), `rotation` (degrees), `opacity` (0–1), `volume`,
  and the mask fields. `at` is a **0–1 fraction of the clip** — convert
  times: `at = (t - clipStart) / clipLength`.
- `ease` is a CSS cubic-bezier `[x1,y1,x2,y2]`. The working set:
  - linear `[0,0,1,1]` — machine moves, tracked overlays
  - ease-out `[0.16,1,0.3,1]` — things ARRIVING (entrances)
  - ease-in `[0.7,0,0.84,0]` — things LEAVING (exits)
  - smooth in-out `[0.25,0.1,0.25,1]` — long drifts (Ken Burns)
  - overshoot `[0.2,1.4,0.4,1]` — pops and punch-ins
- Two keys minimum: the property's start (at 0) and end (at 1 or mid).
  `keyframe_clear` removes one; `clearClipKeys` (via edit_apply) all.

## The recipes (motion field values in the plan)

| Motion | Keys (start → end) | Clip length | Use |
| --- | --- | --- | --- |
| `ken-burns-in` | scale 1.0 → 1.08, ease in-out, 0→1 | 3–6 s | default for calm/expository |
| `ken-burns-out` | scale 1.08 → 1.0 | 3–6 s | reveals, settling |
| `parallax-up` | offsetY 0.02 → −0.02 + scale 1.04→1.0 | 3–6 s | vertical-feel depth on widescreen |
| `slide-left` | offsetX 0.06 → 0 (ease-out, 0→0.3), then hold | any | B-roll entrances on cuts |
| `slide-right` | mirror | any | variety |
| `punch-zoom` | scale 1.0 → 1.12 → 1.0, overshoot ease, 0→0.12→1 | 1.5–3 s | beats, numbers, comedy |
| `static` | none (or scale 1.02 drift) | ≤ 3 s | between two moving shots — stillness is rhythm |

Rules of dose: ONE motion type per segment; the HOOK and the PAYOFF get the
strongest moves; a video of all punch-zooms is a slideshow on energy drinks.
Cut motion when the clip itself moves (`camera: zoom_in`) — don't double-move.

## Speed and time (raw edit_apply commands)

- `setClipSpeedCurve` — eased speed ramp: e.g. speed 1.0 → 0.4 across a
  reveal (the "wait for it" beat), then back. Ramps ≤ 1.5× change read best.
- `freezeFrame` — hold the exact frame at a time (the dramatic stop),
  typically 0.3–0.8 s, pair with an Impact SFX.
- `setClipSpeed` straight — 1.05–1.1 tightens a slow VO clip; ≤ 0.0625 is
  a freeze in spirit.

## Mask motion (the pro touch)

Keyframe `maskWidth`/`maskHeight`/`maskX` on a `rectangle` or `linear` mask
over a text card for a wipe-in reveal (120–200 ms), or animate `maskFeather`
0.3 → 0.05 to sharpen a reveal. Shapes: linear, mirror, circle, rectangle,
heart, star. One masked reveal per video is plenty.

## The 4 GB-laptop reality

Every keyframe is free (params, not renders) — but heavy stacks of effects +
keyframes make exports longer. Keep per-clip stacks ≤ 2 effects (grade +
transition) and prefer transform moves over effect-driven motion when both
would do.

## Self-check

- [ ] Every B-roll segment carries a motion field (or a deliberate `static`).
- [ ] Motions alternate; no two identical moves adjacent.
- [ ] Easings match direction (out for entrances, in for exits).
- [ ] Speed ramps used at most twice; freeze frames at most once.
- [ ] `at` values computed from clip-relative time, re-checked after trims.
