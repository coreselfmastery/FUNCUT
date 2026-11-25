# The visual model shelf

The picture's reader (`sense_visual`, `visual_sense.py`) runs on nothing but
the standard library and an ffmpeg. Everything in this file is a **door**: a
model that adds one reading the classical layer cannot produce, runs when its
file and `numpy` + `onnxruntime` are present, and refuses with the reason when
they are not. No door is required for an edit to finish, and no door is
silently skipped.

**38 actions, 11 of them model-free, 30 rows on the shelf.**

## Installing a door

Three calls, all on the Hub's own API - this module never invents a file
address:

```text
sense_visual {action: "hf_search", query: "depth anything onnx"}
sense_visual {action: "hf_files", hfRepo: "onnx-community/depth-anything-v2-small"}
sense_visual {action: "download", modelId: "depth-anything-v2-small",
              hfRepo: "onnx-community/depth-anything-v2-small",
              file: "onnx/model_quantized.onnx"}
```

A row with a verified repo downloads with `modelId` alone (`transnetv2`,
`depth-anything-v2-small`, `dinov2-small`, `clip-vit-b32`). A gated repo takes
`token` or `HF_TOKEN`, and the token is never echoed back. With `sha256` the
digest is checked before anything is kept; without it the digest that arrived
is **pinned** in the folder's `index.json` and checked on every later download
of that file - trust on first use, recorded, not silent.

Files land in the app's own model folder
(`<data>/app.concat.editor/cutout-models`, or `FUNCUT_VISUAL_MODELS`, or
`modelDir`), which is the folder the reader reads. Download it once and the
picture reader uses it.

`sense_visual {action: "models"}` lists every row with `present`, `path`,
`onDiskBytes`, `pinned` and `ready`.

## What runs on any machine (no file, no runtime)

| Reading | Signal | Spends |
| --- | --- | --- |
| `measure_motion` | per-frame motion vectors: direction, magnitude, onset | cut timing, motion sync, SFX sync |
| `camera_motion` | global camera motion: pan / tilt / zoom / static | cut continuity, motion preset, stock query |
| `shot_scale` | wide / medium / close-up from the subject's area | cut rhythm, motion design, caption placement |
| `visual_sync` | sync score correlating audio onset with flow onset | sound design, cut timing |
| `scene_cuts` | shot changes inside a clip: cuts and dissolves, from frame difference | the cut stage: a montage clip is not one take, and a seam is a cut site |
| `saliency` | where the eye lands, as a grid and a focus box | the caption band, which moves away from it |
| `palette` | dominant colours per scene | motion style, continuity, stock similarity |
| `visual_quality` | noise variance per clip | stock filter, quality gate |
| `composition` | composition score and subject placement | stock ranking, caption placement |
| `motion_blur` | directional blur probability | stock filter, cut veto |
| `tracks` | persistent identities from per-frame boxes | subject continuity, cut continuity |

## The doors

One decoder per reading kind - `logits`, `prob`, `embedding`, `depth`, `masks`,
`boxes`, `keypoints`, `segments`, `scalar`, `flow` - so a door is a row, not a
module: point `modelId` at another checkpoint in the same class and nothing
else changes. `generic: true` forces the generic runner even where a
specialised door exists. The four captioning rows are `vlm: true`: they are
chat models, not graphs, and `describe_scene` is their door.

