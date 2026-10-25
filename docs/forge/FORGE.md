# Forge: set up an AI director for FUNCUT

Forge (<https://github.com/Adulari/forge>) is the harness; FUNCUT is the
engine; the `funcut-director` skill in this repo is the brain. This page gets
you from nothing to an agent editing a real video in your open window — and
then to overnight queues — on a free tier.

## 1. Install Forge (one line)

```bash
curl -fsSL https://raw.githubusercontent.com/Adulari/forge/main/install.sh | sh
```

That puts `forge` in `~/.local/bin` (Linux x86-64/ARM64, macOS Apple Silicon;
Windows builds exist too). It is a single Rust binary — no Node, no Python.

## 2. Give it free keys

Pick one or two free providers; more keys = more requests/min (Forge
round-robins comma-separated keys: `GROQ_API_KEY="k1,k2"`).

```bash
forge init            # the wizard walks you through providers
# or, later:
forge auth groq       # key from https://console.groq.com/keys (free, ~30 RPM)
forge auth cerebras   # https://cloud.cerebras.ai (free, ~14.4k/day)
forge auth sambanova  # https://cloud.sambanova.ai (free, ~20 RPM)
```

Keys land in the OS keyring, never in a dotfile. To hard-stop paid routing,
put this in `~/.config/forge/config.toml`:

```toml
[mesh]
credit_mode = "strict"   # free + subscription models only
daily_budget_usd = 1.0   # and a cap, if you want a second belt
```

The Forge pane's Settings page (Settings > Forge) can also hold keys: they are
injected into the spawned agent's environment when you press Start, so the
pane works even without `forge auth`.

## 3. Connect Forge to FUNCUT

The window must be serving the API once: **FUNCUT > Settings > Automation >
Scripts and apps** on. Then either, once:

```bash
forge mcp add funcut concat -- --mcp window -s user
```

or run Forge from inside this repository — the repo's own `.forge/mcp.toml`
already declares the server (and a 900 s call timeout, because transcription
and analysis take longer than the default 60 s):

```toml
# .forge/mcp.toml (this repo)
call_timeout_secs = 900

[[servers]]
name = "funcut"
[servers.transport]
type = "stdio"
command = "concat"
args = ["--mcp", "auto"]   # window when open, headless otherwise
```

Check it: `forge mcp` should list `funcut` as connected, with the tool count.
For overnight/batch work the pane and headless mode cover everything; nothing
needs the window.

*(Other harnesses work too — the same server, other configs. Claude Code:
`claude mcp add funcut concat -- --mcp window` or a `.mcp.json` with the
`mcpServers` shape; `forge mcp import` reads existing configs from Claude
Code, Codex, Cursor, Windsurf and VS Code.)*

## 4. Install the director's brain

The skill ships in this repo at `.claude/skills/funcut-director/`. Install it
into Forge from GitHub:

```bash
forge skill install coreselfmastery/FUNCUT
```

(Claude Code picks it up automatically when working inside this repo; Forge
also has `forge import claude` for skills you already have there.) Verify:
`forge commands` lists `funcut-director`.

## 5. First run

Ask for a whole faceless video, end to end — the skill drives the pipeline:

```bash
forge run "Using the funcut-director skill, make a 30-second faceless video about why the sea is salty: script from the hook rules, Kokoro voice 8 narration, dead-air removed, B-roll from OpenVid, one SFX per cut on the beat, word-synced bold captions, teal-orange grade, verify with preview_frame, then export."
```

Or do it from the editor: the **Forge** tab (12th) — paste the script, pick a
niche preset, voice and music mood, press **Start Forge**. Every line the agent
prints streams into the pane; the edits land on the open project's timeline,
undoable, while you watch. `funcut-director` finds tools through Forge's
deferred discovery (`mcp_search_tools` then `mcp_call funcut__clip_trim`,
`funcut__openvid_search`, …) and follows the plan-first pipeline:

```
script → tts_generate → transcribe → analyze_silence → clip_cut_spans
  → analyze_beats → openvid/bgm/sounds searches → edit plan (one pass)
  → edit_apply batches → captions_add → preview_frame checks → export_start
```

## 6. Overnight

Queue episodes and let it drain while you sleep:

```bash
forge queue add "Make episode 3 of the luxury objects series with the funcut-director skill"
forge queue add "…episode 4…"
forge queue run          # headless, isolated worktrees, budget-capped
```

`forge schedule` can fire the same prompt weekly (systemd timers), and
`forge run` inside `cron` works too. Headless mode writes projects under your
home (or `CONCAT_MCP_ROOTS`), never touching the window's open projects.

## 7. Troubleshooting

| Symptom | Fix |
| --- | --- |
| `forge: command not found` | `~/.local/bin` not on PATH: `export PATH="$HOME/.local/bin:$PATH"` |
| `forge mcp` shows funcut "failed: the Concat window is not serving the API" | Open FUNCUT > Settings > Automation > turn on **Scripts and apps**; or use `--mcp headless` |
| `analysis.scenes` returns "python3 not found" / a pip hint | `pip install -r ~/.local/share/app.concat.editor/analysis/requirements.txt` (scenedetect, opencv-headless, librosa) |
| A tool call times out | This repo's `.forge/mcp.toml` sets `call_timeout_secs = 900`; if you declared the server yourself, raise yours (default 60) |
| `transcribe` says a model is not downloaded | Settings > Transcriber, download `base.en` (fast, English) or `small` (multilingual) |
| `tts_generate` says a model is not downloaded | Settings > Speech, download Kokoro int8 (the 4 GB-laptop choice) |
| Agent "done" but the video looks wrong | The skill's review checklist exists for this: run `preview_frame` at start/middle/end, check gaps and overlaps, fix, re-verify. Never accept "done" without an export that `export_wait` says succeeded |
| Low memory with big projects | Close other apps; keep proxy scrubbing on; use headless for overnight; prefer Kokoro int8 over Chatterbox |

## 8. What the agent can call (67 tools)

Everything in `docs/MCP.md`, plus the six new ones: `analyze_scenes`
(PySceneDetect cut points), `analyze_beats` (librosa BPM/beat grid),
`analyze_silence` (dead-air spans), `analysis_wait` (job results inline),
`transcribe` (whisper words with times), `tts_generate` (Kokoro/Pocket/
Chatterbox narration WAV, imported straight into the project).
