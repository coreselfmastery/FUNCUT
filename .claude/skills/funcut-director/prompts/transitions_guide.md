# Transitions

The 122 real ones, when each earns its place, and the mechanics that make
them actually render.

## The mechanics (get this wrong and the transition is not there)

A transition is a property of the CUT, carried by the clip that begins the
new side:

```json
clip_update { "path": "...", "clipId": "c42",
  "patch": { "transitionIn": { "id": "concat.whip-pan", "duration": 0.3 } } }
```

Three rules, all of them hard:

1. **`clip_update`'s patch, never `effect_layer_add`.** A layer is a
   single-input treatment span — it cannot carry a two-input transition, and
   a transition package placed as a layer renders as nothing. This is the
   single most common way an edit ends up with "no transitions".
2. **Same lane, adjacent, within half a frame.** The outgoing clip must END
   where the incoming clip STARTS, on the same track. A transition on a
   moved, gapped or cross-lane cut is silently orphaned — it plays as a
   straight cut with no error. After any `clips_move` or trim, re-check
   adjacency on every transition you own.
3. **The duration clamps to the shorter side.** A 0.5 s transition into a
   0.3 s clip is a 0.3 s transition. Plan sections so the clips beside a
   transition have room.

Take a transition off with `"transitionIn": null` in the same patch.

## Density

Cuts by default; a named transition only where meaning demands it:

| Mode | Density |
| --- | --- |
| Reel (≤ 60 s) | 2–4 per 30 s, every section change, one in the hook |
| Longform, calm niche | section changes only |
| Longform, energetic | every section change + at least one per 15 s |

A transition without its SFX (whoosh/impact on the cut) is half a
transition. Pair them in the plan.

## The 22

| Package | Use it when |
| --- | --- |
| `concat.dissolve` | a soft topic shift; the default when a cut is too abrupt but nothing is "happening" |
| `concat.fade-black` | end of a movement, a beat of loss, a hard chapter break |
| `concat.fade-white` | an arrival, a reveal of something bright, dawn/insight |
| `concat.push` | direction: forward momentum, "moving on" |
| `concat.zoom` / `concat.zoom-blur-transition` | energy spike, punch-in on a claim |
| `concat.whip-pan` | fast topic change, list rhythm, reel cuts on the beat |
| `concat.slideleft` (and right/up/down siblings) | lateral motion matching the footage's direction |
| `concat.wipe-left` / `wipe-right` | clean section change in explainers; editorial, not flashy |
| `concat.clock-wipe` | time passing, countdown feels |
| `concat.iris` / `concat.circleopen` | focus on one subject, old-film warmth |
| `concat.glitch-warp` / `concat.glitch-transition` | tech, error states, digital finance |
| `concat.pixelize` | data, privacy, anonymization beats |
| `concat.blinds` | structured, corporate, clean reveals |
| `concat.distortion-*` | impact hits with sound design |
| `concat.split-*` | before/after, compare beats |

## The 100 viral pack, by shelf

The classic 22 above stay the defaults. The hundred below are for edits
that want to be *felt* — faceless channels, reels, trailers, retention
edits. Same rules: a cut by default, and every transition paired with
its SFX on the cut.

| Shelf | Reach for |
| --- | --- |
| Viral Zoom | `concat.punch-in`, `concat.crash-zoom`, `concat.velocity-ramp`, `concat.dolly-vertigo` — the beat-drop of scale; hook moments and claim landings |
| Whip & Pan | `concat.whip-left`…`whip-down`, `concat.lurch-whip`, `concat.pendulum-swipe` — fast topic changes with camera energy |
| Glitch | `concat.datamosh-warp`, `concat.signal-drop`, `concat.bitrate-crash`, `concat.pixel-storm` — tech, error states, digital money |
| Shape Reveal | `concat.circle-pop`, `concat.heart-open`, `concat.hex-reveal`, `concat.star-wipe-spin` — playful reveals, romance, tech channels |
| Slice & Split | `concat.venetian-down`, `concat.checker-sweep`, `concat.razor-diagonal`, `concat.curtain-lift` — structured, editorial section changes |
| Spin & Flip | `concat.door-swing`, `concat.flip-card`, `concat.barrel-roll`, `concat.portal-spin` — scene geometry changing, portal/magic cuts |
| Flash & Impact | `concat.impact-flash`, `concat.whiteout-cut`, `concat.blackout-cut`, `concat.strobe-beat` — hits that land on the beat, drop moments |
| Organic | `concat.ink-bleed`, `concat.smoke-reveal`, `concat.ripple-dissolve`, `concat.liquid-melt` — mood shifts, dreamy sections |
| Meme & Bounce | `concat.meme-shake`, `concat.jelly-wobble`, `concat.squash-cut`, `concat.bounce-drop` — comedy, meme energy, listicle beats |
| Cinematic | `concat.film-burn-ember`, `concat.letterbox-slam`, `concat.shuttle-warp`, `concat.anamorphic-squeeze` — film language, scope moments |
| Social FX | `concat.story-slide`, `concat.swipe-soft`, `concat.zoom-slam-social` — the clean reads story edits already use |

The knobs matter: most of the pack takes an `amount` (0–100), the
directional ones a `softness` and a `seam dim`, the shape reveals a
`size` and a `rim glow`. Defaults already read; touch them only when the
footage demands it.

(Legacy short ids still resolve: `cross-fade`, `fade-black`, `fade-white`,
`push`, `zoom`, `wipe-left`, `wipe-right`. The package ids are the ones
`effects_list {kind: "transition"}` names — those are authoritative.)

The names above are the map; the truth is `effects_list`. If a name here is
not there, trust `effects_list`.

## When a plain cut wins

Most of the time. A cut is invisible; a transition is a sentence. If the
audience should not notice the edit, cut. If the edit itself is saying
something — new chapter, big claim, time jump — name it with a transition.
Two named transitions back to back is a style choice you must be able to
defend; three is a mistake.
