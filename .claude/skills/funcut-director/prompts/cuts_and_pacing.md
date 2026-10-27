# Cuts and pacing

The cut is the sentence; pacing is the paragraph. A piece that only
hard-cuts is a slideshow, and a piece that cuts on every frame is a strobe.
This is the vocabulary between them.

## The twelve cut types (the plan's `cutType`)

| Cut | What it is | Use it for |
| --- | --- | --- |
| `hard` | the plain cut | the default; most cuts, everywhere |
| `match` | cut on the same shape/color/action | invisible flow between subjects; the pro signature |
| `jump` | deliberate same-subject displacement | time passing, unease, "then vs now" |
| `smash` | hard cut against sound: silence→impact or soft→loud | the claim lands; the number hits |
| `j` | audio of the next scene leads its picture | conversations, reveals — the word before the visual |
| `l` | audio of the current scene plays over the next | exits, moving on |
| `whip` | transition-packet cut (`concat.whip-pan`) | fast list rhythm, reels on the beat |
| `montage` | rapid sequence of very short clips | escalation, accumulation, "and again and again" |
| `action` | cut mid-movement, land on the completion | any physical motion; sport, process |
| `beat` | cut exactly on a downbeat (`analyze_beats`) | music-driven sections, reels |
| `flash` | 1-frame white/flash frame between clips | glitch energy, tech error beats — sparingly |
| `speed` | speed-ramp through the cut (`setClipSpeedCurve`) | dramatic emphasis: slow into the moment, whip out |

`montage` is not reserved for the finale any more than a period is
reserved for the last sentence — one montage per 20 s is fine in
energetic pieces. In the plan, each segment names its cut; a run of six
`hard`s in a row is a pacing smell to fix at review.

## Pacing curves

Cuts per minute by register — and the curve WITHIN a piece:

- calm 6–10 (documentary, history)
- standard 10–16 (explainer, how-to)
- energetic 16–24 (tech, finance, motivation)
- hype 24+ (gaming, extreme)

Longform breathes on a curve: hook at the top of your register → body a
notch calmer → re-hook spike at the 50–60% dip → outro settles. A flat
cpm across eight minutes is monotone even when the number is "right".

## The 4-second law

No visual segment exceeds **4.0 s** (energetic niches) or **6.0 s**
(calml), enforced by the plan validator. This is the "stock footage
doesn't change" complaint as a number: a 9-second aerial is nine seconds
of the viewer's attention walking out. Within one long clip you can also
CUT INTO IT — place the same source as two clips with different trims and
different framings (that counts as two segments and two shots; the reuse
law is about consecutive identical treatments, and the same source used
at 0:10 and 2:40 is fine).

## J-cut feel inside contiguous segments

The plan's segments are contiguous — the validator rejects gaps. The
J-cut *feel* is created inside the placement: lead the visual 100–200 ms
before the narration's word by trimming the clip's start earlier — never
by leaving gaps between segments.

## Silence removal parameters

`analyze_silence` defaults: `-35` dB threshold, ≥0.5 s spans, and a
`paddingMs` that is REPORTED, NOT APPLIED. Shrink each span by ~80 ms per
side yourself before `clip_cut_spans` — the breath at a sentence end is
performance, not dead air. When in doubt cut less: a tight video that
breathes beats a suffocated one.

## The self-audit at review

Count the cuts (`timeline_read`: clip count on the picture lane, minus
1). cuts/minute = count × 60 / runtime. It should land in your register
band, and the variety of `cutType`s used should be ≥ 4 across a piece.
Then ask: did any single cut surprise me? If nothing did, the edit is
smooth and forgettable — add one `smash` or `speed` where the script
makes its biggest claim.
