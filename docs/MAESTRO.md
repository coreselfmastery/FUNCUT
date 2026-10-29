# Maestro: the judgment shelf for the AI director

[maestro](https://github.com/leobbaroni/maestro) is a unified design, motion,
3D and video skill for AI coding agents, by leobbaroni. This repository
vendors it at `.claude/skills/maestro/` — the install path maestro's own
documentation recommends for project skills — so every agent that works
inside this repo (Claude Code, Forge via `forge skill install
coreselfmastery/FUNCUT`, or any harness that reads Claude-Code-format skills)
gets it automatically. It is pinned at maestro **5.0.0**, commit
`64e1d6ba443dc477547ad646c8b090afd15a0c04`.

The one-line division of labor: **funcut-director is the hands, maestro is
the eye.** The director skill knows the pipeline — which MCP tool to call, in
which order, against which hard floors. Maestro knows what good looks like —
typography, color, hierarchy, motion timing, shot vocabulary, sound design.
An agent that edits with FUNCUT and reads maestro while it does so makes
choices a plain tool-driver cannot.

## What it adds to FUNCUT

Maestro is two layers, and both earn their place differently:

- **The judgment layer** (`references/`, 28 modules) is distilled, editorially
  reconciled guidance. The modules that matter most to this editor's pipeline:
  `design-direction` (style registers, art direction), `design-foundations`
  (typography, color, hierarchy), `motion-principles` (timing, easing,
  choreography — engine-agnostic, so it applies to keyframes and motion comps
  as much as to web animation), `video-direction` (story structure, beat
  planning, pacing), `video-shotcraft` (a 157-card product-shot vocabulary
  with tuned reference implementations), `video-sound` (a declarative SFX pin
  table, anti-machine-gun spacing, and a music beat-sync method that verifies
  the render to a ≤3-frame tolerance), `design-audit` (systematic critique and
  anti-slop gates), and `process` (brief-locking discipline).
- **The depth layer** (`library/`, ten vendored corpora, verbatim with their
  own licenses) is what the judgment layer points into when a task needs a
  full recipe rather than a rule of thumb: the 157 shot cards' tuned demos
  (`video-shotcraft`), GSAP-grade animation references (`gsap-skills`),
  fourteen named techniques (`genjutsu`), LottieFiles' motion principles
  (`motion-design-skill`), design-DNA extraction (`design-dna`), impeccable's
  craft corpus, and the generative-media mechanics baselines
  (`higgsfield-skills`, `comfy-skills`).

## The division of labor

FUNCUT's director stack ships four skills (a fifth, `run-concat`, is the
build-and-verify utility). They do not overlap; they stack:

| Skill | What it owns | Format |
| --- | --- | --- |
| `funcut-director` | The pipeline: measure, plan, execute, verify — every MCP tool, the phases (0–8 plus the 1b/5b sub-phases), the hard floors | Contract + router (`SKILL.md`) with playbooks in `prompts/` |
| `motion` | The editor's own motion-comp engine: presets, nodes, baking, overlays | `SKILL.md` (see also `docs/MOTION.md`) |
| `maestro` | Judgment: what good design, motion, film and sound look like — plus escape-hatch engines when a piece needs code | 28 modules + vendored corpora |
| `clip-skills` | Editing craft: how to cut — audio spine, B-roll, pacing, grading workflow, voice, transitions, export numbers | 15 knowledge modules + 4 execution guides (see [docs/CLIPSKILLS.md](CLIPSKILLS.md)) |

The routing from the director's phases into maestro modules is in the
funcut-director skill, §6. Two rules govern the interaction: the Phase-0
`creative_brief` remains the single locked brief (maestro's interview
questions are a way to fill it, not a second brief), and the director's hard
floors always win where guidance conflicts — maestro refines how a floor is
met, never lowers it.

## The escape hatch: when the editor's motion layer is not enough

FUNCUT's motion layer covers titles, tickers, charts, kinetic captions and
the 327 effect packages. When a piece needs something it cannot express — a
bespoke 3D logo sting, a scroll-driven narrative sequence, a procedural
generative background — maestro's engine modules describe how to build it as
code (`video-remotion` for React-to-video, `video-hyperframes` for
HTML-to-video, `gsap` and `threejs` for animation and 3D), and the rendered
result is ordinary footage: `media_import` it and cut it like any other clip.
The shot cards in `video-shotcraft` ship tuned Remotion demo implementations
of exactly the shots product and promo work reaches for.

## Using it

- **Claude Code**: nothing to do. Skills under `.claude/skills/` load
  automatically in this repo; ask for design/motion/video work or name
  `maestro` directly.
- **Forge**: `forge skill install coreselfmastery/FUNCUT` installs the skills
  from this repo — both `funcut-director` and `maestro` arrive together (see
  [docs/forge/FORGE.md](forge/FORGE.md), section 4).
- **Other harnesses** (Codex, anything that reads Markdown): maestro's own
  `AGENTS.md` pattern applies — read `.claude/skills/maestro/SKILL.md` (the
  router), then only the module the task needs.

Maestro itself has no runtime dependencies; the engines above only matter
when a task actually reaches for them, and maestro checks what is installed
before it does.

## Updating the vendor (drift tracking)

`scripts/maestro/` carries maestro's upstream drift tracker:
`upstreams.json` pins the exact upstream commits each corpus was distilled
from, and `check-upstreams.mjs` (Node 18+, zero dependencies) compares them
against the upstreams today:

```bash
node scripts/maestro/check-upstreams.mjs        # exit 1 + module list on drift
node scripts/maestro/check-upstreams.mjs --json # machine-readable
```

When drift is reported, re-vendor: copy a fresh `skills/maestro/` from the
maestro repository over `.claude/skills/maestro/` (keep the `NOTICE.md` and
`LICENSE` files alongside), update the pinned maestro commit in this page's
first paragraph, refresh `scripts/maestro/upstreams.json` from the new
checkout, and re-read the funcut-director §6 routing table against the new
module list. See `scripts/maestro/README.md`.

## Licensing and attribution

Maestro's own text is MIT (`.claude/skills/maestro/LICENSE`); the vendored
library corpora are MIT and Apache-2.0, each carrying its license file
alongside it — all compatible with this repository's AGPL-3.0-or-later. The
full attribution table, including sources that informed the distilled layer
only, is `.claude/skills/maestro/NOTICE.md`, and the summary required by this
repository's notice policy is in `THIRD_PARTY_NOTICES.md` under "The maestro
skill".
