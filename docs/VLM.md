# The vision critic: with a VLM, without a VLM

FUNCUT's quality loop (`watch_render` → `craft_critique` → revise) works on
every machine, with or without a vision model. This page is the honest map of
the three rungs — what each one measures, what it costs, and how to move
between them.

| Rung | Engine label | Confidence | What judges the frames | Setup |
| --- | --- | --- | --- | --- |
| None | `heuristic` | 0.5 | Real pixel + audio measurements only — no model, nothing downloaded, nothing leaves the machine | Nothing to do |
| Bundled 0.5B | `local-vlm-flags` | 0.58 | The measurements, plus bounded visible-defect flags (black frames, bars, readable text), capped at ±1 point per axis | `local_vlm {action: "setup"}` once (~546 MB, CPU-only) |
| Strong model | `local-vlm` | 0.78 | Full six-axis VLM scoring: the model sees the contact sheet and the measured facts, its scores blend 70/30 with the measurements | A hosted free-tier provider (one `local_vlm` call) or any OpenAI-compatible server |

The critic an agent gets is automatic: `craft_critique` uses the best rung
that answers. Nothing here is mandatory — a machine with no VLM at all still
gets measured, honest scores.

## Without a VLM: what the heuristic measures

The measured baseline is real computation over the actual frames and audio,
not a guess. Every sampled frame passes through a 32×18 luma/colour grid:

- **Exposure**: mean luma, black frames (< 14 luma), overexposed frames
  (> 245 luma — clipped highlights that no edit can recover).
- **Contrast and detail**: per-frame std-dev and gradient energy; frames
  below the flat threshold read as dead air.
- **Letterbox/pillarbox bars**: near-black bands along the borders — the
  most common defect when 16:9 stock is dropped on a 9:16 canvas.
- **Colour**: mean saturation per frame and across the whole sheet —
  near-zero saturation reads grey unless monochrome is the point.
- **Duplicate shots**: a 64-bit difference hash sampled across the WHOLE
  frame (it used to cover only the top rows — two frames differing below
  the top border hashed identical).
- **Static transitions**: adjacent samples that barely differ; a piece of
  near-static transitions is a slideshow.
- **The opening frame**: the hook axis specifically judges the first
  sampled frame — a black, barred or flat opener costs it, wherever the
  rest of the piece goes.
- **Audio**: energy envelope, silence fraction, dynamic range, and EBU R128
  integrated LUFS / true peak / loudness range against the platform target.

Every axis score (`hook, story, rhythm, shotChoice, design, sound`) is a
formula over these facts, each revision names a real tool, and the reply
says plainly that no vision model reviewed the frames. The measurements
also feed the VLM rungs — the model sees them as `MEASURED FACTS`, so its
judgment stays anchored to what is actually on the timeline.

## The free-tier hosted providers

`local_vlm {action: "providers"}` lists them; `configure` wires one up.
Facts below checked 2026-10-28 — free tiers change, and the tool's reply is
regenerated from the same table, so trust it over this page when they
disagree.

| Provider | Endpoint | Default model | Free tier (honest) |
| --- | --- | --- | --- |
| `groq` | `https://api.groq.com/openai/v1` | `meta-llama/llama-4-scout-17b-16e-instruct` | No card needed; LPU-fast; roughly 30 req/min, limits are org-wide — [console.groq.com](https://console.groq.com/keys) shows the live numbers |
| `gemini` | `https://generativelanguage.googleapis.com/v1beta/openai` | `gemini-2.5-flash-lite` | Free tier covers the Flash models (Pro left the free tier in April 2026) — [aistudio.google.com](https://aistudio.google.com/apikey) shows the live quotas |
| `zai` | `https://api.z.ai/api/paas/v4` | `glm-4v-flash` | GLM vision flash models are served free — check [z.ai/model-api](https://z.ai/model-api) for the current model id and quota |
| `openrouter` | `https://openrouter.ai/api/v1` | `qwen/qwen2.5-vl-72b-instruct:free` | `:free` models cost nothing and rotate often — pick a vision one at [openrouter.ai/models](https://openrouter.ai/models) |
| `custom` | any OpenAI-compatible base URL | yours | Yours to know |

**Privacy, in bold because it matters: the local rungs never send a byte off
the machine. A hosted provider sends the contact-sheet frames — decoded
frames of your footage — to that provider's API.** That is the trade: the
hosted models are far better critics, and the frames they see are 12–24
sampled stills of the cut, not the source media. If the footage is
sensitive, stay local: the bundled 0.5B or your own Ollama / LM Studio /
llama.cpp server via `FUNCUT_VLM_ENDPOINT`.

### Configure one (one MCP call, no config file editing)

```
local_vlm {action: "configure", provider: "groq", apiKey: "gsk_..."}
```

The key is written to `vlm.json` inside the app's own data dir
(`~/.local/share/app.concat.editor/vlm/` on Linux, or `FUNCUT_VLM_DIR`
anywhere), every reply echoes it masked only, and `craft_critique`
immediately uses the full six-axis prompt. `local_vlm {action: "clear"}`
removes the config; `{action: "status"}` shows what is active, masked.

Model ids drift. If scores look odd, `{action: "providers"}` again or pass
`model` to `configure` with the id the provider's console currently serves.

### Environment variables instead of a config

| Variable | Effect |
| --- | --- |
| `FUNCUT_VLM_ENDPOINT` | Any OpenAI-compatible base URL — tried first, before the saved config |
| `FUNCUT_VLM_API_KEY` | Bearer key for that endpoint; beats the config file |
| `FUNCUT_VLM_MODEL` | Overrides the model id everywhere |
| `FUNCUT_VLM_STRONG=1` | Force the six-axis prompt even for a weak local endpoint (experimentation) |
| `FUNCUT_VLM_DIR` | Move the app's VLM data dir (models, config, logs) |

A provider's own exported key also works without configuration: export
`GROQ_API_KEY` (or `GEMINI_API_KEY`, `ZAI_API_KEY`, `OPENROUTER_API_KEY`)
and run `local_vlm {action: "configure", provider: "groq"}` with no
`apiKey` — the key is picked up from the environment and saved.

## How the engines choose

`detect()` resolves the endpoint in this order: `FUNCUT_VLM_ENDPOINT` →
the saved `vlm.json` config → a local server on :8099 / :8080 / :1234 →
Ollama on :11434. A keyless entry must answer its probe (a dead endpoint
falls through, as it always did); an entry with a key is trusted even when
the probe fails, so `craft_critique` surfaces the provider's own 401
instead of silently dropping to pixels. A hosted entry (any keyed,
non-loopback endpoint) unlocks the six-axis prompt on its own — those
models are the class the prompt was written for. The bundled 0.5B never
gets it: in the end-to-end test it hallucinated a whole film from a plain
blue frame, so it only ever applies bounded, explained nudges.

## What the strong critic is good for, and what it is not

The six-axis model scores blend 70/30 with the measurements, degenerate
replies (one repeated value, all zeros, all tens) are rejected outright,
and the sound axis is judged from measured LUFS facts, never guessed from
the picture. A strong model is a real taste upgrade for shot choice,
design and story — but it is still one model's opinion on 12–24 sampled
frames. The measurements stay in every reply (`measured`), the quality
loop still requires `watch_render` on the exported file, and the honest
score is the one you read off the final contact sheet.
