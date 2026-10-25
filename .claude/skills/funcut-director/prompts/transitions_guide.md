# Transitions guide

The engine ships 22 transition packages (`effects_list {kind:"transition"}`
is the runtime truth). A transition must MEAN something — direction, time
passing, a world change. If nothing changed, use a cut. In a 30 s video, at
most 2–4 transitions.

## The 22, and what each says

| Transition | Says | Use for | Typical |
| --- | --- | --- | --- |
| `concat.dissolve` | time passes, soft refocus | section changes in calm/standard edits | 0.5 s |
| `concat.blur-dissolve` | a dreamy/epic beat | science reveals, nostalgia | 0.5 s |
| `concat.fade-black` | a chapter closes | mid-video section breaks only; NEVER at the start of a short (retention drops) | 0.4 s |
| `concat.fade-white` | a hard beat lands | stingers, punchlines | 0.2–0.3 s |
| `concat.film-burn` | vintage jump | history, retro | 0.4 s |
| `concat.light-leak-flash` | a burst of light | upbeat/vlog beats, luxury sparkle | 0.3 s |
| `concat.light-leak-transition` | gentle light wash | luxury, soft section changes | 0.6 s |
| `concat.whip-pan` | the camera moved on | energetic same-scene jumps, tech pacing | 0.3 s |
| `concat.push` | we advance | list steps, forward motion | 0.4 s |
| `concat.slide` | parallel move | listicle B-roll | 0.3 s |
| `concat.spin` | playful spin | meme-adjacent, comedy | 0.3 s |
| `concat.zoom-blur-transition` | dive in | going from context to detail | 0.4 s |
| `concat.zoom-transition` | punch in | scale reveal ("and it costs THIS much") | 0.3 s |
| `concat.iris` | focus on one spot | history artifacts, reveal-the-object | 0.5 s |
| `concat.clock-wipe` | time passing, stylized | history, countdown | 0.5 s |
| `concat.linear-wipe` | clean sweep | corporate/finance steps | 0.4 s |
| `concat.shape-wipe` | stylized sweep | gaming, kids content | 0.4 s |
| `concat.rgb-split-wipe` | glitchy energy | tech/glitch aesthetics | 0.3 s |
| `concat.glitch-transition` | a hard digital break | tech reveals, hype resets | 0.3 s |
| `concat.glitch-warp` | heavier digital break | horror-tech, extreme | 0.3 s |
| `concat.swirl` | a twist | surreal transitions, comedy | 0.4 s |
| `concat.cube` | a 3D flip | listicle items, splits | 0.4 s |

## Choosing in 10 seconds

1. Is it a SECTION change (new idea, new place, new time)? If not → cut.
2. Calm register → dissolve family. Energetic → whip/push/zoom family.
3. Does the next segment carry a surprise? → smash cut or `glitch-*`, SFX
   peaked exactly on the transition's midpoint.
4. Horror build → cuts until the sting, then ONE hard transition (`fade-black`
   / `glitch-warp`) at the payoff only.

## Placement mechanics

- Add via `effect_layer_add` on the clip that begins the new section; the
  transition's length is part of that clip's entrance — keep the narration's
  first word clear of it (start the word ~0.2 s after a 0.4 s transition ends,
  or let the SFX whoosh cover it).
- A transition eats into BOTH clips visually; when in doubt shorten the
  transition (0.25–0.4 s) and keep picture time.
- Pair each transition with its SFX (whoosh for whips/pushes, riser for
  zooms, impact for smash/glitch) — peak on the transition, per
  `prompts/sfx_design.md`. A silent transition is a mistake.

## Self-check

- [ ] ≤ 4 transitions per 30 s; every one sits on a section change.
- [ ] Register matched (no `cube` in a documentary).
- [ ] Every transition has its sound, peaked on the moment.
- [ ] No `fade-black` inside the first 5 s or the last 2 s of a short.
