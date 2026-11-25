# The visual model shelf

The picture's reader (`sense_visual`, `visual_sense.py`) runs on nothing but
the standard library and an ffmpeg. Everything in this file is a **door**: a
model that adds one reading the classical layer cannot produce, runs when its
file and `numpy` + `onnxruntime` are present, and refuses with the reason when
they are not. No door is required for an edit to finish, and no door is
silently skipped.

`sense_visual {action: "models"}` lists every row below with `present`,
`path`, `onDiskBytes` and `ready`; `sense_visual {action: "download",
modelId, url, sha256}` fetches one file, verifies the digest it was given and
keeps nothing on a mismatch. The module never invents a download address: the
`upstream` column is the model's own page, not a file.

## What runs on any machine (no file, no runtime)

| Method | Reading | Spends |
| --- | --- | --- |
| `measure_motion` | per-frame motion vectors: direction, magnitude, onset | cut timing, motion sync, SFX sync |
| `camera_motion` | global camera motion: pan / tilt / zoom / static | cut continuity, motion preset, stock query |
| `shot_scale` | wide / medium / close-up from the subject's area | cut rhythm, motion design, caption placement |
| `visual_sync` | sync score correlating audio onset with flow onset | sound design, cut timing |
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
specialised door exists.

| Model | Tier | Signal | Size | Licence | Door | Decoder | Spends |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `film-shot-classifier` | 1 | shot type: wide, medium, close, extreme close, insert, two-shot | 90 MB | Apache-2.0 | `film_shot_type` | `logits` | cut rhythm, motion design, stock (benched against the derived shot scale) |
| `movenet-a0` | 1 | temporal action class per window (600 Kinetics actions) | 15 MB | Apache-2.0 | `action_class` | `logits` | stock filter (reject the wrong action), Foley type, cut boundary, motion preset |
| `scenegraph-vlm` | 1 | subject-predicate-object graph per scene | 800 MB | CC-BY-4.0 | `scene_graph` | `logits` | stock query, caption context, action verification |
| `smolvlm2-500m` | 1 | bounded scene description: environment, objects, activity | 546 MB | Apache-2.0 | `describe_scene` | `logits` | stock query, motion style, ambience, caption context |
| `transnetv2` | 1 | shot-boundary probability per frame | 30 MB | MIT | `shot_boundaries` | `prob` | the cut stage: a boundary is where a cut lands, a pause only where it must |
| `vjepa2-vitl` | 1 | self-supervised video embedding + action head | 1130 MB | MIT | `video_embedding` | `embedding` | action class, video retrieval, stock similarity |
| `bytetrack-yolo` | 2 | persistent object tracks across frames | 15 MB | MIT | `detect_objects` | `boxes` | stock query by phrase, subject continuity (through `tracks`) |
| `depth-anything-v2-small` | 2 | per-pixel relative depth | 27 MB | Apache-2.0 | `depth_map` | `depth` | parallax motion, caption placement, cut seam, stock subject isolation |
| `fastsam-s` | 2 | pixel-accurate object masks | 40 MB | Apache-2.0 | `subject_mask` | `masks` | subject tracking, caption avoidance, motion overlays |
| `midas-v21-small` | 2 | monocular relative depth (the alternative to Depth Anything) | 80 MB | MIT | `depth_map` | `depth` | parallax motion, caption placement, cut seam, stock subject isolation |
| `segformer-b0` | 2 | semantic segmentation (150 ADE20K classes) | 15 MB | Apache-2.0 | `semantic_segments` | `segments` | scene continuity, caption contrast, stock filter |
| `clip-vit-b32` | 3 | image-text similarity embedding | 150 MB | MIT | `visual_embedding` | `embedding` | stock visual similarity, continuity, motion style |
| `dinov2-small` | 3 | 384-d visual embedding per frame | 84 MB | Apache-2.0 | `visual_embedding` | `embedding` | stock visual similarity, continuity, motion style |
| `raft-small` | 3 | learned optical flow (better on complex scenes) | 4 MB | BSD-3-Clause | `optical_flow` | `flow` | cut timing, motion sync (the learned flow, benched against the classical read) |
| `siglip2-base` | 3 | vision-language embedding (the CLIP successor to bench) | 400 MB | Apache-2.0 | `visual_embedding` | `embedding` | stock visual similarity, continuity, motion style |
| `dover-mobile` | 4 | aesthetic + technical quality score per video | 100 MB | Apache-2.0 | `aesthetic` | `scalar` | stock ranking's taste tiebreaker |
| `laion-aesthetic` | 4 | aesthetic score 0-10 per frame | 4 MB | MIT | `aesthetic` | `scalar` | stock ranking's taste tiebreaker |
| `smblurdetect` | 4 | motion blur probability / mask | 8 MB | Apache-2.0 | `blur_mask` | `masks` | stock filter, cut veto |
| `vinet-s` | 4 | video saliency map | 36 MB | Apache-2.0 | `video_saliency` | `depth` | cut timing, caption placement, motion attention |
| `eyegaze` | 5 | gaze pitch/yaw per face | 10 MB | research-only (Gaze360) | `gaze` | `keypoints` | caption placement, eyeline match - licence-blocked for commercial paths |
| `mediapipe-hands` | 5 | 21 hand landmarks + gesture class | 7 MB | Apache-2.0 | `hands` | `keypoints` | Foley, caption, motion overlay, stock query |
| `microexpnet` | 5 | emotion class per face | 900 KB | MIT | `face_emotion` | `logits` | cut (emotional beat), stock filter |
| `mobilegaze` | 5 | gaze pitch/yaw per face (mobile variant) | 5 MB | research-only (Gaze360) | `gaze` | `keypoints` | caption placement, eyeline match - licence-blocked for commercial paths |
| `movenet-pose` | 5 | 17 body keypoints per person | 5 MB | Apache-2.0 | `pose` | `keypoints` | cut (pose transition), Foley, caption, motion |
| `aqa-transformer` | 6 | how well an action was performed | 60 MB | research | `action_quality` | `scalar` | stock ranking, motion preset |
| `florence-2-base` | 6 | lightweight captioning / VQA | 230 MB | MIT | `describe_scene` | `logits` | stock query, motion style, ambience, caption context |
| `grounding-dino-tiny` | 6 | open-vocabulary object detection by text query | 628 MB | Apache-2.0 | `detect_objects` | `boxes` | stock query by phrase, subject continuity (through `tracks`) |
| `smolvlm2-256m` | 6 | the small VLM variant, for 4 GB machines | 256 MB | Apache-2.0 | `describe_scene` | `logits` | stock query, motion style, ambience, caption context |
| `syncnet` | 6 | lip-sync score | 20 MB | MIT | `lip_sync` | `scalar` | caption timing, talking-head cuts |
| `uform-gen2` | 6 | lightweight captioning / VQA (UForm) | 400 MB | Apache-2.0 | `describe_scene` | `logits` | stock query, motion style, ambience, caption context |

## The three rules a door follows

1. **Absent is an answer.** A door with no file, no `numpy` or no
   `onnxruntime` replies `ok: false` with the reason and the file it looked
   for - never a made-up reading and never a zero.
2. **The bounded schema.** `describe_scene` is the one free-text door, and it
   is asked for visible facts only: `docs/VLM.md` records a whole film
   hallucinated from a blue frame, and the prompt is written against that.
3. **A reading with no consumer is a bug.** Every reading a door produces
   names what spends it, in the table above and in `CONSUMERS` in the module,
   and `measure_clips` spends the classical ones on the stock choice the
   AUTO EDIT button makes.

## What the shelf does not do

It does not download anything on its own, it does not run a model the
operator did not install, and it does not claim a signal the classical layer
already produces. The bench in `sense_visual {action: "bench"}` is the
evidence a door has to beat before the edit starts spending it.

