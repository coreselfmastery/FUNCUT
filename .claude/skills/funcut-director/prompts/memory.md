# The editing journal (memory)

The journal is how an editing session knows what it has already done —
first, middle and last — without guessing and without re-reading a
300-clip timeline. It is one file, `memory.md`, in the project folder, and
it is part of the deliverable: a human opening the project later can read
what was decided and why.

## When to write

After **every** `edit_apply` batch — one line, appended, never rewritten:

```markdown
- 00:12-00:19 | added broll "trading floor" clip 901824 fill | whip-pan in, fade out | whoosh-swish-2 @0:12 -8dB | grade: teal-orange 70 (layer, full runtime)
- 00:19-00:23 | motion comp funcut.stats.triptych "58% / 42%" baked 1080x1920, imported, placed @0:19 | pop in | data-countup-medium @0:19 -10dB
- 00:23-00:31 | captions_add segments 14-19, offsetY 0.35 | — | — | note: segment 17 text fixed from transcript
```

The shape: time range | what entered or left | entrance/exit | sound |
grade/note. Keep it to one line — the journal is an index, not a diary.

## The thirds recap

Every time you write the journal, you should be able to answer, from it:

- What does the FIRST third contain — hook? which comps? which SFX ids?
- What does the MIDDLE third contain — the body chapters, the re-hook?
- What does the LAST third contain — CTA, outro card, loop ending?

If you cannot say what a third contains in one sentence, you have lost the
thread of your own edit — re-read `timeline_read` and write the recap
before placing anything else.

## When to re-read

- Before every placement phase (the journal says which lanes are taken and
  which SFX ids are spent).
- Before the SFX pass (the anti-repetition law: no id twice within 30 s —
  the journal is how you check without guessing).
- At review (Phase 7): the recap above is a checklist item.
- After any `edit_undo` (write what came back out).

## What the journal is not

It is not a log of tool calls, and it is not a plan. The plan looks
forward; the journal looks backward. Both live in the project folder; the
plan as `plan.json`, the journal as `memory.md`, and neither replaces
`timeline_read` when the truth of the timeline is the question.
