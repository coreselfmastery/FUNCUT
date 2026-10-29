# concat-sense — giving the editor eyes and a feel for the voice

Three standard-library Python files, shipped inside the `concat-mcp` binary
via `include_str!`. They need **no pip install, no Python packages, no
system-wide installation**. Python 3 itself is required (Concat already
needs it for `analyze_scenes` / `transcribe`); set `FUNCUT_PYTHON` if it is
not `python3`.

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

All commands take `--spec '<json>'` (or `--spec -`, which reads the spec
from standard input - the form the Rust layer uses, because a spec carrying
real frames is megabytes and Linux caps one argv string at 128 KiB) and
print one JSON object on stdout. Diagnostics go to stderr, so stdout stays
parseable.

| command | purpose |
| --- | --- |
| `sheet` | Decode frames into one labelled PNG contact sheet; measure each tile. |
| `critique` | Six-axis craft score + concrete revisions, from the sheet + brief. |
| `rank` | Semantically rank stock candidates against the creative brief. |
| `overlay` | Composite a baked RGBA PNG sequence over a plate video (or a solid colour) into an H.264 clip, honouring alpha at the compositing step. |
| `loudness` | EBU R128 integrated LUFS, true peak dBTP and loudness range of a media file, checked against a platform target. |
| `rhythm` | BPM, energy-onset curve and beat-aligned cut times spread across the whole piece, with no Python extras. |
| `vlm` | `status` / `setup` for the local vision model. |
| `emotion` | The emotional read of the voiceover: per-line emotion, act, easing, colour, duck, vocabulary + the arc; `brand` / `gate` / `verify` / `template` beside it. |
| `selftest` | Generate a synthetic PNG, decode it, measure it, and exercise the model-reply, loudness and cut-spread parsers — no fixtures; the beat path (onset curve, click-track BPM) runs when an ffmpeg is on PATH. `vlm.py selftest` and `emotion.py selftest` check the critic and the reader the same way, and CI runs all three. |

Example:

```bash
python3 sense.py selftest
python3 sense.py sheet --spec '{"path":"render.mp4","samples":12,"audio":true}'
python3 sense.py critique --spec '{"sheet": <sheet json>, "brief": {...}, "autoSetup":true}'
python3 sense.py vlm --spec '{"action":"status"}'
python3 sense.py emotion --spec '{"script":"You won't believe what they found."}'
cat big-spec.json | python3 sense.py sheet --spec -
```

`sense.py sheet` returns `pngB64` (base64 PNG), `tiles[]` with measured
`luma`, `contrast`, `detail`, `blackFraction`, `barRatio`, `cropHint`, a
perceptual `hash`, and `craft` aggregates plus plain-English `notes`.
`cropHint` is the frame's energy centroid — a real answer to "where should a
9:16 crop sit?" instead of always taking the centre column.

### Honest capability of the bundled 0.5B critic

Tested end-to-end on 2026-10-27 with the real
`SmolVLM2-500M-Video-Instruct-Q8_0` model and llama.cpp `b11930`: setup,
health, image inference and grammar-constrained JSON all work. But given a
long six-axis critique prompt the model **hallucinated an entire film from a
plain blue frame** and returned nested string "scores". It cannot grade
craft. `craft_critique` therefore uses it only as a bounded **visible-defect
flag detector** (`engine: "local-vlm-flags"`, confidence 0.58, at most
±1.0 per axis, with strict schema validation and degenerate-answer
rejection). Full six-axis scoring (`engine: "local-vlm"`) happens only when
`FUNCUT_VLM_ENDPOINT` names a stronger user-supplied model. `rank` got the
same short-prompt/schema treatment and falls back cleanly.

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
skipped entirely. `vlm.py` resolves the endpoint in this order:
`FUNCUT_VLM_ENDPOINT`, the saved hosted-provider config (`vlm.json` in the
data dir, written by the `local_vlm {action: "configure"}` MCP tool),
`127.0.0.1:8099`, `:8080`, `:1234` (OpenAI-compatible) and `:11434`
(Ollama).

## Hosted free-tier providers (`configure` / `providers`)

`vlm.py` also speaks to hosted vision APIs — the free tiers of groq
(`https://api.groq.com/openai/v1`, default `meta-llama/llama-4-scout-17b-16e-instruct`),
gemini (`https://generativelanguage.googleapis.com/v1beta/openai`,
`gemini-2.5-flash-lite`), zai (`https://api.z.ai/api/paas/v4`,
`glm-4v-flash`) and openrouter (`https://openrouter.ai/api/v1`,
`qwen/qwen2.5-vl-72b-instruct:free`). `FUNCUT_VLM_API_KEY` (or the
provider's own env var — `GROQ_API_KEY`, `GEMINI_API_KEY`, `ZAI_API_KEY`,
`OPENROUTER_API_KEY`) is sent as a Bearer token, which is the only thing a
hosted endpoint needs beyond the base URL; `FUNCUT_VLM_MODEL` overrides the
model id. A keyed entry is trusted even when its probe fails, so `ask()`
surfaces the provider's own 401 instead of silently dropping to pixels, and
a hosted entry unlocks the full six-axis prompt by itself. **A hosted
provider means the contact-sheet frames are sent to that provider's API** —
the local rungs never leave the machine. See `docs/VLM.md`.

