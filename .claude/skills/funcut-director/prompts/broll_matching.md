# B-roll matching

B-roll is meaning, not decoration. The rule: **match the visual MEANING of the
narration**, not its keywords. "Dissolved" over a shot of sugar in water beats
any stock clip tagged "ocean".

## Query engineering (openvid_search)

Concrete noun + action or setting + shot feel, in that order:

- ✅ "aerial canyon river sunrise" · ✅ "close-up hands typing mechanical
  keyboard" · ✅ "slow motion waves crashing cliff"
- ❌ "success" · ❌ "beautiful nature 4k" · ❌ "business growth"

OpenVid indexes captions in plain descriptive English. Use the dataset's own
vocabulary: things, actions, places, weather, time of day. Prefer
`orientation` to match delivery (`vertical` for 9:16) and use the sorts
honestly: `aesthetic` for the hook and payoff (the frames people screenshot),
`relevance` for explanation, `motion` when the narration is about speed or
change, `temporal` for steady handheld feel.

## Scoring the results

Each clip carries `aesthetic` (0–6), `motion`, `temporal` (stability 0–1),
`camera`, `seconds`, `bytes`. Pick per segment:

1. **Meaning** (non-negotiable): the caption must describe what the narration
   is really about at that moment.
2. **Aesthetic ≥ 4** for hook/payoff segments, ≥ 3 elsewhere.
3. **Motion fit**: narration calm → `temporal` high, `motion` low; energetic
   → `motion` high. Never cut a shaky handheld clip into a calm VO without
   `clip_stabilize`.
4. **Length**: `seconds` ≥ 1.5× the segment (trim after placing).
5. **Cost**: `bytes` — on a metered connection prefer the smaller of two
   equal candidates.

## Variety and reuse rules

- No clip id repeats within a 20 s window.
- Vary shot type across adjacent segments: wide → close → detail → wide.
  Two aerials back to back read as one long boring shot.
- Vary camera motion: follow a `zoom_in` with a `static` or `pan_left`,
  not another zoom.
- For a full faceless video you need ~1 clip per 2.5–4 s of runtime — a
  30 s video is 8–12 clips, so run one search per plan segment (they are
  independent — fan the searches out in parallel).

## The fallback ladder (never a gap, never a stall)

1. **Re-query** once, more concretely (drop the shot feel, keep the subject:
   "canyon river" instead of "aerial canyon river sunrise").
2. **Abstract to the metaphor**: the narration's concept as a physical scene
   ("dissolved" → "sugar stirring in water close-up").
3. **`textCard`**: the segment's 2–5 key words on a clean background (a
   `timeline_background` blur or color), one of the 31 text presets, with
   motion from `prompts/keyframes_motion.md`. Numbers and names ARE great
   text cards.
4. **`titleCard`** for section breaks when 2–3 fail.

A black frame or a stalled search is a director failure; a text card with a
punch zoom is a style choice.

## Niche query recipes

History: "ancient ruins", "old map parchment close-up", "candlelit archive
documents", "marble statue slow pan", "battlefield wide landscape" ·
Tech: "circuit board macro", "server room lights", "code on screen close-up",
"robot arm assembly", "city aerial night" · Luxury: "gold texture macro",
"watch mechanism close-up", "private jet interior", "yacht aerial",
"champagne pour slow motion" · Finance: "stock chart close-up", "coins
stacking macro", "printing press money", "skyline financial district" ·
Science: "microscope slide macro", "space nebula", "laboratory pipette",
"ocean underwater wide" · Motivation: "runner sunrise road", "climber summit
reach", "boxer heavy bag sweat", "storm clouds timelapse" · Horror:
"dark forest night", "abandoned corridor flashlight", "fog street lamp",
"crows on branch" · Fitness: "gym barbell plates", "meal prep overhead",
"treadmill close-up feet", "jump rope shadow" · Gaming: "arcade neon",
"esports arena crowd", "controller hands glow", "pixel graphics" ·
How-to: the actual object being worked on, hands, overhead desk shots.

More per niche in `prompts/niche_playbooks.md`; MAGICK stickers are CC BY-NC —
only the `openmoji` shelf is safe for monetized work.
