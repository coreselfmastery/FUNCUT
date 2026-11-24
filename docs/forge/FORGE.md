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

The Forge page itself can also hold keys: they are injected into the spawned
agent's environment when you press Start, so the pane works even without
`forge auth`.

## 3. Connect Forge to FUNCUT

The window must be serving the API once: **FUNCUT > Settings > Automation >
Scripts and apps** on. Then either, once:

```bash
forge mcp add funcut concat -- --mcp window -s user
```

or run Forge from inside this repository — the repo's own `.forge/mcp.toml`
already declares the server (and a 1200 s call timeout, because the deep
tools - transcription, the sense families, motion reels - are budgeted up
to 1200 s server-side), with the per-tool allowlist already active in the
server's `env`: the director's 106-tool surface, every tool but the eight
an agent must never call by surprise - `project_close` and `export_cancel`
among them, plus the diagnostics and farm doors the director has no use
for:

```toml
# .forge/mcp.toml (this repo)
call_timeout_secs = 1200

[[servers]]
name = "funcut"
[servers.transport]
type = "stdio"
command = "concat"
args = ["--mcp", "auto"]   # window when open, headless otherwise
[servers.transport.env]
CONCAT_MCP_ALLOW = "analysis_wait,…,watch_render"   # 106 of the 114 tools
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

A second skill ships beside it: **maestro** (`.claude/skills/maestro/`), a
design, motion and video judgment library — 28 reference modules plus ten
vendored corpora, including a 157-card product-shot vocabulary,
engine-agnostic motion principles, and a sound-design and beat-sync method.
The same `forge skill install coreselfmastery/FUNCUT` picks it up, and the
director's skill routes into it phase by phase (its §6; the full map is
[docs/MAESTRO.md](../MAESTRO.md)).

A third skill ships beside them: **clip-skills**
(`.claude/skills/clip-skills/`), the editing-craft core — 15
software-agnostic knowledge modules (audio-driven cutting, A/B-roll
matching, pacing, grading workflow, keyframes, speed ramps, keying,
tracking, multicam, subtitles, transitions, voice processing, export
numbers) plus a FUNCUT execution guide that maps every module to this
editor's MCP tools. Maestro judges what looks good; clip-skills judges how
to cut. The same install picks it up; the director's skill routes into it
phase by phase (its §7; the full map is
[docs/CLIPSKILLS.md](../CLIPSKILLS.md)).

## 5. First run

Ask for a whole faceless video, end to end — the skill drives the pipeline:

```bash
forge run "Using the funcut-director skill, make a 30-second faceless video about why the sea is salty: script from the hook rules, Kokoro voice 8 narration, dead-air removed, B-roll from OpenVid chosen against the brief's avoid list, the numbers and comparisons as motion comps (ticker, chart, split) rather than plain text, SFX on the cuts that carry a beat with one cut in four left dry, readable segment-timed bold captions, teal-orange grade, verify with watch_render and craft_critique, revise once, then export."
```

Or do it from the editor: the **Forge** page (page 12, beside Stickers) — paste the script, pick a
niche preset, voice and music mood, press **Start Forge**. Every line the agent
prints streams into the pane. A run whose narration comes from the timeline's
own voiceover edits a sibling of it, and **the window follows that edit live**:
the timeline becomes the run's project while the Director works - every landed
edit visible as it happens, B-roll arriving, captions appearing, motion cards
placed - read-only while the run owns the document, and when the run ends well
the finished edit opens on the timeline for real, with a claim, an undo
history and a place in the recents. A run that fails or is cancelled hands the
window back the project it held, saved. `funcut-director` finds tools through Forge's
deferred discovery (`mcp_search_tools` then `mcp_call funcut__clip_trim`,
`funcut__openvid_search`, …) and follows the plan-first pipeline:

```
script → tts_generate → transcribe → analyze_silence → clip_cut_spans
  → analyze_beats → openvid/bgm/sounds searches → edit plan (one pass)
  → edit_apply batches → captions_add → preview_frame checks → export_start
