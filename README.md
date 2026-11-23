<p align="center">
  <img src="assets/concat-mark-dark.png" alt="Concat" width="100" />
</p>

<h1 align="center">FUNCUT</h1>
<h3 align="center">The free, open-source CapCut replacement for Linux — with a native 3D motion engine and an agent stack.</h3>

<div align="center">

<a href="https://github.com/coreselfmastery/FUNCUT/releases"><img src="https://img.shields.io/github/downloads/coreselfmastery/FUNCUT/total?style=flat&logo=github&logoColor=F8F8F8&label=Downloads&labelColor=212123&color=0568FD" alt="Total Downloads" /></a>
<a href="https://github.com/coreselfmastery/FUNCUT/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/coreselfmastery/FUNCUT/ci.yml?style=flat&logo=githubactions&logoColor=F8F8F8&label=Build&labelColor=000000" alt="Build Status" /></a>
<a href="https://github.com/coreselfmastery/FUNCUT/releases"><img src="https://img.shields.io/badge/Version-0.2.8--funcut.15-0568FD?style=flat&logo=semver&logoColor=F8F8F8&labelColor=212123" alt="FUNCUT Version 0.2.8-funcut.15" /></a>
<img src="https://img.shields.io/badge/Native_3D-CPU-7C3AED?style=flat&logo=three.js&logoColor=F8F8F8&labelColor=212123" alt="Native CPU 3D" />
<img src="https://img.shields.io/badge/MCP-114_tools-0EA5E9?style=flat&logo=json&logoColor=F8F8F8&labelColor=212123" alt="114 MCP tools" />
<a href="https://discord.gg/DVuPfpXfqP"><img src="https://img.shields.io/badge/Discord-Join%20the%20server-5865F2?style=flat&logo=discord&logoColor=F8F8F8&labelColor=000000" alt="Join Concat Discord" /></a>
<a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-0568FD?style=flat&logo=gnu&logoColor=F8F8F8&labelColor=212123" alt="License: AGPL-3.0-or-later" /></a>

**🇬🇧 English** · [🇩🇪 Deutsch](docs/README.de.md) · [🇪🇸 Español](docs/README.es.md) · [🇫🇷 Français](docs/README.fr.md) · [🇮🇹 Italiano](docs/README.it.md) · [🇳🇱 Nederlands](docs/README.nl.md) · [🇵🇱 Polski](docs/README.pl.md) · [🇧🇷 Português (Brasil)](docs/README.pt-BR.md) · [🇷🇺 Русский](docs/README.ru.md) · [🇺🇦 Українська](docs/README.uk.md) · [🇹🇷 Türkçe](docs/README.tr.md) · [🇦🇿 Azərbaycanca](docs/README.az.md) · [🇭🇷 Hrvatski](docs/README.hr.md) · [🇮🇩 Bahasa Indonesia](docs/README.id.md) · [🇻🇳 Tiếng Việt](docs/README.vi.md) · [🇹🇭 ไทย](docs/README.th.md) · [🇯🇵 日本語](docs/README.ja.md) · [🇰🇷 한국어](docs/README.ko.md) · [🇨🇳 简体中文](docs/README.zh-Hans.md) · [🇹🇼 繁體中文](docs/README.zh-TW.md) · [🇸🇦 العربية](docs/README.ar.md) · [🇮🇱 עברית](docs/README.he.md) · [🇮🇷 فارسی](docs/README.fa.md) · [🇮🇳 हिन्दी](docs/README.hi.md) · [🇵🇰 اردو](docs/README.ur.md) · [🇧🇩 বাংলা](docs/README.bn.md)

<sub>Translations may lag the English README.</sub>

<img src="assets/editor.png" alt="FUNCUT editor" width="100%" />

</div>