**No model is bundled in the repository.** The deterministic pixel/audio
scorer always works and is explicitly labelled `engine: "heuristic"` with
`confidence: 0.5` so the agent never mistakes a measurement for taste. It
measures exposure (black AND blown-out frames), contrast, detail,
letterbox/pillarbox bars, colour saturation, full-frame duplicate hashing,
static transitions, the opening frame (the hook axis), the audio envelope
and EBU R128 loudness — every axis score is a formula over those facts.

## The emotion read (`emotion.py`)

`emotion.py` is the third sense: what the voiceover **feels like**, measured
from the words themselves. The director pipeline had eyes (the sheets) and
hands (the edit tools); the edit was driven by structure alone. This file
adds the missing driver — every line of narration read as an emotion, an
intensity, an act and a viewer state, and each of those mapped to what the
edit actually does: the easing curve (tense eases in, warm eases out, a
burst pops), the colour temperature, the SFX register, the BGM duck depth,
the TTS delivery and the stock-query vocabulary.

Two engines, both honest about what they are: a **stem lexicon** (always
available, zero dependencies, confidence 0.55, English-only) and the hosted
`tabularisai/multilingual-emotion-classification` text classifier
(XLM-RoBERTa across 23 languages — Bengali, Hindi, Urdu, Arabic, Chinese,
Spanish and more — its 11 labels folded onto the edit's 7) on
HuggingFace's free Inference API when `FUNCUT_EMOTION_API_KEY` / `HF_TOKEN`
is set (75/25 blend with the lexicon, confidence 0.85 — the model is the
read, multilingual and semantic; the lexicon keeps a quarter for the
punctuation and caps the model never will). The pipeline never *requires*
the key; a non-English script without one is answered with an honest note
rather than a silent run of neutral lines.

Commands (same `--spec` contract): `extract` (per-line read + the arc),
`brand` (validate and apply a brand guide), `gate` (the placement gate:
0–1 failed checks passes, 2+ rejects, all rejected = DROP THE BEAT),
`verify` (word-level alignment of the edit against the re-transcribed
export), `template` (a ready-to-edit brand guide) and `selftest`
(deterministic, no network). The MCP tool is `emotion`; see
`docs/EMOTION.md` for the full map.

## Licensing note

SmolVLM2 is Apache-2.0; MiniCPM-V 2 is Apache-2.0; llama.cpp is MIT. The
OpenVid clips themselves come from a research dataset assembled from
Pixabay/Pexels/WebVid/CelebV and are **not** a blanket commercial licence.
`OpenvidClip.source` and `caption` are returned to the agent so it can make
an informed decision; see `docs/STOCK_LICENSING.md`.


## Why `overlay` exists

The motion sidecar's encoded videos are opaque. VP9-with-alpha and ProRes
4444 were tested and came back with every alpha byte at 255; FFmpeg's WebM
muxer does not reliably write `AlphaMode` (a `yuva420p` encode round-tripped
as `yuv420p`). Rather than ship a feature that silently discards
transparency, `overlay` composites the RGBA PNG bake over the *actual*
footage using ffmpeg's `overlay` filter, with `setpts` for the start offset
and an `enable` window for the comp's duration. The output is an ordinary
H.264 clip that the existing `media_import` / `clip_add` path understands.
The original alpha sequence is left untouched, so one bake can be
re-composited over many plates.


## Loudness is measured, not promised

`watch_render {path, audio:true}` and the `measure_loudness` tool run
FFmpeg's `ebur128` meter and report **integrated LUFS, true peak dBTP and
loudness range LU**, the error against the target, and whether the target
was met. Short-form platforms sit near −14 LUFS with a true peak below
−1 dBTP; broadcast EBU R128 is −23 LUFS. `export_wait` reports no
loudness, so a plan's target used to be the only number anyone had —
which is not proof of compliance. `craft_critique`'s `sound` score now
uses the real measurement, and `measure_loudness` gives a headless
check on any exported file.


## The revision journal

`craft_critique` with a `project` writes `<project>/craft-history.json`
(last 20 passes: engine, overall, the six scores, notes and revisions) and
returns `pass`, `previousOverall`, `delta`, `improved` and a `trend`
sentence. An agent in a later turn can therefore see whether its revision
actually helped. `delta <= 0` means "undo or rethink", not "try again with
a nicer prompt".


## Rhythm without librosa

`analyze_beats` needs the optional Python analysis extras. `suggest_cuts`
(and `sense.py rhythm`) works with ffmpeg alone: decode 8 kHz mono PCM,
take the positive short-time energy derivative as an onset function,
autocorrelate it over 60–200 BPM, then pick the strongest onset in each
time bucket so the cuts cover the WHOLE piece rather than clustering in
one loud chorus. On a synthetic 120 BPM kick track it reports 120.0 BPM
and returns ten cut times spread from 0.45 s to 7.45 s. It is an honest
approximation, not a claim of librosa-grade tracking — and the tool
explicitly tells the agent to keep a few cuts off the pulse.
