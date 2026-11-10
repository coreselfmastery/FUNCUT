# FUNCUT — an agent's guide to this repository

You are an AI agent (Claude Code, Codex, Cursor, opencode, Zed, or any
MCP client) about to work in FUNCUT, a Linux video editor with an agent
stack bolted on. This file is the map. It mirrors `.claude/CLAUDE.md`
for the tools that read this convention instead.

## What this is

FUNCUT is a fork of [Concat](https://github.com/jub0t/Concat) — a
Rust + Slint video editor (`src/crates/*`, 20+ crates, ~220k lines) —
tuned for small low-memory Linux laptops and extended with:

- a built-in **MCP server** (`concat --mcp`, 109 tools, 99 served to a
  director by default) that can edit whole projects end to end;
- the **funcut-director** skill (`.claude/skills/funcut-director/`): a
  12-phase measured pipeline that turns a script or a voiceover into a
  finished video;
- **maestro** and **clip-skills** (`.claude/skills/{maestro,clip-skills}/`):
  read-only judgment shelves the director reads phase by phase;
- the **Geneva** motion engine (`src/crates/concat-motion` + an external
  sidecar binary): JSON comps, 18 presets, kinetic typography, reels.

## Routing: which skill to use

- **Making or editing a video — `funcut-director`.** It is the only
  skill that executes the pipeline end to end. Everything else is a
  shelf it routes to, on its own phases.
- `maestro` and `clip-skills` are judgment shelves, not entry points:
  a request that arrives at one of them should be handed to
  `funcut-director` rather than answered with an interview.
- `motion` is the Geneva engine's own manual — read it when composing
  or debugging a comp, a reel, kinetic captions or a pulse.
- `run-concat` is dev-only: building and testing the editor itself.

## The documents, in the order an agent wants them

| Read this | When |
| --- | --- |
| [docs/MCP.md](docs/MCP.md) | Connecting to the editor over MCP; every tool area |
| [docs/FEATURES.md](docs/FEATURES.md) | The verified inventory: every tool, page, engine, model, with counts |
| [docs/forge/FORGE.md](docs/forge/FORGE.md) | The Forge harness that drives a director run (any vision-capable model) |
| [docs/EMOTION.md](docs/EMOTION.md) | The emotion read and the gate that drives easing, colour, sound, duck, stock |
| [docs/MOTION.md](docs/MOTION.md) | The Geneva comp document, its ops, presets, storyboard, farm |
| [docs/MAESTRO.md](docs/MAESTRO.md) · [docs/CLIPSKILLS.md](docs/CLIPSKILLS.md) | The two judgment shelves |
| [docs/STOCK_LICENSING.md](docs/STOCK_LICENSING.md) | What the stock shelf's clips are licensed as (read before publishing) |
| [docs/FUNCUT-CHANGES.md](docs/FUNCUT-CHANGES.md) | The fork's change ledger, newest findings first |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Building, testing, and how changes are licensed |

## The five things that surprise agents here

1. **A project is a folder**, not a file: `project_open {path}` names
   the folder; `concat.json` is inside it. Media the agent imports is
   copied into the project's own folders.
2. **The agent's sessions are not the window's.** A headless
   `concat --mcp` server edits projects of its own; to edit the project
   on screen, the window must share it (Settings > Automation, then
   `concat --mcp window`). The Forge page's voiceover run builds a
   sibling project and opens the finished edit in the window at the end.
3. **Times are seconds, keyframe positions are 0–1 fractions, title
   sizes and offsets are frame fractions.** Tool descriptions repeat
   this; believe them.
4. **A path to a PNG is not a picture you can see.** Only inline image
   content blocks (`preview_frame`, `motion_frame`, `watch_render`,
   `stock_contact_sheet`) are. A text-only model should say so and lean
   on the computed rankings and `craft_critique`'s measured facts.
5. **Every edit is one undoable step and autosaves.** `edit_apply` with
   a `batch` makes several into one; `edit_undo` takes them back.

## Building and testing

```sh
cd src
cargo check -p concat            # the fastest "does it build"
cargo test                       # the suite (CI counts it in docs/TESTING.md)
cargo fmt --check && cargo clippy --all-targets -- -D warnings
```

The pinned toolchain is in `src/rust-toolchain.toml` (Rust 1.93.1);
FFmpeg 8 dev libraries are needed (see `scripts/ci/deps.lock` and
`.github/workflows/ci.yml`), or `./install-linux.sh` from the repo root
installs a release build with everything a first run needs.