> [!NOTE]
> **FUNCUT is a fork of [Concat](https://github.com/jub0t/Concat)** — the native Rust + Slint video editor — tuned for small, low-memory Linux laptops (4–6 GB RAM), and extended with a full agent stack: a built-in MCP server with **114 tools**, a 25-phase director skill, emotion-driven editing, a motion-graphics engine with a **native CPU 3D scene layer**, local stock/music/sound/sticker shelves, and a Forge page that starts a whole faceless video with one click.
>
> It is **Linux-only** (x86_64 and aarch64). No account. No watermark. No upload. Your footage stays on your disk.

---

## At a glance

| What | What you get |
|---|---|
| **Editor** | Multi-track, multi-timeline cutting, 55 commands, keyframes with curves, 327 effects/filters/transitions/audio packages, titles, captions, masks, stabilize, enhance, HDR/4K export |
| **3D** | Native CPU 3D scenes: box, plane, sphere, cylinder, torus; ambient/directional/point lights; camera flight path; per-pixel Phong; no GPU and no browser |
| **Motion** | 33 presets and 30 authoring ops: lower thirds, kinetic captions, charts, counters, code, devices, maps, social cards, 3D showcase |
| **AUTO EDIT** | One click or one MCP call: voice repair, transcription, voice signals, emotion, plan, picture, B-roll, captions, motion, music, SFX, preview, revise, save, verify |
| **Local AI** | Whisper transcription, Kokoro/Pocket/Chatterbox TTS, voice cloning, MediaPipe/RVM/IS-Net/SlimSAM cutout, Real-ESRGAN enhance |
| **Agent stack** | 114 MCP tools (106 served to a director by default), 25-phase director skill, maestro and clip-skills judgment shelves, Forge harness |
| **Libraries** | 937,957 OpenVid clips, 1,025 CC0 + 166 MobyGratis tracks, 530 CC0 sounds, 1,285 stickers, 41 fonts |
| **Small machines** | Caches size themselves to RAM; proxies, decode ladder, measured 4 GB editing; 3D measured at ~0.7 s per 1080p frame |
| **Export** | H.264, HEVC, AV1; up to 4K 60; 10-bit; HDR HLG/PQ; NVENC/QSV/AMF; faststart MP4 |

---

## What's new in `0.2.8-funcut.15` — 23 November 2026

`.15` is `.14` plus the native 3D scene layer.

### Native CPU 3D scenes

A comp can carry `NodeKind::Scene3D`:

- **Five geometries:** box, plane, sphere, cylinder, torus.
- **Per-object:** position, rotation, scale, spin, colour, metalness, roughness, emissive.
- **Three lights:** ambient, directional, point.
- **Camera:** the comp’s own `Camera3D` path — `at`, `x`, `y`, `z`, plus optional pitch, yaw, roll and fov stops.
- **Renderer:** CPU z-buffer, per-pixel Phong, near-plane clipping, point lights, specular from the real eye.
- **No GPU path. No browser. No JavaScript runtime.**

Two authoring ops open it: `add-scene3d` and `set-scene3d`. The preset shelf grew to **33** with `funcut.scene3d.showcase`; the authoring vocabulary grew to **30** ops.

Measured on the reference CPU-only box: **~0.7 seconds per 1920×1080 frame** for the four-object showcase. An 8-second, 30 fps scene is roughly **2.8 minutes**; `samples: 2` costs about **4×** that. Keep scenes to a few dozen objects at 1080p.

Engine routing is explicit: a comp whose only engine-specific feature is `scene3d` resolves to **native** even when the Geneva sidecar is installed; a comp that also carries markup keeps **Geneva** and warns that it cannot draw the scene.

### Everything from `.14`

- **Headless AUTO EDIT** — `auto_edit` runs the fixed spine as one MCP call, copies the project to a sibling by default, returns the receipt, and leaves an editable timeline.
- **One Geneva surface** — paths, mattes, 3D, rigs, adjustment layers and supported effects behind one name; every reply says the route.
- **Remap, cached frames, parallel preview, timeline placement** — the programmatic-video primitive and `motion_bake { "place": true }`.
- **Creative contract** — `plan_validate` enforces a style bible and a retention contract.
- **The signal work** — the fifty-signal vocabulary is parsed, spent and logged; every reading has a reader, a consumer and a decision.
- **AUTO EDIT picture work** — carried A-roll, the talking-head door, the picture floor, the beat grid, honest plans, live timeline follow.
- **Security and CI** — stock redirect validation, the `url 2.5.9` RUSTSEC fix, Nix and RustSec release gates, a non-cancelling workflow queue.

Read the full note: [`docs/RELEASE-LETTER-0.2.8-funcut.15.md`](docs/RELEASE-LETTER-0.2.8-funcut.15.md).

---

## Install

### One command

```sh
git clone https://github.com/coreselfmastery/FUNCUT.git
cd FUNCUT
./install-linux.sh
```

Or run it straight from the repository:

```sh
curl -fsSL https://raw.githubusercontent.com/coreselfmastery/FUNCUT/main/install-linux.sh | bash
```

`install-linux.sh` downloads the newest published release for your architecture, verifies its checksum, and drops the AppImage into `~/.local/bin` — no root. It also fetches the Geneva motion sidecar (v1.6.0, ~16 MB) on a best-effort basis. Other shapes:

```sh
./install-linux.sh --deb      # through apt, asks for sudo
./install-linux.sh --rpm      # through dnf, asks for sudo
./install-linux.sh --list     # show what the releases hold, install nothing
```

If the newest tag is still building, the script picks the newest **published** build. The release workflow publishes a release itself when the tag’s CI and package jobs pass.

### Packages

Every Linux architecture gets a self-contained bundle:

| | x86_64 | aarch64 |
|---|---|---|
| Linux | `.deb`, `.rpm`, `.pkg.tar.zst`, `.AppImage` | `.deb`, `.rpm`, `.pkg.tar.zst`, `.AppImage` |

`SHA256SUMS` lists each file’s checksum; `manifest.json` names the bundle set. The `.deb` and `.rpm` carry the FFmpeg runtime libraries beside the binary. The AppImage needs no `libfuse2`.

There is also a community **Flatpak on [Flatpark](https://flatpark.org/apps/app.concat.editor/)**, wrapping the x86_64 `.deb` of each release.

### First run

The window opens on Vulkan. On a minimal install, add the Vulkan and ALSA runtime libraries:

```sh
sudo apt install mesa-vulkan-drivers libasound2t64   # Debian 13+ / Ubuntu 24.04+
sudo dnf install mesa-vulkan-drivers alsa-lib        # Fedora
```

A machine with no GPU still runs: Mesa’s lavapipe is a software Vulkan renderer — slower, but complete. The AI models download once from Settings and work offline after that. The 3D layer itself never needs a GPU.

### Headless

The editor is also an MCP server. A sandbox needs no desktop, GPU or build toolchain:

```sh
export PATH="$HOME/.local/bin:$PATH"
concat --version && concat --smoke
export CONCAT_MCP_ALLOW=all CONCAT_MCP_ROOTS=/workspace/video
concat --mcp headless
```

Full recipe: [`docs/SANDBOX.md`](docs/SANDBOX.md).

---

## The editor

### Timeline and cutting

Multi-track, multi-timeline, untyped lanes, magnetic mode, ripple edits, snapping, trim-follow, split/razor, delete-left/right, merge, freeze frame, reverse, mirror, rotate, per-lane lock/visibility/mute/height, marquee and band selection, keyframe diamonds, continuous zoom, phone pinch zoom.

**55 edit commands** cover media, clips, speed, transforms, text, shapes, motion clips, effect layers, keyframes, masks, cutout, stabilize, tracks, timelines, fonts and media paths. `batch` makes several commands one atomic, undoable edit.

### Keyframes

12 animatable properties per clip — scale, offset x/y, rotation, opacity, volume and seven mask numbers — with full CSS cubic-bezier easing, a dopesheet, a curves view and per-key bezier handles. Effect knobs are keyframable. Speed ramps run 1/16×–16× with six presets.

### Effects and grading

**327 packages**: 57 filters, 123 effects, 122 transitions, 25 audio packages. GPU WGSL for picture, FFmpeg chains for audio, multi-pass shaders, 3D LUTs, `.cube` import, user packages, a 164-filter FFmpeg allowlist. Colour wheels, per-channel curves, exposure in stops, adjustment layers, per-clip or one-look-per-video.

### Text and captions

rustybuzz shaping, bidi, 41 bundled title faces, six script fallbacks (CJK, Arabic, Hebrew, Devanagari, Bengali, Thai), colour emoji, stroke, shadow, plate, tracking, line height, wrapping, per-word reveal maps, 31 text presets, user font import.

### Masks, stabilize, enhance

Six analytic mask shapes, painted brush strokes, MediaPipe selfie (compiled in), Robust Video Matting, IS-Net, SlimSAM smart brushes. Stabilization uses Shi-Tomasi + pyramidal Lucas-Kanade + RANSAC + Gaussian smoothing. Enhance uses Real-ESRGAN 1×/2×/4× with a before/after preview.

### Rendering and export

wgpu compositor in Rgba16Float scene-linear, one draw call per layer, six blend modes, HDR end to end (HLG/PQ in, BT.2390 tone-map or HDR pass-through out). Export H.264/HEVC/AV1, 10-bit, up to 4K 60, x264/x265/SVT-AV1 and NVENC/QSV/AMF, VBR/CBR/average, AAC 48 kHz, faststart MP4, staged progress with cancel.

### Small-machine tuning

Every cache sizes itself to installed RAM. Seven caches are retuned for ≤6 GB: warm decoders 8→4, stills 24→8, title paintings quartered, export pool RAM-shared. Proxies and a decode-level ladder keep scrubbing responsive. A 4 GB laptop can edit 20-minute videos; the measurements are in [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md).

---

## The motion engine and 3D

- **33 presets**: lower thirds, kinetic captions, stat cards, wordmark sting, chapters, pull quote, CTA, code terminal, code diff, bar/progress/line/donut/area charts, before/after, counting number, mesh backdrop, bar race, comparison table, social quote/stat, headline flash, phone/tablet/laptop frames, pin route map, horizontal timeline, flowchart, and `funcut.scene3d.showcase`.
- **30 authoring ops**: slots, text, words, markup, move, retime, tracks, track points, remap, track colours/size/noise, fade, slide, pop, groups, rects, text, charts, counters, particles, 3D scene add/set, delete, blend, opacity, duration, background, camera, ink.
- **3D**: five geometries, three light types, camera path, per-pixel Phong, CPU z-buffer.
- **Routing**: native for 3D-only comps; Geneva for markup; a warning when Geneva cannot draw the scene.
- **Tools**: `motion_presets`, `motion_inspect`, `scene_analysis`, `motion_frame`, `motion_preview`, `motion_bake`, `motion_reel`, `motion_probe`, `motion_overlay`, `motion_import`, `motion_farm_progress`, `motion_farm_cloud`.

Full manual: [`docs/MOTION.md`](docs/MOTION.md).

---

## The agent stack

### 114 MCP tools

`concat --mcp` serves **114 tools** over stdio, with both protocol eras and schemas generated from the API’s own types. **106** are served to a director by default; the whole catalogue is behind `CONCAT_MCP_ALLOW=all`.

| Area | Tools |
|---|---:|
| Projects, timeline and planning | 12 |
| AUTO EDIT | 1 |
| Media | 2 |
| OpenVid stock and footage planning | 10 |
| Live stock shelves | 3 |
| Sounds | 2 |
| Music | 5 |
| Stickers | 5 |
| Cutting and history | 18 |
| Picture | 2 |
| Titles and captions | 5 |
| Keyframes and effects | 5 |
| Analysis | 7 |
| Speech | 4 |
| Templates | 2 |
| Export | 4 |
| Perception, emotion and creative | 16 |
| Motion | 11 |
| **Total** | **114** |

### AUTO EDIT

The Forge page starts a whole faceless video with one click. The fixed spine is 18 stages:

`HandoffVoiceover`, `AudioRepair`, `Transcribe`, `VoiceSignals`, `Understand`, `Plan`, `Timeline`, `Gather`, `Captions`, `Motion`, `Music`, `Sfx`, `Preview`, `Revise`, `TimelineReady`, `Export` (optional), `Verify`, `Watch`.

It copies the project to a sibling by default, writes a stage receipt and report, and leaves an editable timeline. It does not export by default; call `export_start` for the file.

### Director, maestro, clip-skills

- [`funcut-director`](.claude/skills/funcut-director/SKILL.md) — a **25-phase** measured pipeline from script or voiceover to finished video.
- [`maestro`](docs/MAESTRO.md) — 28 reference modules over 10 corpora, 157 shot cards; design, motion and film judgment.
- [`clip-skills`](docs/CLIPSKILLS.md) — editing craft: where to cut, audio spine, B-roll matching, pacing, grading, voice processing, export numbers.
- [`Forge`](docs/forge/FORGE.md) — the harness that drives a director run with any vision-capable model.

### Perception and emotion

Only inline image content blocks are visible to a vision model: `preview_frame`, `motion_frame`, `watch_render`, `stock_contact_sheet`. A text-only model is told to rely on rankings and measured facts.

The `emotion` tool reads what every line of the voiceover feels like and drives easing, colour, sound, duck and stock. The gate accepts decisions: `cut`, `caption`, `graphic`, `bgm`, `transition`, `pacing`.

### Safety

Read-only flags, a per-tool allowlist, per-tool timeouts, project locks between window and headless, and writes confined to `CONCAT_MCP_ROOTS`. Full reference: [`docs/MCP.md`](docs/MCP.md).

---

## Libraries

- **Stock:** 937,957 OpenVid clips — the OpenVid-1M **research dataset**, not rights-cleared stock. Read [`docs/STOCK_LICENSING.md`](docs/STOCK_LICENSING.md) before publishing.
- **Music:** 1,025 CC0 tracks from FreePD + 166 MobyGratis tracks. **MobyGratis is non-commercial only — never in monetized or client work.** See [`docs/BGM.md`](docs/BGM.md).
- **Sounds:** 530 CC0 effects in 12 categories, including a 100-sound Hollywood trailer pack. See [`docs/AUDIO_LIBRARY.md`](docs/AUDIO_LIBRARY.md).
- **Stickers:** 1,285 Fluent Emoji, MIT licensed. See [`docs/MAGICK.md`](docs/MAGICK.md).
- **Fonts:** 41 title faces + 6 script fallbacks + user import.
- **Templates:** any project saves as a template; slots are placeholder media a later `FillSlot` swaps in place.

---

## Documentation map

| Read this | When |
|---|---|
| [`docs/FEATURES.md`](docs/FEATURES.md) | The canonical inventory: every tool, page, engine and model, with exact counts |
| [`docs/FUNCUT-CHANGES.md`](docs/FUNCUT-CHANGES.md) | The fork’s change ledger, newest first |
| [`docs/RELEASE-LETTER-0.2.8-funcut.15.md`](docs/RELEASE-LETTER-0.2.8-funcut.15.md) | The `.15` release note |
| [`docs/MCP.md`](docs/MCP.md) | Connecting an agent; every tool area |
| [`docs/SANDBOX.md`](docs/SANDBOX.md) | Install and run headless in a sandbox |
| [`docs/MOTION.md`](docs/MOTION.md) | The Geneva comp document, its ops, presets, storyboard, farm |
| [`docs/EMOTION.md`](docs/EMOTION.md) | The emotion read and the gate |
| [`docs/MAESTRO.md`](docs/MAESTRO.md) · [`docs/CLIPSKILLS.md`](docs/CLIPSKILLS.md) | The two judgment shelves |
| [`docs/FUNCUT.md`](docs/FUNCUT.md) | Small-machine install, first run and tuning |
| [`docs/PERFORMANCE.md`](docs/PERFORMANCE.md) | The measured numbers |
| [`docs/TESTING.md`](docs/TESTING.md) | The test count, breakdown and reproduction |
| [`docs/EFFECTS-GALLERY.md`](docs/EFFECTS-GALLERY.md) | Every effect’s before-and-after card |
| [`docs/SHADER_GALLERY.md`](docs/SHADER_GALLERY.md) | Twenty effects in motion |
| [`CONTRIBUTING.md`](CONTRIBUTING.md) | Setup, layout, checks and licensing |

---

## Status and limits

FUNCUT is in **beta**. It works, and it still has edges. The honest limits:

- **No target-hardware acceptance yet.** The AUTO EDIT button, the 3D renderer and the director’s Verify record are unit- and CI-tested, but no real brief-to-export run on the intended low-memory Linux laptop has been recorded.
- **3D is CPU-only.** No GPU path, no shadows, textures, normal maps or full PBR. Keep scenes to a few dozen objects at 1080p.
- **AUTO EDIT does not export by default.** It leaves an editable timeline; call `export_start`.
- **Headless AUTO EDIT does not extract a talking head from raw video.** That is the GUI handoff’s work.
- **The Geneva sidecar is a pinned closed-source binary (1.6.0).** It has no path drawing, no variable-font axes, and keyframes in px and % only.
- **VAAPI hardware encode is opt-in and unvalidated on real GPUs.**
- **The FreePD music shelf depends on archive.org.**
- **OpenVid is a research dataset**, not rights-cleared stock.
- **MobyGratis music is non-commercial only.**

Read [`KNOWN_ISSUES.md`](KNOWN_ISSUES.md) before trusting a feature on your own machine.

---

## Build from source

You need Rust 1.93 or newer (see `rust-version` in `src/Cargo.toml`), the FFmpeg 7+ development libraries (the CI build pins FFmpeg 8.1), cmake and a C++ compiler.

- **Linux:** use your distro’s FFmpeg if it is 7 or newer, otherwise use a BtbN `shared` build (see `scripts/ci/deps.lock`) and point `FFMPEG_DIR` at it. Or run `./install-linux.sh`, which does all of it.

```sh
cd src
cargo run -p concat          # the editor window
```

Other useful commands:

```sh
cargo build --profile app -p concat   # the shipping binary (fat LTO, stripped)
cargo test                            # the test suite
cargo fmt --check && cargo clippy --all-targets -- -D warnings   # what CI checks
```

CI counts the suite in three lanes — the workspace under `cargo nextest`, the documentation’s examples, and the gRPC feature — **1,393 Rust tests** at the commit this was written, six skipped, each skip named and none of them a platform. [`docs/TESTING.md`](docs/TESTING.md) holds the count, the breakdown and the way to reproduce it.

If you change text the interface shows, run `python3 scripts/locales.py` from the repo root so the string inventory stays in sync. Full details live in [`CONTRIBUTING.md`](CONTRIBUTING.md).

---

## Contributing and license

The best way to contribute is to grab a build from the [Releases](https://github.com/coreselfmastery/FUNCUT/releases) page and use it: find where it breaks, and say where it could be better.

Ready to write code? [`CONTRIBUTING.md`](CONTRIBUTING.md) covers setup, the layout of the tree, the checks to run, and how contributions are licensed.

FUNCUT is free and open source under **AGPL-3.0-or-later**. No watermark, no account, no paid tier. The code is a fork of [jub0t/Concat](https://github.com/jub0t/Concat); upstream’s project and community remain the foundation this work builds on.

Not affiliated with CapCut or ByteDance.

---

## Star history

<a href="https://www.star-history.com/?repos=coreselfmastery%2FFUNCUT&type=date&releases=&legend=bottom-right">
 <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=coreselfmastery/FUNCUT&type=date&legend=bottom-right" />
</a>

## Contributors

<a href="https://github.com/coreselfmastery/FUNCUT/graphs/contributors">
  <img alt="Contributors" src="https://contrib.rocks/image?repo=coreselfmastery/FUNCUT">
</a>
