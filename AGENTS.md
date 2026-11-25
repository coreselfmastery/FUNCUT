# FUNCUT — an agent's guide to this repository

You are an AI agent (Claude Code, Codex, Cursor, opencode, Zed, or any
MCP client) about to work in FUNCUT, a Linux video editor with an agent
stack bolted on. This file is the map. It mirrors `.claude/CLAUDE.md`
for the tools that read this convention instead.

## What this is

FUNCUT is a fork of [Concat](https://github.com/jub0t/Concat) — a
Rust + Slint video editor (`src/crates/*`, 20+ crates, ~220k lines) —
tuned for small low-memory Linux laptops and extended with:

- a built-in **MCP server** (`concat --mcp`, 116 tools, 108 served to a
  director by default, including `auto_edit` - the window's one-click
  spine as one headless call) that can edit whole projects end to end;
- the **funcut-director** skill (`.claude/skills/funcut-director/`): a
  25-phase measured pipeline that turns a script or a voiceover into a
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
| [docs/SANDBOX.md](docs/SANDBOX.md) | Installing a release in a sandbox, running headless, editing and exporting |
| [docs/FEATURES.md](docs/FEATURES.md) | The verified inventory: every tool, page, engine, model, with counts |
| [docs/forge/FORGE.md](docs/forge/FORGE.md) | The Forge harness that drives a director run (any vision-capable model) |
| [docs/EMOTION.md](docs/EMOTION.md) | The emotion read and the gate that drives easing, colour, sound, duck, stock |
| [docs/SOUND-SENSE.md](docs/SOUND-SENSE.md) | What a sound file measures, the six roles it is scored for, and where the shelf's metadata lies |
| [docs/YOUTUBE.md](docs/YOUTUBE.md) | The YouTube package on an AUTO EDIT plan: profile, titles, thumbnail frames, Shorts and the retention map |
| [docs/MOTION.md](docs/MOTION.md) | The Geneva comp document, its ops, presets, storyboard, farm |
| [docs/MAESTRO.md](docs/MAESTRO.md) · [docs/CLIPSKILLS.md](docs/CLIPSKILLS.md) | The two judgment shelves |
| [docs/STOCK_LICENSING.md](docs/STOCK_LICENSING.md) | What the stock shelf's clips are licensed as (read before publishing) |
| [docs/FUNCUT-CHANGES.md](docs/FUNCUT-CHANGES.md) | The fork's change ledger, newest findings first |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Building, testing, and how changes are licensed |

## The signal contract (read before touching AUTO EDIT)

`voice_signals` measures more than AUTO EDIT spends, and that gap is the
bug class that keeps recurring here. Three guards in
`src/crates/concat-mcp/src/director_program.rs` hold the line, and each
one is there because something slipped past the one before it:

- `every_signal_family_has_a_reader` - every family in
  `signal_vocabulary` (`tools.rs`) has a reader, or a stated reason.
- `every_reading_inside_a_family_has_a_consumer` - every `pub field`
  `voice_read.rs` parses is named by the program, read through a
  `voice_read` helper the program calls, or listed with a reason. The
  family guard cannot see inside a family: `prosody` was listed as read
  while `pitchRangeSemitones` was spent by nothing.
- `every_vocabulary_family_names_the_decision_it_informs` - every family
  the edit spends names a decision the log's gate accepts
  (`cut`/`caption`/`graphic`/`bgm`/`transition`/`pacing`), so the critic
  is told what each signal drove.

So: when you add a reading, wire it to a decision and log it with its
confidence; when you deliberately leave one unspent, name it in the
guard's `NOT_SPENT` list with the reason. Never widen a guard to make it
pass - the guards are the deliverable.

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

When linking test binaries for `concat-mcp` (or anything that pulls in
`ort`), a local ONNX Runtime setup can fail with `rust-lld: error:
duplicate symbol: onnx::...` if the link includes conflicting ONNX/protobuf
implementations. Use the shared, digest-pinned runtime from
`scripts/setup-build-env.sh` and set `ORT_LIB_LOCATION` plus
`ORT_PREFER_DYNAMIC_LINK=1` as shown below. If the duplicate remains,
inspect the verbose linker command and remove the conflicting static
library from the link set. **Do not mask it with
`--allow-multiple-definition`**: that suppresses a real link diagnostic
and can leave the selected symbol implementation dependent on link order.

A container restart wipes everything outside `/workspace` — the Rust
toolchain, apt packages and the shell `PATH`. `/workspace/envsetup/setup.sh`
rebuilds it (toolchain 1.93.1, libclang, cmake, the GTK/GL/ALSA/ssl dev
packages); run it first, then `/workspace/envsetup/verify.sh`. The
`ffmpeg-dev/` and `.ort-shared/` trees under the repo root survive, and
the build needs them on the environment:

```sh
export FFMPEG_DIR=$PWD/ffmpeg-dev
export PKG_CONFIG_PATH=$PWD/ffmpeg-dev/lib/pkgconfig
export ORT_LIB_LOCATION=$PWD/.ort-shared/lib ORT_PREFER_DYNAMIC_LINK=1
export LD_LIBRARY_PATH=$PWD/ffmpeg-dev/lib:$PWD/.ort-shared/lib
export LIBCLANG_PATH=/usr/lib/llvm-19/lib
```

Build with `-j2`: the 4-core box runs out of memory and the container is
restarted mid-build at higher parallelism.

## The report card: what runs, and how each lane fails

Seven workflows. `main` requires a pull request (zero approvals, so one
maintainer can merge their own) and three checks, all from `ci.yml`:
`Format, locales and models`, `The analysis senses against real fixtures`,
`Engine tests and lints`. `enforce_admins` is on, conversation resolution
is required, and force pushes and deletions are off. The rest - `Build app`, `Geneva`,
`Nix`, `Dependency security audit`, `Release` - are the lanes that turn
red without blocking a merge, so read them before calling `main` green.

The failure modes this repository has actually paid for, and the fix each
one has:

- **A job that cancels itself.** `The analysis senses against real
  fixtures` had a 15-minute ceiling and needed 15m16s; a timeout counts as
  a failure, so the lane was red on a working tree. Ceilings are for hangs,
  so they sit well above the measured worst case now (30 minutes there,
  240 for the engine job, whose cold build has run 2h31m58s).
- **A step that spends its whole budget compiling.** The engine job builds
  its test binaries with debug information off (`CARGO_PROFILE_TEST_DEBUG`
  and `CARGO_PROFILE_DOCTEST_DEBUG`), which made the dev profile a
  different artifact set - so the smoke step's `cargo run` recompiled the
  whole dependency graph and hit its 20-minute ceiling without ever
  launching the binary. `CARGO_PROFILE_DEV_DEBUG: "0"` aligns the dev
  artifacts with the ones the test build already produced, and the
  headless binaries' own Slint codegen and link get a step of their own.
  Cargo resolves features per invocation, so the later `-p concat-cli`
  build unified them differently and recompiled the tree again; the smoke
  and the 4 GiB benchmark now run the binaries that step built.
- **A benchmark with no ffmpeg to make its clip with.** The 4 GiB step
  generates its source clip with `ffmpeg`, and the engine job installs the
  BtbN bundle for its *libraries* only - no CLI on the PATH - so the step
  died on `no --video and no ffmpeg`. The pinned bundle carries the
  executables too; `ffmpeg-dev/bin` joins the PATH now, with a `test -x`
  in the install step so a bundle without them fails there instead.
- **A retry that gives up too early.** `scripts/ci/fetch.sh` retried the
  release CDN five times over 62 seconds while the CDN was returning 500s
  for 61 of them. Backoff is 2/4/8/16/32s per attempt, digest re-verified
  each time, and never retried at all on a digest mismatch or a 4xx. The
  retry set was still short of the ways a CDN dies: on 2026-11-21 the
  Geneva lane lost ONNX Runtime to `curl: (18) transfer closed with
  4135441 bytes remaining to read` - a truncated body, HTTP 200 - and the
  script gave up on attempt 1 because exit 18 was not one of the reasons
  it rode the backoff for. 18 (partial file), 52 (empty reply), 55 (send
  failure) and 92 (HTTP/2 stream) ride it now, with 28 and 56. The digest
  is what makes a wider retry set safe: a truncated body never matches it,
  and a wrong release still fails on the first attempt.
- **The Nix lane losing the runner.** Three runs on `main` ended "runner
  lost communication" with no step conclusion, 1h15m-1h18m in, always in
  the silence after the last compile line. That silence is the thin-LTO
  join, which loads every codegen unit of every crate at once; the
  derivation now sets `CARGO_PROFILE_RELEASE_LTO=false` (the codegen-units
  ladder is untouched, because those units keep the window crate's own
  rustc alive), and `Build app` still builds the shipping `app` profile
  with fat LTO, so LTO stays tested.
- **A dependency audit with ignores.** Seven `--ignore` entries hid the
  `zip 2.4.2`/`zstd` subtree that sherpa-onnx-sys 1.13 pulled in. PR #173
  moved to sherpa-onnx 2.x and deleted every ignore; the audit is green on
  `main` with zero suppressions, and a new advisory is meant to fail the
  lane rather than be suppressed.

When a lane goes red, read the job's steps first: a step with no
conclusion and a log that stops mid-line is a runner death, not a test
failure, and retrying it is the honest first move.
