# Forge research: the harness, KinoCut, and the landscape

This documents what was verified before the Forge integration was designed, so
future changes start from facts rather than folklore. Everything below was
checked against primary sources (the projects' own repositories and docs) in
October 2026, on `main` of this fork (`010032b`).

## 1. The Forge harness — verified

**Forge is real, and it is the harness this integration targets:**
<https://github.com/Adulari/forge> — "the model-agnostic AI coding agent. One
harness, every provider — routed to the cheapest capable model that can do the
job, with automatic cross-provider failover." A single Rust binary: full-screen
or inline terminal UI, a one-shot/headless CLI (`forge run "task"`), a queue
autopilot (`forge queue`), and a desktop companion app. It installs in one line
on Linux and macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/Adulari/forge/main/install.sh | sh
```

### Claim-by-claim verification

| The plan's claim | Verdict | Notes |
| --- | --- | --- |
| Model Mesh routes each task to the cheapest capable model across 18+ providers | **Verified** | Anthropic, OpenAI, Groq, Gemini, DeepSeek, OpenRouter, NVIDIA NIM, SambaNova, Mistral, Cohere, xAI, Cerebras, Qwen, Ollama, any OpenAI-compatible endpoint. `forge mesh "<task>"` explains a routing decision. |
| Task-tier routing (trivial edits to free models, complex planning to frontier) | **Verified** | Benchmark-ranked cost tiers; `/effort` steers the whole route. |
| Cross-provider failover on 429 (wait, retry, next ranked provider) | **Verified** | Health-aware, capability-aware failover down the full ranked catalog. |
| Multiple keys per provider round-robin | **Verified** | `forge auth <provider>` again to stack keys; also `GROQ_API_KEY="k1,k2"` or numbered `GROQ_API_KEY_2`, `GROQ_API_KEY_3`. Two free Groq keys roughly double requests/min. |
| `credit_mode = "strict"` = free + subscription only | **Verified** | `[mesh]` section of `config.toml`, alongside `daily_budget_usd` and `monthly_cap_usd`. |
| `spawn_agents` fan-out with opt-in git-worktree isolation | **Verified** | Mesh-routed child agents. |
| `/workflow` sandboxed JS orchestration (`agent()`, `parallel()`, `pipeline()`) | **Verified** | Saved scripts, phases, a dedicated animated workflow view. |
| `/duel` races up to 3 distinct-provider models in parallel worktrees, merges the winner | **Verified** | |
| pre/post tool-use hooks | **Verified** | `[[hooks]]` in `config.toml`; hooks ship in `forge migrate` bundles. |
| Completion gate (a turn claiming "done" needs a real artifact + recognized check) | **Verified** | "Anti-phantom-success completion gate, test-pinned." |
| Evidence ledger (exit codes, digests) + doom-loop guards | **Verified** | Objective, tool-grounded completion checks; doom-loop protection named in the feature list. |
| Forge is both an MCP client and an MCP server | **Partly verified** | MCP client ships (stdio + HTTP/SSE, permission-brokered, allowlisted). The client spec lists server mode as a future extension; treat "client" as the fact. |
| Skills are Claude-Code-compatible markdown | **Verified** | `forge import claude` migrates `~/.claude` commands/skills/agents; imported skills auto-normalize (claude/codex paths to forge paths); `forge skill install owner/repo[@ref]` installs skills from a GitHub repo. |
| ~100–200 MB RAM | **Estimate** | "One fast native binary — Rust, no Node/Python/Bun runtime, no Electron." No published number; the native-binary claim supports the estimate. Treat as small next to the editor. |

### The facts that shape this integration

- **Config layers** (defaults → user → project → env): user config is
  `~/.config/forge/config.toml`, project config `./.forge/config.toml`, **MCP
  servers `./.forge/mcp.toml`**, agent types `./.forge/agents/<name>.toml`,
  project memory `./.forge/AGENTS.md`, env overrides with a `FORGE_` prefix,
  API keys in the OS keyring via `forge auth`. *Correction to the original
  plan:* MCP servers are not declared in `~/.config/forge/mcp.toml`; they live
  in the project's `.forge/mcp.toml`, or at user scope via
  `forge mcp add ... -s user`.
- **Adding an MCP server** — `claude mcp add` syntax:
  `forge mcp add <name> <commandOrUrl> [args...] [-t stdio|http|sse]
  [-s local|user|project] [-e KEY=VAL]`. Secrets never land in the TOML
  (`token_env` / keyring instead).
- **Server entry shape** (what this repo ships as `.forge/mcp.toml`):
  `[[servers]]` with `name`, `[servers.transport]` `type = "stdio"`,
  `command`, `args`, optional `env`, plus `call_timeout_secs` at the top.
- **Deferred tool discovery:** a server with many tools is reached through the
  meta-tools `mcp_search_tools` and `mcp_call` (qualified name
  `<server>__<tool>`, e.g. `funcut__clip_trim`) rather than flooding the
  context with every schema. **The funcut-director skill teaches this.**
- **Per-call MCP timeout** defaults to 60 s — too short for transcription and
  long analyses, so this repo's `.forge/mcp.toml` sets `call_timeout_secs = 900`.
- **Free tier** (no credit card): Groq (~30 RPM, 1K requests/day), Cerebras
  (~30 RPM, 14.4K/day), NVIDIA NIM, SambaNova (~20 RPM), Ollama local.
  `credit_mode = "strict"` keeps routing to free + subscription models only.
- **Overnight:** `forge queue` drains queued tasks headless into isolated
  git worktrees (budget-capped, leaves review-ready branches);
  `forge schedule` runs recurring prompts on native OS timers.

## 2. KinoCut — what it actually is, and what we took

**Kinocut by KyaniteLabs** (<https://github.com/KyaniteLabs/kinocut>,
<https://kinocut.dev/>) is not a GUI editor — it is a guardrailed
video-editing **MCP server for AI agents** (203 MCP tools, Apache-2.0,
formerly "mcp-video"). What is worth copying is not its tool count but its
methodology, which it publishes as `skills/kinocut/SKILL.md`:

- **Video receipts:** every step is recorded with SHA-256 digests, a resume
  cursor, and an explicit `human_review pending` state before anything is
  called done.
- **Plan → render → verify gates:** `video_workflow_validate` → `plan` →
  `render` → `inspect`, with an operation allowlist and review floors
  (blackdetect, LUFS, first-15-seconds).
- **Human-in-the-loop B-roll:** `video_propose_broll` proposes; nothing is
  inserted silently.
- **Repurposing:** `video_repurpose_plan` → `video_repurpose` with per-platform
  manifests (aspect, target LUFS, max duration) for YouTube Shorts, Reels,
  TikTok.

What this repo adopted: the plan-first discipline (the funcut-director skill's
edit plan), the verify-before-done discipline (`preview_frame` checkpoints +
the review checklist), explicit licensing flags (MAGICK stickers are CC BY-NC),
and per-platform export presets in the niche playbooks.

## 3. The wider landscape — the numbers the skill encodes

- **OpenMontage** (github.com/calesthio/openmontage) — the reference for
  agent-driven editing methodology: a three-layer split (tools / pipeline
  manifests / skills), and a short-form retention rulebook with sourced numbers:
  hook inside 3 s (the scroll decision happens around 1.7 s), a pattern
  interrupt every 2–4 s, 20–40 cuts/min for short-form, captions at 42 px or
  larger with word-highlight, −14 LUFS integrated / −1 dBTP, 180–200 words per
  minute, 15/30/60 s length templates, loop endings.
- **Revid.ai** — MIT-licensed SKILL.md files publish their defaults: 35 s
  default render, 9:16, captions centered, music beat-synced
  (`music.syncWith: "beats"`), SFX on, auto-cover; a 14-category, 130+ hook
  library; from 2.4M TikToks: 16–30 s videos earn the best median views and
  faceless videos earn ~1.9× the views of person-on-screen.
- **AutoShorts.ai** — the "series" model (topic + voice + style + duration +
  music recurring weekly) and the 3-scene storyboard rule: establish the
  setting → show the detail → land the human moment.
- **Descript** — script-first editing: edit the text, the timeline follows.
  The Forge pane's script box is that idea's honest version: the script is the
  source of truth, the agent cuts to it.
- **CapCut × Codex** — chat-refine, then "open the full editor" handoff. The
  Forge pane is the same handoff: the agent edits the open window's project;
  you watch it happen.

## 4. What FUNCUT already has (the baseline, from the code)

Verified by reading this fork's current source: 69 MCP tools (projects,
reading, media, every cut operation, masks/stabilization, titles,
segment-timed captions, keyframes, effect layers, batched atomic edits,
templates, export with progress, `preview_frame`), the OpenVid library
(937,957 clips), BGM (1,191 tracks), SFX (430 CC0 sounds, 12 categories),
MAGICK stickers (145,193, CC BY-NC), 127 effect packages (57 filters, 25 audio,
23 effects, 22 transitions), 31 text presets, Kokoro/Pocket/Chatterbox voices,
whisper.cpp transcription, window/headless/auto MCP backends, and a Slint UI
with a free tab slot and a settings dialog with a free page.

**The conclusion that shaped everything:** the engine already covers every cut,
transition, effect, keyframe, and grade the plan asked for. What was missing was
(1) the agent's *senses* — scene, beat, and silence analysis, and speech on the
API — and (2) the *director's brain* — a skill that encodes professional
editing decisions. Those, plus the Forge wiring and the UI pane, are what this
integration adds.

Sources: <https://github.com/Adulari/forge> (README, `docs/features/mcp-client.md`,
`docs/features/config-command.md`) · <https://github.com/KyaniteLabs/kinocut>
+ <https://kinocut.dev/> · <https://github.com/calesthio/openmontage> ·
<https://github.com/coreselfmastery/FUNCUT> (own source, API read) ·
Revid.ai and AutoShorts.ai public skill/doc pages.