```

### AUTO EDIT: the same Director, one click

Beside **Start Forge** sits **Auto edit**. It is not a second editor and
not a second Director: it is the *same* Director agent, started
autonomously.

It is enabled the moment the open timeline carries a voiceover (audio
clips). One click builds the same sibling project a voiceover forge run
edits - the narration reconstructed clip for clip beside the source - and
then hands that sibling to the Director:

```
AUTO EDIT
  -> sibling project (narration reconstructed, mapping written)
  -> the window follows the sibling LIVE: the timeline shows the edit
       as the Director makes it, read-only, proxies built as media lands
  -> the Director (`forge run`, same skill, same keys, same MCP tools)
       understands the footage and the voice
       plans, validates the plan (plan_validate: valid + creativeReady)
       cuts, gathers B-roll, builds motion, captions, grades, scores music
       watch_render -> craft_critique -> revises -> watch_render again
       saves the editable project, reads the timeline back and checks every lane
       watch_render -> craft_critique on the final project preview
  -> the finished edit opens on the timeline (the server lets the
       project go and the window claims it, undoable from the first edit)
```

The Director receives the sibling the button built, with the narration
already placed clip for clip. Nothing is measured for it: the prompt
states that the repair, the transcription and the voice-signal/emotion
reads are the Director's own to run, in the skill's order, and that a
decision which never read the voice is a guess.

Progress is read from the harness's own output. Each line that names one
of the skill's twenty-five phase headings (1 through 25) moves the pane's
stage and bar to that phase - and only forward; a
line that names no phase moves nothing, so the bar cannot show work the
Director has not reached. The stop button kills the harness at
once.

#### When the Director is not available

`Auto edit` hands over only when all of these hold:

| Needs | Where the user sets it |
| --- | --- |
| `forge` on `PATH` | install the harness |
| The harness wired to this editor | **Connect** on the Forge page |
| The `funcut-director` skill installed | **Install** on the Forge page |
| A model key saved | one of the eight LLM provider keys on the Forge page |
| The editor's MCP surface serves the run's tools | `CONCAT_MCP_ALLOW` (the default 106-tool director surface already does) |

If one is missing the button **says which**, and the run falls back to the
deterministic spine - the same seventeen-stage program a handed-over run's
measurements stand on, run unattended: repair, transcribe, voice signals,
the understand reads (music, discourse, emotion), the plan and its
validation, the tighten and the grade, licensed live-stock B-roll
(searched, seen on a contact sheet, continuity-checked, imported and
trimmed), segment-timed captions with an accent pass, motion comps baked
from the emotion's own register, the music bed shopped, trimmed to the cut
and ducked under the voice, SFX accents on the measured emphasis peaks,
preview, revise, export, verify (the render re-transcribed, its loudness
measured, its words compared, its timeline checked for collisions) and the
watch that scores the shipped file - so a lesser edit is never presented
as the Director's:

```
open the sibling -> repair the take (enhance + clean, landing on the
  file the timeline plays) -> transcribe -> voice signals ->
  understand (music gate, discourse, emotion) -> plan + plan_validate
  -> tighten + grade -> gather B-roll (live stock: search, contact
  sheet, continuity, import) -> captions -> motion -> music bed ->
  SFX accents -> preview -> revise -> export -> verify -> watch + score
