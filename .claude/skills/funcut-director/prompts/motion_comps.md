# Motion comps — the design layer, in a director's plan

Read this when the edit needs **design**, not just footage: a lower third, word-timed captions, a stat
card, a chart building on, a logo sting, a subscribe card, a backdrop, a before/after split. Design is
what makes a cut feel authored; footage, grades and transitions cannot fake it.

The mechanical reference — every operation, every curve, every preset, the CLI — is the **`motion`
skill** (`.claude/skills/motion/SKILL.md`) and `docs/MOTION.md`. This page is how comps fit into an
edit: when to reach for them, what to say, and how to check.

---

## 1. Reach for comps in this order

1. **A library preset that is already close.** Twelve exist; filling a slot is three fields and the
   result will look designed.
2. **A preset plus operations** — retime it, re-word it, slide it, re-colour it, delete what this edit
   does not need.
3. **Author from primitives** (`add-group`, `add-rect`, `add-text`, `set-track`) only when nothing is
   close. Hand-built shapes look hand-built; prefer markup.

Never rebuild in primitives what a preset already does. Never place a full-frame opaque backdrop over
footage unless you mean to replace the picture.

---

## 2. The loop

```
inspect  →  one sentence of intent  →  one edit batch  →  three frames  →  bake at cut size  →  place
```

- **inspect** first, always — node ids and slot labels come from the document, not from memory. Every
  operation that names a missing node or slot is refused, and says which.
- **intent**: one sentence per comp — *"name card slides in at 3.5 s over 0.7 s on a spring, out at
  7.2 s"*. Two comps competing for one corner is the usual amateur tell.
- **one batch**: all-or-nothing and validated, so a half-built comp cannot reach the timeline.
- **three frames**, not thirty: mid-entrance, settled, mid-exit.
- **bake** at the size you cut at.
- **place** the clip as `kind: "motion"` with `comp`, `hash`, `engine`.

---

## 3. Where comps belong in the pipeline

| Phase | What a comp does |
| --- | --- |
| hook (first 2 s) | the wordmark sting, or one number popping on a downbeat |
| chapters | one chip per section, landing on the downbeat (`analysis.beats`) |
| talking head | lower third after the first full sentence — never on the cut |
| B-roll | kinetic captions at the bottom third, one comp per 4–7 words |
| data | chart bars or a counting number, one figure per shot, revealed not dumped |
| CTA / outro | subscribe card, held long enough to read twice |

A comp that repeats every few seconds is a rhythm, not a template: keep the same entrance curve and
stagger (60–120 ms) across all of them so they read as one system.

---

## 4. Three worked plans

### 4a. Transcript → word-timed captions

```
1. transcribe → words with start/end
2. inspect funcut.caption.kinetic → node "caption", slot "Transcript line"
3. one comp clip per 4–7 words, or per sentence:
   {"op":"set-words","node":"caption","words":[[w,s,e], …]}
   - two-element [word, start] holds until the next word
   - a null end on the last word holds to the end of the comp
4. frame at the middle of the longest word — the highlight must be on it
5. bake, place bottom third, 10% margin, clear of faces
```

2.5–3.5 words/second reads; past 4 it does not. Cue-level transcripts: split each cue evenly by
character count — never invent timings that drift from the audio.

### 4b. A lower third over an interview

```
1. inspect funcut.lower-third.glass → slot "Name", node "card"
2. edit, one batch:
   set-slot  Name = "Ada Lovelace"     ← the slot targets the sample text; set-text is for
                                         plain text nodes and is refused on the card's markup
   move      card → start 3.5          ← after the first full sentence, not on the cut
   retime    card → 4.0                ← long enough to read the name twice
   fade-in   card 0.5 s EMPHASIS
   slide-to  card x=… y=… 0.7 s
3. frame at 4.2 s and 7.4 s — readable, and clear of the lower-third-safe area?
```

### 4c. Beat-driven stat reveal

```
1. analysis.beats → downbeats
2. one stat card per 4 beats; cards sharing a row stagger 90 ms apart
3. set-track on each card's "scale": [[0, 0.9, SPRING], [0.5, 1.0, null]]
4. bars grow with STEPS when a number counts, EMPHASIS when it is a single figure
```

---

## 5. Verification

- Three frames per comp: mid-entrance, settled, mid-exit. A comp that is right at 2 s and wrong at
  6 s is a bug you have not seen yet.
- `needs_markup: true` needs the Geneva sidecar; without it the comp bakes on `native` **without the
  markup nodes** — read the bake manifest's warnings rather than trusting the picture.
- A comp that renders empty is either a transparent background (correct for an overlay) or a node
  whose `start` is later than the frame you looked at.
- Backdrops are the expensive bake (full-frame gradients). Bake once at cut size and reuse.
