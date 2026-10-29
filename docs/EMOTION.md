# The emotion-driven edit

FUNCUT's director pipeline had eyes (`watch_render`, `craft_critique` — it
can **see** the cut) and hands (the edit tools — it can **change** the cut).
What drove an edit was structure alone: sections, pacing floors, variety
laws. The `emotion` tool adds the missing driver — a measured read of what
the voiceover **feels like**, line by line, and a set of laws that make
every layer answer to that read.

```
voiceover ──transcribe──> words with measured times
        ──emotion──> per line: emotion, intensity, act, hormone
                             easing curve, colour temperature, SFX register,
                             duck depth, TTS delivery, pacing, stock queries
        ──emotion──> the arc: shape, score, warnings, brief-beat match
```

The law in one line: **tense lines ease in, warm lines ease out, a burst
pops, anger snaps** — and everything else (colour, sound, duck, vocabulary)
follows the same read.

## The two engines (with and without a key — both first-class)

| engine | when | confidence | what it sees |
| --- | --- | --- | --- |
| `lexicon` | always | 0.55 | a curated stem lexicon over the 7 edit labels, negation windows, amplifiers, hedges, caps and punctuation intensity — **English-only** |
| `model+lexicon` | `FUNCUT_EMOTION_API_KEY` or `HF_TOKEN` set | 0.85 | `tabularisai/multilingual-emotion-classification` (XLM-RoBERTa, **23 languages**, 11 labels folded onto the edit's 7) on HuggingFace's free Inference API (75% weight) blended with the lexicon (25%) |

The hosted model reads the narration in whatever language it was written
in — English, Bengali, Hindi, Urdu, Arabic, Chinese, Spanish and eighteen
more — where the old English-only classifier saw gibberish and the lexicon
sees nothing at all. Its eleven labels fold onto the edit's seven:
contempt rides the disgust grade, frustration the anger cut, gratitude
and love the joy lift (love at calm arousal lands on oxytocin, which is
the state the edit wants anyway). The lexicon keeps a quarter of the
weight because it sees punctuation, caps and idioms the model never will.
When the hosted call fails — no network, refused key, wrong shape — the
reply says so and the lexicon answer stands, labelled. The pipeline never
**requires** the key: every phase below runs at either confidence. A
non-English script *without* a key is answered with an honest note — the
lexicon is English-only — rather than a silent run of neutral lines.

The key resolution order: the `apiKey` argument, `FUNCUT_EMOTION_API_KEY`,
`HF_TOKEN`, `HUGGINGFACE_API_KEY`. The reply masks it (`hf_...xY`). The
`model` argument (or `FUNCUT_EMOTION_MODEL`) overrides the default model
id. Long transcripts travel in batches of 64 lines per request. Nothing
is stored on disk by this tool — the key travels the existing
stdin pipe, never argv, and appears in no log.

The "hormone" field (adrenaline, cortisol, dopamine, serotonin, oxytocin,
endorphin) is a creative **shorthand** for the state a beat puts the viewer
in — a mnemonic for pacing and sound decisions, not physiology. The docs
say so because a field named like science will be trusted like science.

## The pipeline, end to end

The nine stages a voiceover-driven edit walks, and the tool that owns each:

| stage | what happens | the tool |
| --- | --- | --- |
| 1. Voiceover | words with **measured** times | `transcribe` (or `tts_generate` with a `delivery`) |
| 2. Emotion read | per-line emotion, act, demands | `emotion {action: "extract", transcript, project}` |
| 3. Brand guide | the channel's non-negotiables | `emotion {action: "template"}` → `creative_brief {action:"set", brief:{brandKit}}` |
| 4. Edit plan | emotion-constrained segments | `plan_validate` (per-segment `emotion`, `act`, `easing`, `colourTemp`, `duckDb`) |
| 5a. Stock | emotion+act query vocabulary, then the gate | `openvid_search` with the extract's `queries` → `stock_contact_sheet` → `emotion {action: "gate"}` |
| 5b. Motion | easing from the emotion, MAESTRO's judgment | `motion_inspect` → `motion_frame` → `motion_bake` (curve = the extract's authoring JSON) |
| 5c. Sound | SFX peaks ON cuts, BGM ducked by act | `sounds_search` (`syncAt` → plan `peakAt`) → `sounds_add`; `clip_audio_levels_db` to each segment's `duckDb` |
| 6. Placement gate | 2+ failed checks rejects; all rejected drops the beat | `emotion {action: "gate", segment, candidates, brand}` |
| 7. Grade + transitions | per-act colour temperature inside ONE look | `effect_layer_add` once; transitions via `clip_update` (never `effect_layer_add`) |
| 8. Critic | six-axis score, measured not felt | `watch_render` → `craft_critique` |
| 9. Export + verify | duration, LUFS, true peak AND the words themselves | `export_wait` → `measure_loudness` → `transcribe` the file → `emotion {action: "verify"}` |

