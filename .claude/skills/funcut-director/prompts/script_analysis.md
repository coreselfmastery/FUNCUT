# Script analysis

Turn a topic (or a rough script) into narration that survives the first three
seconds. You are writing for a VOICE, not a page.

## Duration math first

- 180–200 words per minute → **words ≈ seconds × 3.1**. A 30 s video is
  ~90–95 words; 60 s ≈ 190 words; 90 s ≈ 285.
- Short-form sweet spots (from 2.4M-TikTok research): **16–30 s earns the best
  median views**. Defaults: explainer 30 s, story/POV 30–65 s, listicle 45 s.
- Hook budget: the first sentence must land inside 3 s (~9 words), because
  the scroll decision is made around 1.7 s.

## Hook patterns (pick one, make it concrete)

1. **Bold claim** — "The ocean is a mountain, dissolved."
2. **Myth break** — "Everything you know about X is backwards, and 3 numbers prove it."
3. **Stakes** — "This mistake costs creators their channel."
4. **Question with a wrong guess** — "Why do all airports look the same? It's not laziness."
5. **List tease** — "Three foods your grandmother trusted that science just confirmed."
6. **POV drop** — "You're standing on the salt of a mountain that eroded before animals existed."

Rules: name a concrete noun in the first five words; numbers beat adjectives
("4 billion years" not "a very long time"); no greeting, no "in this video".

## Beat sheet (short-form, 30 s)

| Beat | Span | Job |
| --- | --- | --- |
| Hook | 0–3 s | One sentence, one image, one SFX |
| Setup | 3–8 s | The one fact/frame the hook needs to make sense |
| Escalation 1 | 8–15 s | The strongest proof, a number, a comparison |
| Escalation 2 | 15–22 s | The twist, the scale-up, the "and then" |
| Payoff + loop | 22–30 s | Close the loop the hook opened; one-line CTA that reuses a hook word |

Longer videos repeat escalations (each 6–8 s, harder proof each time), one
pattern interrupt every 2–4 s of runtime (see `cuts_and_pacing.md`).

## Write for TTS

- Short sentences. One idea each. Full stops are your pacing tool — the
  silence-removal pass keeps sentence-final breaths.
- Spell out numbers and units ("four billion" not "4B"); expand every
  abbreviation; no URLs, no hashtags in narration.
- Punctuation is stage direction: a comma = small pause, an em dash = beat,
  "..." = deliberate hang. Avoid ALL-CAPS (TTS shouts unpredictably) — put
  emphasis in word choice, not formatting.
- Read-aloud test: every sentence under ~12 words, mouth-friendly words, no
  tongue-twisters where the beat is tight.

## Segmenting for the plan

After `transcribe` you have `{segments:[{start,end,text}]}`. Group them into
plan segments at natural beats (sentence ends), 2–5 s each: a segment is the
smallest unit that gets ONE visual. If a sentence runs past 5 s, split its
visual into two moves (e.g. Ken Burns into punch zoom) rather than letting one
clip sit static.

## Loop endings

The last line should hand back to the first image/idea ("So the next time you
taste the sea, taste the mountain"). Loops raise rewatch rate; a CTA earns its
2 s only if the loop is intact ("Follow for one fact like this a day" —
'fact' echoes the hook's register).

## Checklist before you lock the script

- [ ] Hook is one sentence, ≤9 words, lands in ≤3 s, has a concrete noun.
- [ ] Runtime matches the brief (words ≈ seconds × 3.1), ±10%.
- [ ] Every escalation is either a number, a name, or a comparison.
- [ ] Reads aloud without stumbling; no abbreviations or numerals.
- [ ] Last line loops to the opening image; CTA ≤ one short sentence.
