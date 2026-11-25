# The evidence store — one place the run's facts live

Every reader the Director can call answers in its own shape: `voice_signals`
its profile, `sense_sound` its measurements, `sense_sound_model` its labels
and plan, `sense_visual` its sheet, `emotion` its arc, `analyze_beats` its
grid. The store is the discipline that makes them ONE record a revision can
still read after the tool replies are gone: the plan cites evidence, the
timeline executes it, the critic re-reads it, and a later session picks the
project up without re-running an inference the cache already paid for.

## Where it lives

Inside the project folder, beside the timeline it serves:

```
project/
  director/
    brief.json                  the creative brief, once it is agreed
    style-bible.json            the look and sound identity
    retention.json              hook, open loops, pattern breaks, payoff
    evidence/
      manifest.json             every record's id, source, hash, consumer
      voice-profile.json        the voice_signals extract (the acoustic score)
      discourse-profile.json    the script as language
      emotion-profile.json      the arc, per line and per act
      sound-library-index.json  the measured shelf index (sense_sound)
      sound-model-index.json    the semantic shelf index (YAMNet + CLAP)
      sound-design-plan.json    the per-moment plan (sense_sound_model plan)
      music-structure.json      the bed's sections, energy, cutSafePoints
      generated-audio.json      every generated asset and its birth record
      visual-candidate-reports.json  the measure_clips sheets, per slot
      visual-continuity.json    the manifest the continuity gate read
    plans/
      edit-plan.json            the canonical plan (the schema's own shape)
      audio-arrangement.json    soundEvents + musicArrangement, extracted
      motion-storyboard.json    the Geneva scenes and their timing
    execution/
      operation-receipts.json   every applied edit, by tool and arguments
      model-execution.json      which models ran, on what, for how long
      revision-history.json     every defect, its fix, its re-inspection
    quality/
      visual-critique.json      the watch_render reads, per section
      audio-critique.json       check_placement and measure_loudness results
      final-acceptance.json     the gates, each passed or refused, at the end
```

Write a file when its evidence exists; never pre-create empty ones. A file
that would hold one unmeasured reading stays absent, and the report says so.

## The common record

Every evidence record, whatever reader produced it, carries the same spine —
so a consumer that knows how to read one knows how to read them all:

```json
{
  "assetId": "stock_014",
  "analysisId": "visual_014_v1",
  "modelId": "movinet-a0",
  "modelVersion": "the version the reply carried",
  "reading": "action_class",
  "time": { "start": 12.4, "end": 14.8 },
  "value": { "action": "running" },
  "confidence": 0.94,
  "status": "measured",
  "consumer": ["shot_selection", "shot_timing"],
  "evidenceHash": "sha256:…"
}
```

- `assetId` — what was read (a file, a clip, a span of the take).
- `modelId` / `modelVersion` — which reader, which version; `null` for the
  classical measurements.
- `reading` — the reading's own name, the catalogue's identifier.
- `time` — where in the asset the reading falls; point readings carry
  `start` only.
- `confidence` — the reader's own number, never invented.
- `status` — `measured`, `absent` (the model was missing), or `na`
  (the reading cannot apply: no face in a landscape shot). An `absent`
  or `na` record is HONEST and required — a missing record is a lie of
  omission, and a run that claims full intelligence with absent models
  says so here.
- `consumer` — the decisions this record feeds. A record with no consumer
  is reporting, not editing; do not store it.
- `evidenceHash` — the reply's own digest, so a later revision can prove
  the decision it reconsiders stood on this evidence and not a newer one.

## The rules

1. **Cite, never copy.** The plan and the store reference each other by id
   and hash; thousands of model values never ride inline in a plan the
   editor must re-read.
2. **A consumer for every record.** Evidence no decision reads is a
   dashboard; the store is not a dashboard.
3. **Absent and `na` are records too.** The honest store names what was not
   measured and why — that is what makes a `measured` record worth trusting.
4. **Generated assets carry their birth.** Prompt, seed, kind, licence —
   a review must be able to answer where a sound came from and whether it
   may ship.
5. **The store outlives the session.** Phase 23's hand-off points at it;
   the next session reads the store before it re-measures anything, and
   the cache (keyed by source hash and model version) is what makes that
   cheap instead of lazy.

## What the store is for

- **The plan** reads evidence by id; `plan_validate`'s `signalLog` holds the
  citations, and the audiovisual slots (`soundEvents`, `musicArrangement`)
  cite the store's records the same way.
- **The critic** re-reads the store instead of trusting the plan's summary:
  the acceptance gates compare what the timeline shows against what the
  evidence said it would.
- **The revision** finds the record behind a defect, decides whether the
  decision or the evidence was wrong, and records which in
  `revision-history.json`.
- **The hand-off** ships the store with the project: a complete, auditable
  account of why every clip, sound, caption and cut is where it is.
