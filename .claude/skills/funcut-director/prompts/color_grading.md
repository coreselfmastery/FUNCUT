# Color grading

One look per video, chosen from the niche playbook, set once. The engine's
57 filters ARE the grade — apply via `effect_layer_add`, tune via
`effect_key_set` (each package's parameter key/min/max/default comes from
`effects_list` at runtime — read it, don't recall it). Never stack two color
filters on one clip; never change the look mid-video.

## The palette recipes (niche → filter)

| Niche | Filter | Intensity notes |
| --- | --- | --- |
| History | `concat.super-8` or `concat.noir` (b/w period) or `concat.polaroid` (photo-archive) | light touch — texture over tint |
| Tech | `concat.cool` + `concat.vaporwave` only for retro beats | cool blues, low warmth |
| Luxury | `concat.teal-orange` at moderate, or `concat.warm` candlelight (3000–4200 K range of its `temperature` param) | rich, never neon |
| Motivation | `concat.warm` (sunrise) or teal-orange (gym grit) | mid-high contrast |
| Finance | `concat.cool`, `concat.bleach-bypass` for crash/crisis beats | desaturated = serious |
| Science | `concat.teal-orange` or neutral + `concat.warm` for nature beats | clean, one accent hue |
| Horror | desaturate via `concat.mono`-adjacent grades + `concat.vhs` found-footage beats | low saturation, crushed blacks |
| Fitness | `concat.bleach-bypass` or high-intensity warm | sweat-and-chrome |
| Gaming | `concat.vaporwave`, `concat.cyberpunk`, `concat.vhs` CRT beats | neon allowed here |
| How-to | `concat.warm` low or none — clarity first | the object's real color is the content |

The full filter shelf also includes: mono, warm, cool, noir, vhs, polaroid,
super-8, vaporwave, cyberpunk, teal-orange, bleach-bypass — plus
non-color "looks" you should NOT use as a grade (glitch effects, fisheye…).
`effects_list {kind:"filter"}` is the runtime source of truth; if a name here
is missing there, trust `effects_list`.

## How to apply

1. `effects_list {kind:"filter"}` → confirm the id + its parameter key
   (e.g. `concat.warm` → `temperature`, 3000–6000 K, default 4600).
2. `effect_layer_add` on the FIRST clip; record the layer/effect id.
3. `effect_key_set` the intensity once. For clips added later, replicate the
   SAME filter + SAME intensity — consistency beats perfection.
4. A/B check: `preview_frame` at a mid-segment before and after — the grade
   should be legible at thumbnail size, not subtle at 10 %.

## Grade discipline

- Skin-ish subjects (even stock hands/faces): teal-orange and warm are safe;
   heavy mono/bleach only on objects and landscapes.
- Grain/texture filters (super-8, vhs, polaroid) are PERIOD signals — use on
   archive material, not the whole modern explainer.
- Vertical 9:16 on a 16:9 canvas: `timeline_background {kind:"blur"}` (or a
  brand color for luxury/finance) — the blurred sides are part of the look,
  keep them neutral.
- The export pass may add `concat.loudness` (audio) — color has no such
  auto-pass: what you set is what ships.

## Self-check

- [ ] Exactly one color filter; same id and intensity on every clip.
- [ ] Chosen from the niche playbook, not vibes.
- [ ] `preview_frame` confirms legibility at small size.
- [ ] Period textures only on period material.
- [ ] Backgrounds (blur plates) stay neutral.
