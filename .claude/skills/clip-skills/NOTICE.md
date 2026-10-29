# Notice & Attribution

`clip-skills` is vendored from
[appergb/ClipSkills](https://github.com/appergb/ClipSkills) (MIT), with the
FUNCUT-specific extensions listed below. It is pinned at upstream commit
`4d85c26` **plus** the FUNCUT integration commit (`e38abe7` on the working
copy — the vendored tree includes that commit's additions: the
`editor-funcut.md` execution guide and three new knowledge modules).

## What is vendored

The skill in this folder is the upstream `clip-skills` skill, unmodified in
structure (SKILL.md router + `references/` loaded on demand):

- **15 software-agnostic knowledge modules** (`references/01..15-*.md`) — the
  editing craft core: video typing, audio-driven cutting, A/B-roll
  classification, sound & music, pacing & structure, color grading,
  keyframes & motion, speed ramps, keying/masking/compositing, tracking &
  motion-graphics packaging, multicam & interviews, subtitles & text design,
  transitions, voice & dialogue processing, export & delivery. Written as
  agent decisions (situation → basis → decision → output), in Chinese with
  bilingual trigger keywords in the frontmatter.
- **4 execution guides** (`references/editor-*.md`) — the "which buttons"
  layer: `editor-funcut.md` (this editor, see below), `editor-palmier-pro.md`,
  `editor-code-video.md`, `editor-other-nle.md`.

## What was added for FUNCUT

- `references/editor-funcut.md` — the FUNCUT/Concat execution guide: the
  module-to-tool map (`transcribe` word times, `clip_cut_spans` + the 80 ms
  padding rule, `openvid_search` semantic B-roll, `transitionIn` transitions,
  whole-runtime `effect_layer_add` grading, `keyframe_set` motion,
  `concat.chroma-key`, `measure_loudness`, the `watch_render` /
  `craft_critique` verification loop), an honest capability-boundary table
  (no optical-flow interpolation, no planar tracking, no native multicam —
  with the workaround paths), and the division of labor with this repo's
  other skills.
- Three new knowledge modules (13 subtitles & text, 14 transitions, 15 voice
  & dialogue processing), contributed back upstream as a patch (see
  `docs/CLIPSKILLS.md`).
- Bilingual trigger keywords in SKILL.md's description (the upstream
  description was Chinese-only; FUNCUT's agents work in English).

## License

MIT — see [LICENSE.txt](LICENSE.txt) in this folder. The upstream LICENSE
covers the original expression (skill structure, decision rules,
orchestration). The editing techniques it systematizes are credited to the
影视飓风 course and the creator community and remain their owners' IP; this
is an independent educational re-creation (upstream's disclaimer, carried
verbatim in the upstream README).
