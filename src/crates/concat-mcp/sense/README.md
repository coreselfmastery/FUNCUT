# concat-sense — giving the editor eyes

Two standard-library Python files, shipped inside the `concat-mcp` binary via
`include_str!`. They need **no pip install, no Python packages, no system-wide
installation**. Python 3 itself is required (Concat already needs it for
`analyze_scenes` / `transcribe`); set `FUNCUT_PYTHON` if it is not `python3`.

Decoding has two paths:

- **Timeline previews (no decoder at all).** `watch_render` asks the API's
  `preview.frame` for base64 PNGs and `sense.py` decodes them with its own
  pure-Python PNG reader (zlib + filters + nearest-neighbour fit). Concat
  links FFmpeg as a library and does **not** ship an ffmpeg CLI, so this path
  matters: it works on a stock install with zero downloads.
- **Rendered movie files.** If `ffmpeg` is on `PATH` (or `FUNCUT_FFMPEG` /
  `spec.ffmpeg` names it), it is used. Otherwise a pinned ~42 MB static build
  is bootstrapped once into the app data dir on Linux x86_64/arm64. On other
  platforms set `FUNCUT_FFMPEG` to an ffmpeg binary; the error message says
  exactly that. This same decoder supplies the audio energy envelope.

## Why this exists

Before this layer, the MCP server had exactly one tool (`preview_frame`) that
returned an MCP `type: image` content block. Everything else that produced
pixels (`motion_frame`, stock results, rendered exports) handed the agent a
**file path** — which a coding agent cannot see. The director was cutting
blind. `sense.py` closes that loop:

```
render / candidates  ->  decode real frames  ->  labelled contact sheet PNG
                     ->  pixel + audio metrics ->  base64 image over MCP
                     ->  local VLM (or honest heuristic) craft score
                     ->  concrete revisions -> edit -> render again
```

## Commands

All commands take `--spec '<json>'` and print one JSON object on stdout.
Diagnostics go to stderr, so stdout stays parseable.

| command | purpose |
| --- | --- |
| `sheet` | Decode frames into one labelled PNG contact sheet; measure each tile. |
| `critique` | Six-axis craft score + concrete revisions, from the sheet + brief. |
| `rank` | Semantically rank stock candidates against the creative brief. |
| `vlm` | `status` / `setup` for the local vision model. |
| `selftest` | Generate a synthetic PNG, decode it, measure it — no fixtures, no ffmpeg. |

Example:

```bash
python3 sense.py selftest
python3 sense.py sheet --spec '{"path":"render.mp4","samples":12,"audio":true}'
python3 sense.py critique --spec '{"sheet": <sheet json>, "brief": {...}, "autoSetup":true}'
python3 sense.py vlm --spec '{"action":"status"}'
```

`sense.py sheet` returns `pngB64` (base64 PNG), `tiles[]` with measured
`luma`, `contrast`, `detail`, `blackFraction`, `barRatio`, `cropHint`, a
perceptual `hash`, and `craft` aggregates plus plain-English `notes`.
`cropHint` is the frame's energy centroid — a real answer to "where should a
9:16 crop sit?" instead of always taking the centre column.

## The local VLM (`vlm.py`)

`vlm.py` is the "1B model for every PC, no install" path. It downloads, on
first use and only on request, into the app's own data directory:

- **`mini` (default)** — `ggml-org/SmolVLM2-500M-Video-Instruct-GGUF`,
  Q8_0 model (437 MB) + Q8_0 mmproj projector (109 MB). **0.5B parameters,
  ~546 MB total**, CPU-only, runs on a 4 GB laptop, and understands sequences
  of video frames natively.
- **`image`** — `ggml-org/SmolVLM-500M-Instruct-GGUF`, same size, tuned for
  single images.
- **`minicpm`** — `openbmb/MiniCPM-V-2-gguf`, Q2_K + f16 projector (~2.2 GB)
  for stronger text/chart reading on machines with more RAM.

The `llama-server` binary comes from the pinned `ggml-org/llama.cpp` release
`b11930` prebuilt CPU archives for Linux x64/arm64, macOS x64/arm64, and
Windows x64/arm64. Nothing is installed into the system; files live in
`$XDG_DATA_HOME/app.concat.editor/vlm` (or `FUNCUT_VLM_DIR`).

If the user already runs Ollama, LM Studio, LM Studio-compatible servers, or
any OpenAI-compatible endpoint, set `FUNCUT_VLM_ENDPOINT` and the download is
skipped entirely. `vlm.py` probes `FUNCUT_VLM_ENDPOINT`, `127.0.0.1:8099`,
`:8080`, `:1234` (OpenAI-compatible) and `:11434` (Ollama).

**No model is bundled in the repository.** The deterministic pixel/audio
scorer always works and is explicitly labelled `engine: "heuristic"` with
`confidence: 0.45` so the agent never mistakes a measurement for taste.

## Licensing note

SmolVLM2 is Apache-2.0; MiniCPM-V 2 is Apache-2.0; llama.cpp is MIT. The
OpenVid clips themselves come from a research dataset assembled from
Pixabay/Pexels/WebVid/CelebV and are **not** a blanket commercial licence.
`OpenvidClip.source` and `caption` are returned to the agent so it can make
an informed decision; see `docs/STOCK_LICENSING.md`.