Stages 1–3 shape the script; 4–7 shape the edit; 8–9 prove it. Every format
rides the same read — long, short, horizontal and vertical differ in canvas
and pacing, not in what the narration demands.

## What `extract` returns

Per line: `emotion` (one of anger, disgust, fear, joy, neutral, sadness,
surprise), `intensity` (0–1, from the win margin plus caps/bangs/amplifiers,
minus hedges and calm words), `act` (hook, setup, tension, turn, reveal,
proof, cta — from the plan's sections when given, then explicit markers,
then position, then emotional U-turns), `hormone`, `scores` (the full
distribution), `valence` and `arousal` (circumplex coordinates, arousal
cooled by calm words), the TTS `delivery` + `speed` that would read the
line, `easing` (a name a person reads AND the exact authoring JSON
concat-motion's `curve` field takes), `colourTemp` (a temperature + a
grade hint), `sfxType`, `duckDb` (the act's BGM duck depth), `pacing`, and
`queries` (2–3 words: the emotion's visual register × the act's shot).

Plus the arc: `arcShape` (flat / rise / fall / dip-rise / rollercoaster /
drift), the valence and arousal curves, `distinctEmotions`, `longestRun`,
`briefBeatsMatched` (did the lines visit the beats the brief's
`emotionalArc` promised), `arcScore` (0–10) and `warnings` that name the
fix — a flat script wants a rewritten or re-delivered section, a missing
reveal wants a "turns out" moment, a low-arousal opening wants a hook with
more voltage. **Fix the script before planning; an edit cannot rescue a
narration that never changes state.**

## The brand guide

One channel's non-negotiables, validated and enforced:

```json
{
  "id": "science-shorts",
  "name": "Science Shorts",
  "vibe": "bright conspiratorial science: curious, fast, never cynical",
  "palette": { "background": "#0B1220", "ink": "#F5F7FA", "accent": "#4CC9F0" },
  "type":     { "family": "Archivo", "weight": 700 },
  "motion":   { "easing": "auto" },
  "grade":    { "base": "concat.teal-orange", "amount": 25 },
  "sfx":      { "palette": ["impact-*", "whoosh-*", "riser-*"] },
  "bgm":      { "genres": ["ambient electronic"], "duckDb": -12 },
  "taboos":   ["lens flare", "beaker stock footage", "flame particle"],
  "voice":    { "reference": "curious, assured, warm; never shouty" }
}
```

`motion.easing: "auto"` (the default posture) means the emotion decides the
curve; a fixed name overrides **neutral** beats only — the craft law
(tense eases in, warm eases out) is not a preference the brand gets to
break. `grade` is the base look; the per-act colour temperature pushes on
top of it as intensity variation, never as a second filter. Store the guide
as `brandKit` via `creative_brief` and `emotion {project}` picks it up
automatically; pass it as `brand` to apply it to lines, or to the gate.

A **taboo** in any planned query or caption is a blocker: replace the
query, never the taboo.

## The placement gate (a hard floor)

Every stock candidate for a beat faces three checks:

1. **emotion** — does the caption/keywords carry this beat's emotional
   visual register? (fear wants dark/fog/night/alone, joy wants
   sunlight/celebration/golden)
2. **act** — does it carry the act's shot semantics? (hook wants extreme
   close-up / striking high-contrast, setup wants wide establishing,
   reveal wants bright clear hero)
3. **brand** — is it free of the guide's taboos?

**0–1 failed checks passes; 2+ rejects the candidate; when every candidate
is rejected, DROP THE BEAT** — cut the segment and tighten around it.
Padding a beat with a bad match is worse than a shorter video. The gate is
lexical (it reads captions and keywords); the pixels were the contact
sheet's job — run both, in that order. A rejected candidate is never
"close enough".

## The export verify

`export_wait` proving a file exists is not proof the edit survived the
encode. `watch_render {path}` proves the pictures; `measure_loudness`
proves the loudness; `emotion {action: "verify"}` proves the **speech**:
`transcribe` the exported file, align its words against the edit's word
timings, and pass requires matchRate ≥ 0.9, worst drift within
`toleranceMs` (default 150), and no duplicated words. A failure names
clipped audio (check clip ends and trims), shifted audio (a clip boundary
or speed change moved it) or duplicated audio (un-trimmed overlap) —
defects the eye cannot see and no score would catch.

## Where the laws live

- The director's contract: `.claude/skills/funcut-director/SKILL.md` —
  Phase 1b (the read), §3.E (the gate floor), Phase 3 (the plan's
  per-segment emotion fields), Phase 8 (the verify).
- The plan schema: `.claude/skills/funcut-director/templates/edit_plan.schema.json`
  — `emotion`, `act`, `easing`, `colourTemp`, `duckDb` per segment,
  validated when present, optional so non-voiceover edits still pass.
- The engines: `src/crates/concat-mcp/sense/emotion.py` (embedded in the
  binary via `include_str!`, shipped with it, selftested in CI).
- The MCP surface: the `emotion` tool (this document's subject) and the
  creative loop in the server instructions that orders it among the
  other tools.
