# The Forge architecture

One chain, four layers. Everything in this integration serves it:

```
Forge (the harness: reasoning, Model Mesh routing, /workflow parallelism, hooks)
  └─ reads the funcut-director skill (the director's brain: .claude/skills/)
       └─ calls MCP tools (67 of them, over stdio)
            └─ `concat --mcp window|headless|auto` → the Concat API → the engine
```

- **Forge** is the coder-harness that reasons, routes each task to the cheapest
  capable model across 18+ providers, fans out with `spawn_agents` / `/workflow`
  / `/duel`, and refuses to call work done without an artifact and a check.
  Install: <https://github.com/Adulari/forge>. Skills are Claude-Code-compatible
  markdown, so the same skill also works in Claude Code and anything that
  imports its format.
- **The skill** is the product. The MCP tools are commodity; what makes an
  agent direct a professional edit instead of randomly calling tools is
  `.claude/skills/funcut-director/SKILL.md` and the playbooks under its
  `prompts/`: never leave a visual gap, SFX peak lands on the cut, section
  changes on downbeats, captions word-synced at 42 px+, a hook inside 3
  seconds, verify with `preview_frame` before export.
- **The tools** are the Concat API over MCP: 61 shipped (projects, media, every
  cut, masks, titles, captions, keyframes, effects, batched atomic edits,
  templates, export, libraries: OpenVid 937k clips, BGM 1,191 tracks, 430 SFX,
  MAGICK stickers) plus the six this integration adds — `analyze_scenes`,
  `analyze_beats`, `analyze_silence`, `analysis_wait`, `transcribe`,
  `tts_generate` — the agent's senses and voice.
- **The engine** is FUNCUT itself: the Rust + Slint editor, its window open
  while the agent works through the window's server (`--mcp window`), or
  headless for overnight queues.

## The two ways the agent reaches the editor

| Mode | How | Use |
| --- | --- | --- |
| `--mcp window` | Through the open FUNCUT window's socket server (Settings > Automation > Scripts and apps). The agent cannot overwrite a project the window has open. | Interactive direction: you watch the timeline change. |
| `--mcp headless` | An API in the agent's own process, writing under the home dir (or `CONCAT_MCP_ROOTS`). | Overnight queues, batch farms, nothing needs to be open. |
| `--mcp auto` | The window's server when reachable, headless otherwise. | The default. |

Environment overrides: `CONCAT_API_ADDRESS` + `CONCAT_API_TOKEN` name the
window's server directly; `CONCAT_MCP_ROOTS` confines headless writes.

## The analysis layer (new)

Three Python scripts — PySceneDetect (scene cuts), librosa (BPM, beats,
onsets), FFmpeg `silencedetect` (dead air) — live in
`src/crates/concat-api/analysis/`, are embedded into the binary with
`include_str!`, and are materialized at first use under the app's data
directory. The API runs them as jobs, exactly like an export:

```
analyze_scenes   → analysis.scenes   → Started { job }
analyze_beats    → analysis.beats    → Started { job }
analyze_silence  → analysis.silence  → Started { job }
transcribe       → speech.transcribe → Started { job }   (whisper.cpp)
tts_generate     → speech.generate   → Started { job }   (Kokoro/Pocket/Chatterbox)
analysis_wait    → export.status polled until done (the same generic
                   wait loop as export_wait), returns the job's status;
                   for analyses and transcriptions the status's `output`
                   field carries the result as JSON; for tts_generate it
                   is the WAV's path.
```

The Python dependency is small and honest: `pip install -r requirements.txt`
(scenedetect, opencv-headless, librosa). Without it, the analysis tools return
a structured error that says exactly that — the agent can tell the user
instead of failing silently. Python is the ecosystem PySceneDetect and librosa
live in; wrapping them keeps FUNCUT native while giving the agent its senses.

Speech comes from the engine's own models — whisper.cpp for transcription,
Kokoro int8 by default for TTS (4 GB-laptop friendly; Chatterbox is ~1 GB and
feature-gated) — now exposed on the API as jobs so every transport and any
harness can use them.

## The Forge pane (new, tab 12)

The UI is a director's chair, not a wizard:

- **Script box** — paste or write the script; this is the source of truth.
- **Voice** — TTS voice + speaking rate for the narration the agent will
  generate, or "no narration".
- **Style** — a niche preset (history, tech, luxury, motivation, finance,
  science, horror, fitness, gaming, how-to) that sets the look: caption style,
  color grade, pacing, BGM mood, SFX set, B-roll query recipes.
- **Start Forge** — composes the prompt (script + choices + the skill's
  contract) and runs `forge run` with the provider keys from Settings injected
  as environment variables, streaming every line the agent prints into the
  pane's log. Cancel stops the child.
- **Setup buttons** — one click runs `forge mcp add funcut concat -- --mcp
  window -s user` (connect the harness) and `forge skill install
  coreselfmastery/FUNCUT` (install this repo's skill).

The pane never edits your project itself; the agent does, through the window's
server, so every change lands on the timeline with full undo.

## Provider keys (new, Settings page 8)

Keys for Groq, Gemini, OpenRouter, Mistral, Cerebras, SambaNova, NVIDIA NIM
and friends are stored in `settings.json` (mode 0600, the same file that
already holds the automation token) and injected into the spawned agent's
environment as `<PROVIDER>_API_KEY`. Forge stacks keys comma-separated
(`GROQ_API_KEY="k1,k2"` doubles free-tier requests/min), and `credit_mode =
"strict"` in Forge's `[mesh]` keeps every route on free + subscription models
— the "$0 setup" the plan promised. `forge auth` remains the canonical,
OS-keyring way for keys used outside the editor.

## Reliability (both sides)

- **Forge's side:** completion gate (no "done" without an artifact + check),
  evidence ledger, doom-loop guards, per-provider failover, budget caps.
- **FUNCUT's side:** every edit is one undoable command or one atomic
  `edit_apply` batch; jobs report through events; `preview_frame` lets the
  agent *see* frames before claiming anything; the skill's review checklist
  gates the export.

## The 4 GB laptop

Forge is a single native binary (no Node/Python runtime of its own). FUNCUT
with Kokoro int8 and whisper `base.en` fits comfortably; Chatterbox (~1 GB) is
opt-in only. For heavy sessions: keep the window open with proxy scrubbing on
(it is by default), prefer `--mcp headless` for overnight queues, and stack two
or three free provider keys instead of paying for one.

## What is deliberately not here

- **Geneva motion cards** — later, after Forge is proven, exactly as the plan
  sequenced it.
- **Generative video APIs** (Kling/Runway/Veo) — optional and last-resort by
  design; stock search is free and instant.
- **A built-in LLM client** — the harness owns reasoning and provider
  routing; FUNCUT owns editing. The pane launches any MCP-capable harness,
  so Claude Code, opencode, or Codex CLI work with the same `.forge/mcp.toml`
  shape (their config importers take it as-is: `forge mcp import` exists for
  exactly this).
