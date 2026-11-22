# Generative media — use it when it exists, never fake it

The editor can make pictures that stock does not have: a style key, a
metaphor, a period reconstruction, an impossible camera move, a product
hero, a texture, a transition plate. Use a real generator when one is
installed. If none is, say so and use footage, Geneva comps and typography.

## Detect the generator first

The supported external generator is the **Higgsfield CLI** (the
`higgsfield-skills` shelf in `maestro` documents it):

```sh
command -v higgsfield
higgsfield account status
higgsfield model list --json
```

If `higgsfield` is absent, do **not** invent a result. Use the project's
own footage, licensed stock, Geneva comps and type. A missing generator is
an honest limitation, not a reason to ship a broken asset.

If `higgsfield account status` says the session is expired, stop and report
it. Authentication is interactive; an unattended run cannot log in for the
user.

## The model map

| Need | Model | Why |
| --- | --- | --- |
| One style key / character / stylised still | `nano_banana_2` | reference-driven, consistent character and style |
| Fast style edit | `nano_banana_2_lite` | cheap iteration before the final |
| High-fidelity general image, type on image, design | `gpt_image_2` | the general image default |
| Serious video, multi-shot, image-to-video, 4K | `seedance_2_0` | the production video default |
| Reference-to-video, up to 7 images | `gemini_omni` | when the style key must carry into motion |
| 3D asset from 1-4 reference images | `multi_image_to_3d` | a real GLB, not a rendered picture |
| SFX, ambience, Foley, music-like audio | `seed_audio` | one sound-design door |

Check the live catalogue before choosing:

```sh
higgsfield model get nano_banana_2 --json
higgsfield model get seedance_2_0 --json
```

## The style-key workflow (the one that keeps a film consistent)

1. Write the style bible first (`style_bible.md`). The generator must serve
   it, not replace it.
2. Generate **one** style key with `nano_banana_2` at the delivery aspect:
   ```sh
   higgsfield generate create nano_banana_2 \
     --prompt "<the style bible: palette, light, grain, camera, subject>" \
     --aspect_ratio 9:16 --wait --json
   ```
3. Use that image as the reference for every later still:
   ```sh
   higgsfield generate create nano_banana_2 \
     --prompt "<shot description>, same grade, grain and palette as the reference" \
     --image ./style-key.png --aspect_ratio 9:16 --wait --json
   ```
4. Use the same key as the first frame for image-to-video:
   ```sh
   higgsfield generate create seedance_2_0 \
     --prompt "<camera move and action>" \
     --start-image ./style-key.png --duration 5 --resolution 1080p --wait --json
   ```
5. Download the result into the project's own `generated/` folder, import
   it with `media_import`, then `clip_add`. Never hot-link a temporary URL
   into the timeline.
6. Record the model, prompt, seed/reference and licence in the plan's
   `signalLog` graphic entry and in the final receipt.

## What generative media is for

- a **style key** that anchors palette, grain and light;
- a **metaphor** the narration needs and stock cannot supply;
- a **period or place** that no licensed clip shows accurately;
- a **product hero** with the brand's own object;
- a **background plate** for a kinetic type scene;
- an **impossible transition** (match cut, morph, camera fly-through);
- **Foley/ambience** for a moment the sound shelf does not cover.

## What it is not for

- faking evidence, news, a real person or a real event;
- replacing the narration's actual subject with a generic pretty image;
- generating every shot, which produces a slideshow of unrelated images;
- skipping the style key and letting each generation invent a new look;
- claiming a generated asset is licensed stock or real footage.

## Aspect and vertical rules

- Pass `--aspect_ratio 9:16` for a vertical reel, `16:9` for landscape,
  `1:1` for square. Do not generate 16:9 and letterbox it into a reel.
- Compose the subject in the middle 60%; keep captions out of the bottom
  platform UI.
- For a vertical reel, prefer a vertical push-in or a slow drift; lateral
  camera moves lose their read in a narrow frame.
- A generated still in a reel needs motion: a slow push, a parallax plate
  or a Geneva comp. A static generated image is a slide.

## 3D and bone rigs

Geneva can carry a 3D camera and a bone rig in a comp document. When the
beat is a product turn, a character gesture or a camera fly-through, ask
for a comp with `set-camera` (including `depths` for parallax) or a
`NodeKind::Rig`; do not fake 3D with a flat scale. If the Geneva sidecar
is not installed, the bake refuses the markup-requiring comp with the
install instructions; report that instead of shipping a flat imitation.

## If the generator is absent

The fallback is not "no picture". It is:

1. the project's own footage;
2. licensed Pexels/Pixabay video (video-only in this build);
3. a still extracted from a video with `preview_frame`;
4. a Geneva comp, chart, counter, particle field or kinetic type;
5. typography over a graded plate.

Name which one you used. Never imply a generated asset exists when it does
not.
