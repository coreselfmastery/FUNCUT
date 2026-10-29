# clip-skills: the editing-craft shelf for the AI director

[clip-skills](https://github.com/appergb/ClipSkills) is a video-editing
skill suite for AI agents, by appergb. This repository vendors it at
`.claude/skills/clip-skills/` — the install path Claude-Code-format project
skills live at — so every agent that works inside this repo (Claude Code,
Forge via `forge skill install coreselfmastery/FUNCUT`, or any harness that
reads this skill format) gets it automatically. It is pinned at upstream
commit `4d85c26` **plus** a FUNCUT integration delta (below); MIT, with a
`NOTICE.md` in the skill folder carrying the attribution details.

The one-line division of labor: **maestro is the eye, clip-skills is the
cutter's brain.** Maestro knows what good design, motion and film language
look like; clip-skills knows how an edit is actually built — where to cut,
what to cover with, how the audio spine is shaped, how deep the music
ducks, what a transition means, which export numbers are right. The
`funcut-director` skill routes phases into both shelves (its §6 and §7).

## What it adds to FUNCUT

The skill is two layers, both Markdown knowledge (no runtime dependencies):

- **The knowledge core** (`references/01..15-*.md`, 15 software-agnostic
  modules) — professional post-production experience rewritten as agent
  decisions (situation → basis → decision → output), each with quick
  answers and per-software implementation notes. The modules that matter
  most to this editor's pipeline: `02` (the audio-driven cutting spine —
  the theory behind Phase 1–2), `03` (A/B-roll classification and the five
  B-roll rules behind Phase 4–5), `04`/`15` (music/duck and the voice chain
  behind the L floor), `05` (pacing and structure), `06` (color-grading
  workflow behind Phase 6), `13` (caption readability and the four-zone
  text system behind the Z floor), `14` (transition semantics behind the T
  floor), `12` (export parameters behind Phase 8).
- **The execution guides** (`references/editor-*.md`) — the "which buttons"
  layer per software. `editor-funcut.md` is the one written for THIS
  editor: the module-to-tool map, the 80 ms padding rule, the honest
  capability-boundary table (no optical-flow interpolation, no planar
  tracking, no native multicam — with workaround paths), and the
  precedence rules against this repo's own skills. The other three guides
  (Palmier Pro, code-pipeline, other NLEs) make the same craft portable
  when a job leaves this editor.

The modules are written in Chinese; the SKILL.md frontmatter carries
bilingual trigger keywords so the skill fires on English requests too. An
agent working in English reads the Chinese modules fine — they are tables
and decision lists, not prose-heavy pages.

## The division of labor (five skills, no overlap)

| Skill | What it owns | Format |
| --- | --- | --- |
| `funcut-director` | The pipeline: measure, plan, execute, verify — every MCP tool, the phases, the hard floors | Router (`SKILL.md`) + playbooks in `prompts/` |
| `motion` | The editor's own motion-comp engine: presets, nodes, baking, overlays | `SKILL.md` (see also `docs/MOTION.md`) |
| `maestro` | Aesthetics: what good design, motion, film language and sound look like | 28 modules + vendored corpora |
| `clip-skills` | Editing craft: how to cut — audio spine, B-roll, pacing, grading workflow, voice, transitions, export | 15 knowledge modules + 4 execution guides |
| `run-concat` | Building and verifying the app itself | `SKILL.md` |

Two integration rules (both shelves follow the same ones maestro does):

1. The Phase-0 `creative_brief` stays the single locked brief;
   clip-skills' clarification checklist is a way to fill it, not a second
   interrogation. Its autonomous-execute mode does not run inside this
   repo — the director pipeline is the executor.
2. Where guidance conflicts, the director's hard floors win. clip-skills
   refines how a floor is met (its pacing heuristics tune the P floor's
   ranges by niche; its duck depths fill the L floor's context table),
   never lowers one.

## Using it

- **Claude Code**: nothing to do — skills under `.claude/skills/` load
  automatically in this repo. Ask "how deep should the BGM duck under the
  hook" or "which transition for a time jump" and the routing in
  funcut-director §7 pulls the right module.
- **Forge**: `forge skill install coreselfmastery/FUNCUT` installs the
  skills from this repo — `funcut-director`, `maestro`, `clip-skills` and
  `motion` arrive together (see [docs/forge/FORGE.md](forge/FORGE.md)).
- **Other harnesses**: read `.claude/skills/clip-skills/SKILL.md` (the
  router), then only the module the task needs.

## The FUNCUT integration delta (and the upstream patch)

The vendored tree is upstream `4d85c26` plus changes contributed back
upstream (delivered as a patch bundle — this token's scope could not push
to `appergb/ClipSkills` directly, so the patch ships with the repo docs
and the same tree is vendored here):

- `references/editor-funcut.md` — new; the FUNCUT execution guide.
- `references/13-字幕与文字设计.md`, `14-转场决策.md`, `15-人声与对白处理.md`
  — three new knowledge modules (subtitles & text, transitions, voice).
- SKILL.md description gains English trigger keywords and the FUNCUT
  routing row; both READMEs document the FUNCUT execution path.
- Fixed two stale `jianji-knowledge` references left over from the
  upstream rename (in `editor-palmier-pro.md` and `editor-other-nle.md`).

## Updating the vendor (drift tracking)

`scripts/clipskills/` carries a single-upstream drift checker:

```bash
node scripts/clipskills/check-upstream.mjs        # exit 1 on drift
node scripts/clipskills/check-upstream.mjs --pin   # re-pin to HEAD
```

When drift is reported, re-vendor from upstream: copy a fresh
`ClipSkills/clip-skills/` over `.claude/skills/clip-skills/`, re-apply the
FUNCUT delta (the list above; check `git log` on this folder for the exact
set), update the pin in `scripts/clipskills/upstreams.json` and the NOTICE,
and re-read the new SKILL.md routing against the §7 table in
funcut-director — if upstream renamed or renumbered modules, the phase map
must follow.
