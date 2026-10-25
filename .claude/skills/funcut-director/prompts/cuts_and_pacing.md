# Cuts and pacing

The cut is the sentence of video. Pacing is set by the SCRIPT'S energy and
the niche's playbook, not by vibes — measure it: cuts per minute = segments
per minute from your plan.

## Pacing targets (cuts/minute)

| Register | cpm | Niches |
| --- | --- | --- |
| Calm | 6–10 | history, documentary, horror build-ups |
| Standard | 10–16 | explainer, how-to, science |
| Energetic | 16–24 | tech, finance, motivation, fitness |
| Hype | 24–40 | gaming, extreme, meme-adjacent |

Short-form retention adds a hard rule regardless of register: **a pattern
interrupt every 2–4 s** — a cut, a text pop, an SFX hit, a punch zoom, a
camera move. Two consecutive interrupts of the SAME kind feel like flicker;
alternate cut → text → zoom → SFX. 20–40 cpm in pure short-form is normal.

## The cut types and when each is right

- **Hard cut (default).** Same place/continuity continuing → nothing fancy.
- **Match cut.** Shape, motion or idea carries across the cut (a turning
  wheel → a turning planet). The strongest cut in a faceless edit; plan one
  per video minimum in the escalation beats.
- **Jump cut.** Time compression inside one subject. In stock-B-roll work its
  equivalent is: same subject, tighter shot.
- **Smash cut.** Loud/quiet or fast/slow juxtaposition — the horror sting,
  the joke landing. Needs both sides planned (loud riser into near-silence).
- **J / L feel.** AUDIO crosses the cut, picture changes at the word: the
  next segment's SFX or narration enters up to 150–200 ms before its visual.
  In this engine: `sounds_add` the next beat's SFX peaked at the boundary
  while the previous clip still shows, and let `clip_trim` land the visual on
  the word. The L feel (audio lingers) comes from `detachAudio`-style
  overlaps via raw `edit_apply` when you truly need it — rare in faceless
  stock edits.
- **Montage.** 3–6 clips × 0.4–0.8 s over one rising sentence — use
  `clips_add_sequence` then trim each. Reserved for escalation 2 or the
  payoff.

## Silence removal (the spine pass)

`analyze_silence` → `clip_cut_spans` on the voiceover, then the plan segments
inherit the tightened timeline:

- threshold −35 dB (−40 for noisy rooms, −30 if the VO is a hot whisper),
- spans ≥ 0.5 s (0.35 s in hype pacing),
- padding ≈ 80 ms at each edge (keeps the plosive, kills the dead air).

After the cut, re-listen via the transcript timings: sentence-final gaps
should be short but PRESENT (that is the breath the ducking rises into);
mid-sentence gaps should be gone. If the VO now races, reduce rather than
delete: `clip_speed` at 1.05–1.1 on the filler is calmer than cutting a
breath the TTS needed.

## Ordering discipline

- Cut TO the word, not after it: the visual for a new idea enters ON (or
  100–200 ms before) its first word — late visuals read as lag.
- A 1-frame late SFX is invisible; a 10-frame late visual is amateur hour.
  When in doubt, trim visuals 1 frame early, never late.
- End on purpose: the last frame holds 0.5–1 s longer than feels natural
  before the loop point — it is the thumbnail and the loop's breathing room.

## Self-check

- [ ] cpm matches the register table (count plan segments × 60 / runtime).
- [ ] No still visual > 4 s without an interrupt (zoom, SFX, text pop).
- [ ] No two interrupts of the same kind back to back.
- [ ] At least one match cut or smash cut in escalations.
- [ ] Silence pass kept breaths at sentence ends; no mid-word cuts.
- [ ] Every visual enters on or before its word.