| Model | Tier | Signal | Size | Licence | Door | Decoder | Hub repo / file | Spends |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `movenet-a0` | 1 | temporal action class per window (600 Kinetics actions) | 15 MB | Apache-2.0 | `action_class` | `logits` | — |  |
| `transnetv2` | 1 | shot-boundary probability per frame | 30 MB | MIT | `shot_boundaries` | `prob` | `elya5/transnetv2` / `transnetv2.onnx` | the cut stage's own read: a boundary is where a cut can land, a pause is only where it must |
| `smolvlm2-500m` | 1 | bounded scene description: environment, objects, activity | 546 MB | Apache-2.0 | `describe_scene` | `logits` | — | the repo's own local_vlm; schema-constrained, never free creative judgment |
| `vjepa2-vitl` | 1 | self-supervised video embedding + action head | 1130 MB | MIT | `video_embedding` | `embedding` | — | a stronger temporal encoder than MoViNet and 75x the bytes; the bench decides |
| `film-shot-classifier` | 1 | shot type: wide, medium, close, extreme close, insert, two-shot | 90 MB | Apache-2.0 | `film_shot_type` | `logits` | — |  |
| `scenegraph-vlm` | 1 | subject-predicate-object graph per scene | 800 MB | CC-BY-4.0 | `scene_graph` | `logits` | — | relationships CLIP similarity cannot express; the bounded-schema rule applies |
| `depth-anything-v2-small` | 2 | per-pixel relative depth | 27 MB | Apache-2.0 | `depth_map` | `depth` | `onnx-community/depth-anything-v2-small` / `onnx/model_quantized.onnx` |  |
| `midas-v21-small` | 2 | monocular relative depth (the alternative to Depth Anything) | 80 MB | MIT | `depth_map` | `depth` | — |  |
| `fastsam-s` | 2 | pixel-accurate object masks | 40 MB | Apache-2.0 | `subject_mask` | `masks` | — |  |
| `segformer-b0` | 2 | semantic segmentation (150 ADE20K classes) | 15 MB | Apache-2.0 | `semantic_segments` | `segments` | — |  |
| `bytetrack-yolo` | 2 | persistent object tracks across frames | 15 MB | MIT | `detect_objects` | `boxes` | — | the detector's boxes feed `tracks`, which links them into identities |
| `dinov2-small` | 3 | 384-d visual embedding per frame | 84 MB | Apache-2.0 | `visual_embedding` | `embedding` | `onnx-community/dinov2-small` / `onnx/model_quantized.onnx` |  |
| `clip-vit-b32` | 3 | image-text similarity embedding | 150 MB | MIT | `visual_embedding` | `embedding` | `Xenova/clip-vit-base-patch32` / `onnx/vision_model_quantized.onnx` |  |
| `siglip2-base` | 3 | vision-language embedding (the CLIP successor to bench) | 400 MB | Apache-2.0 | `visual_embedding` | `embedding` | — |  |
| `raft-small` | 3 | learned optical flow (better on complex scenes) | 4 MB | BSD-3-Clause | `optical_flow` | `flow` | — |  |
| `dover-mobile` | 4 | aesthetic + technical quality score per video | 100 MB | Apache-2.0 | `aesthetic` | `scalar` | — |  |
| `laion-aesthetic` | 4 | aesthetic score 0-10 per frame | 4 MB | MIT | `aesthetic` | `scalar` | — |  |
| `vinet-s` | 4 | video saliency map | 36 MB | Apache-2.0 | `video_saliency` | `depth` | — | the map's peak is where the eye already is, so a cut there lands on attention |
| `smblurdetect` | 4 | motion blur probability / mask | 8 MB | Apache-2.0 | `blur_mask` | `masks` | — |  |
| `movenet-pose` | 5 | 17 body keypoints per person | 5 MB | Apache-2.0 | `pose` | `keypoints` | — |  |
| `mediapipe-hands` | 5 | 21 hand landmarks + gesture class | 7 MB | Apache-2.0 | `hands` | `keypoints` | — |  |
| `eyegaze` | 5 | gaze pitch/yaw per face | 10 MB | research-only (Gaze360) | `gaze` | `keypoints` | — | licence blocker: research-only, never a commercial path |
| `mobilegaze` | 5 | gaze pitch/yaw per face (mobile variant) | 5 MB | research-only (Gaze360) | `gaze` | `keypoints` | — |  |
| `microexpnet` | 5 | emotion class per face | 1 MB | MIT | `face_emotion` | `logits` | — |  |
| `syncnet` | 6 | lip-sync score | 20 MB | MIT | `lip_sync` | `scalar` | — |  |
| `aqa-transformer` | 6 | how well an action was performed | 60 MB | research | `action_quality` | `scalar` | — |  |
| `florence-2-base` | 6 | lightweight captioning / VQA | 230 MB | MIT | `describe_scene` | `logits` | — |  |
| `uform-gen2` | 6 | lightweight captioning / VQA (UForm) | 400 MB | Apache-2.0 | `describe_scene` | `logits` | — |  |
| `grounding-dino-tiny` | 6 | open-vocabulary object detection by text query | 628 MB | Apache-2.0 | `detect_objects` | `boxes` | — | pass `labels` (the phrase list) with the call |
| `smolvlm2-256m` | 6 | the small VLM variant, for 4 GB machines | 256 MB | Apache-2.0 | `describe_scene` | `logits` | — |  |

## What a door may spend, and what it may not

- **Direct action**: an optical-flow peak is where a cut lands; a scene
  boundary is where a cut is placed; a depth map is the parallax layer.
- **Veto**: a technical read (soft, noisy, letterboxed) or a blur read
  rejects a stock candidate before the ranking sees it.
- **Confidence modulation**: a low-confidence read down-weights the decision
  it would have driven, and says so in its own output.
- **Repair selection**: a caption that would cover the subject is moved, not
  dropped.
- **Diagnostic**: a similarity score is recorded and not spent, on purpose.

## The sheet's enrichment

`measure_clips` is what the AUTO EDIT button's stock choice reads. It scores
each candidate on sharpness, noise, bars, measured motion and composition, and
adds a blur veto beside the technical one. When the shelf's doors are
installed it also runs, per clip, at most `enrichDoors` (default 3) of
`dover-mobile` (a measured taste prior, blended at a tenth of the score),
`film-shot-classifier` (shot type), `depth-anything-v2-small` (near/far split)
and `movenet-a0` (pose). With nothing installed the sheet is the classical one
and costs a `stat()` per door; `enrich: false` forces that path.

## Honest caveats

- `smolvlm2-500m` is the repo's `local_vlm`; `docs/VLM.md` records it
  hallucinating a whole film from one blue frame, so its door asks for a
  schema and refuses free-form judgment.
- A row's size is the *upstream* file, not what a quantized Hub conversion
  weighs: `onnx-community/depth-anything-v2-small` is 27 MB quantized, not the
  99 MB fp32 file.
- Verified end to end in this tree, on real weights: `transnetv2` (a cut at
  frame 39 of a synthetic 100-frame clip, p 0.9996), `depth-anything-v2-small`
  (a 518x518 map, near/far split correct), `dinov2-small` (a CLS embedding).
- `licence` is what the upstream page says; check it before shipping a model
  in a commercial build, and read `docs/STOCK_LICENSING.md` for footage.
