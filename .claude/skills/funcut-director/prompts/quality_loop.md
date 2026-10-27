# The quality loop — watch, score, revise

Compliance is not craft. `plan_validate` proves the plan has no gaps; it
cannot prove the video is worth watching. The only reliable mechanism is a
short closed loop with real pixels in it:

```
edit -> watch_render -> craft_critique -> revise -> watch_render -> compare
```

## 1. Watch the whole cut, not one frame

`watch_render {project, samples: 12}` returns ONE MCP `type: image` block: a
labelled contact sheet sampled evenly across the entire timeline. Each tile
is numbered with its timestamp. Look at it in order like a film strip.

What the JSON beside the image tells you:

| Field | Meaning | What to do |
| --- | --- | --- |
| `craft.blackFrames` | near-black samples | trim the lead-in or add a visual beat |
| `craft.letterboxedFrames` | bars detected | `clip_transform` to reframe; never ship bars |
| `craft.flatFrames` | very low contrast | new shot, grade, or motion |
| `craft.duplicateFrames` | perceptual-hash matches | replace the repeated shot |
| `craft.staticTransitions` | consecutive near-identical samples | entrance, push-in, or new shot |
| `craft.distinctShots` | unique images | should be close to `samples` |
| `tiles[].cropHint` | energy centroid, -1..1 | offset the 9:16 crop with `clip_transform` |
| `audio.envelope` | 48-bucket loudness | look for long dead air or a flat mix |

`watch_render {path: "<exported file>", audio: true}` does the same on the
encoded movie. Use it in Phase 8 — the encoded file can differ from the
preview.

Pass `times: [...]` when you want specific moments (the hook, the reveal,
the last frame) rather than a uniform sample. Keep `samples` between 8 and
16; a 24-tile sheet is hard to read.

## 2. Score it

`craft_critique {sheet: <the watch_render JSON>, brief, project}` returns:

```
engine: "local-vlm" | "heuristic"
scores: { hook, story, rhythm, shotChoice, design, sound }   // 0-10
overall, confidence, verdict, notes[], revisions[]
```

- `engine: "heuristic"` means no vision model was running. The scores are
  honest measurements, explicitly capped at `confidence: 0.45`. Do not
  treat a heuristic 8 as a human 8.
- `engine: "local-vlm-flags"` means the bundled 0.5B critic looked at the
  sheet and flagged **visible defects only** (near-black frames, letterbox
  bars, readable text). It adjusts the measured score by at most 1 point
  per axis. A 0.5B model CANNOT judge taste: in testing it hallucinated an
  entire film from a plain blue frame. Treat its `observations.description`
  as unverified and look at the sheet yourself.
- `engine: "local-vlm"` means a **stronger** model from
  `FUNCUT_VLM_ENDPOINT` supplied six-axis scores (Ollama, LM Studio or any
  OpenAI-compatible vision server). This is the only path where the model
  grades craft; degenerate responses fall back to the flag detector.
- Set the bundled one up with `local_vlm {action:"setup", profile:"mini"}`
  — about 546 MB into the app data dir, CPU-only, no system install.
- `revisions[]` name concrete tools. Execute at least one before exporting.

## 3. Read the six axes like an editor

- **hook** — do frames 1-2 create a question or a feeling? A title card
  restating the topic scores 3, not 8.
- **story** — is there visible progression? If you shuffle the middle tiles
  and nothing changes, there is no arc.
- **rhythm** — do cut intervals vary with the energy? A perfectly regular
  grid reads as a slideshow.
- **shotChoice** — are the images *meaningful*, varied and well framed?
  Generic pretty footage that says nothing is filler.
- **design** — do graphics add information (a number, a comparison, a
  label) or just decorate?
- **sound** — the audio envelope should rise and fall with the story. Long
  silence or a flat bed is a defect.

Score 5 = competent but generic. 7 = genuinely good. 9 = you would put it
in a portfolio. **Below 7 is not ready.**

## 4. Revise, then compare

Pick the highest-leverage revision, not the easiest one. Typical fixes:

| Symptom | Tool |
| --- | --- |
| black / letterboxed tiles | `trim_clip`, `clip_transform` |
| static run | `set_entrance`, `add_keyframes`, `clip_transform` |
| repeated images | `stock_contact_sheet {rank:true}` then `clip_replace` |
| weak hook | rewrite the first line, `captions_animate`, new first shot |
| flat design | `motion_inspect` -> `motion_frame` -> `motion_bake` |
| dead soundtrack | `sounds_search`, `bgm_search`, `clip_audio_levels_db` |
| no point of view | `creative_brief {action:"set"}` then re-plan |

Call `watch_render` again with the same `samples`, then
`craft_critique {sheet, brief, project}`. Because `project` is given, every
pass is written to `craft-history.json` and the reply carries `pass`,
`previousOverall`, `delta`, `improved` and a `trend` sentence. Read them:
`delta <= 0` means your revision did not help (or hurt) - undo it or pick a
different one. Only claim improvement when `improved` is true and the image
actually changed. A run that never improves across two passes must stop and
report the honest score, not loop forever.

## 5. Be honest in the report

Report the score, the engine that produced it, what you changed between
passes, and what you did not fix. "The edit is perfect" is not a finding;
"hook 6 -> 8 after replacing the opening title card, shotChoice still 6
because only four distinct sources were available" is.