```

The fallback's own progress is the runner's: the stage, the clock, the
fraction of the program's stages - and the timeline follows the edit live
on this path too. A green toast and the finished edit
opens on the timeline; a **yellow** toast means it shipped with warnings
written down - `auto-edit-report.json`, beside the edit, carries each
one; a red toast names the failure and the button answers with **Retry**
or **Fix and restart**.

One run at a time: Start Forge and Auto edit share one sibling and one
server, so each refuses to start while the other is going.

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
| A tool call times out | This repo's `.forge/mcp.toml` sets `call_timeout_secs = 1200`; if you declared the server yourself, raise yours (default 60) |
| `transcribe` says a model is not downloaded | Settings > Transcriber, download `base` (multilingual default, auto-detects language) or `small` for a non-English take |
| `tts_generate` says a model is not downloaded | Settings > Speech, download Kokoro int8 (the 4 GB-laptop choice) |
| Agent "done" but the video looks wrong | The skill's quality loop is mandatory: `watch_render` (whole cut as one image) -> `craft_critique` (six craft scores) -> revise at least once -> `watch_render` again -> `watch_render {path}` on the exported file. `plan_validate` must also say `creativeReady: true`. Never accept "done" without an export `export_wait` says succeeded AND a measured score >= 7 |
| Low memory with big projects | Close other apps; keep proxy scrubbing on; use headless for overnight; prefer Kokoro int8 over Chatterbox |

## 8. What the agent can call (the full MCP catalogue)

Everything in `docs/MCP.md` — the projects, media, cuts, captions, effects, motion, export and analysis tools, plus the five **sense** tools (`creative_brief`, `stock_contact_sheet`, `watch_render`, `craft_critique`, `local_vlm`) that let the agent see and score its own work. Six analysis/speech tools:
`analyze_scenes` (PySceneDetect cut points), `analyze_beats` (librosa BPM/beat
grid), `analyze_silence` (dead-air spans), `analysis_wait` (job results
inline), `transcribe` (Whisper text segments with start/end times and measured
per-word timing inside each), and `tts_generate` (Kokoro/Pocket/Chatterbox narration WAV,
imported straight into the project). The director workflow also has
`plan_validate` and `clip_audio_levels_db` for pre-edit checks and unit-safe
volume automation.


## 9. Picking the director model

The pane used to call `forge run` with no `--model`, so Forge's cheapest
capable router chose the model. That is wrong for this job: the mandatory
quality loop returns **images**, and a text-only model cannot see a contact
sheet. Set a vision-capable model before a serious run:

```sh
export FUNCUT_FORGE_MODEL="your-vision-capable-model-id"
```

`FORGE_MODEL` is accepted as an alias. When neither is set, the pane keeps
Forge's default routing and says nothing about it — check that your model can
see images before trusting a `craft_critique` result.

The run prompt itself now carries the project's persisted creative brief
(`creative-brief.json`), demands `plan_validate` return `creativeReady: true`,
requires `stock_contact_sheet` before choosing B-roll, and requires
`watch_render` → `craft_critique` → revise → `watch_render` → export →
`watch_render {path}` with an honest final score. If no brief is saved, the
first instruction is to write one.


## 10. The vision critic (optional — local, or a free hosted tier)

`craft_critique` always works: without a model it returns measured,
explicitly-labelled pixel/audio scores (`engine: "heuristic"`, confidence 0.5
— black frames, bars, flat and overexposed shots, duplicates, static
transitions, colour saturation and the opening frame, all measured from real
pixels). To add visible-defect detection, call `local_vlm {action:"setup"}`
once. Be aware that a 0.5B model cannot judge taste — it was tested
end-to-end and hallucinated a whole film from a plain blue frame — so it only
applies bounded flags. It downloads a 0.5B
SmolVLM2-500M-Video-Instruct Q8_0 model + projector (~546 MB) and a pinned
llama.cpp CPU server into the app's own data folder, then serves on
127.0.0.1:8099. Works on a 4 GB laptop; nothing is installed system-wide.
If you already run Ollama or LM Studio with a vision model, set
`FUNCUT_VLM_ENDPOINT` instead and skip the download. The heavier
`profile: "minicpm"` (~2.2 GB) reads dense text and charts better.

For real taste there is now a third rung, and it is free: the hosted
free-tier vision APIs. One call wires one up —

```json
local_vlm {"action": "configure", "provider": "groq", "apiKey": "gsk_..."}
```

— and `craft_critique` immediately scores with the full six-axis model
prompt (`engine: "local-vlm"`, confidence 0.78). Providers: `groq`
(llama-4-scout, ~30 req/min), `gemini` (Flash models on the free tier),
`zai` (GLM vision flash), `openrouter` (`:free` vision models) —
`local_vlm {action:"providers"}` lists the live table. The key is stored in
`vlm.json` in the app's own data dir and masked in every reply. **A hosted
provider sends the contact-sheet frames to that provider's API** — 12-24
sampled stills of the cut, not the source media; the local rungs never
leave the machine. The full map, with the privacy trade spelled out, is
[docs/VLM.md](../VLM.md).
