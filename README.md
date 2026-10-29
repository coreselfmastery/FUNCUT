<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/concat-mark-dark.png" alt="Concat" width="100" />
</p>

<h1 align="center">Concat</h1>
<h3 align="center">The free, open-source CapCut replacement for Linux.</h3>

> [!NOTE]
> **FUNCUT** — this repository is that editor tuned for small, low-memory
> Linux laptops (4-6 GB of RAM): every cache sizes itself to the machine it
> is on, and `./install-linux.sh` puts it on one in a single command. On top
> of the editor it ships an agent stack: a built-in MCP server with **87
> tools**, the agent's senses (scene cuts, beat grids, dead air,
> transcription, TTS, loudness), perception (contact-sheet images a model
> can actually see), an emotion-driven edit pipeline with a validated brand
> kit, a motion-graphics engine, a director skill that runs an 11-phase
> measured pipeline, the vendored maestro design judgment, and a Forge page
> in the editor that starts a whole faceless video with one click. Stock
> (937,957 clips), music (1,191 CC0 tracks), sounds (430) and stickers
> (149,758) are all searched locally and fetched per use. The complete
> verified inventory — every tool, page, engine and model, read from the
> source — is [docs/FEATURES.md](docs/FEATURES.md); the fork's change
> ledger is [docs/FUNCUT-CHANGES.md](docs/FUNCUT-CHANGES.md), the
> small-machine notes [docs/FUNCUT.md](docs/FUNCUT.md), and the agent
> guide [docs/MCP.md](docs/MCP.md).

<div align="center">

<img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/editor.png" alt="Concat editor" width="100%" />

<p align="center">
  <a href="https://github.com/jub0t/Concat/releases"><img src="https://img.shields.io/github/downloads/jub0t/concat/total?style=flat&logo=github&logoColor=F8F8F8&label=Downloads&labelColor=212123&color=0568FD" alt="Total Downloads" /></a>
  <a href="https://github.com/jub0t/Concat/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/jub0t/Concat/ci.yml?style=flat&logo=githubactions&logoColor=F8F8F8&label=Build&labelColor=000000" alt="Build Status" /></a>
  <a href="https://github.com/jub0t/Concat/releases"><img src="https://img.shields.io/badge/Version-0.2.8-0568FD?style=flat&logo=semver&logoColor=F8F8F8&labelColor=212123" alt="Concat Version 0.2.8" /></a>
  <a href="https://discord.gg/DVuPfpXfqP"><img src="https://img.shields.io/badge/Discord-Join%20the%20server-5865F2?style=flat&logo=discord&logoColor=F8F8F8&labelColor=000000" alt="Join Concat Discord" /></a>
  <a href="LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-0568FD?style=flat&logo=gnu&logoColor=F8F8F8&labelColor=212123" alt="License: AGPL-3.0-or-later" /></a>
</p>

<p align="center">
  <a href="https://www.star-history.com/jub0t/concat"><img src="https://api.star-history.com/badge?repo=jub0t/concat&type=rank" alt="Star History Rank" height="62" /></a>
  <a href="https://trendshift.io/repositories/191379?utm_source=trendshift-badge&amp;utm_medium=badge&amp;utm_campaign=badge-trendshift-191379" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/trendshift/repositories/191379/daily?language=Rust" alt="jub0t%2FConcat | Trendshift" width="250" height="55"/></a>
  <a href="https://concatenate.pages.dev/#download"><img src="assets/download_button.svg" alt="Download Concat" width="251" /></a>
</p>

**🇬🇧 English** · [🇩🇪 Deutsch](docs/README.de.md) · [🇪🇸 Español](docs/README.es.md) · [🇫🇷 Français](docs/README.fr.md) · [🇮🇹 Italiano](docs/README.it.md) · [🇳🇱 Nederlands](docs/README.nl.md) · [🇵🇱 Polski](docs/README.pl.md) · [🇧🇷 Português (Brasil)](docs/README.pt-BR.md) · [🇷🇺 Русский](docs/README.ru.md) · [🇺🇦 Українська](docs/README.uk.md) · [🇹🇷 Türkçe](docs/README.tr.md) · [🇦🇿 Azərbaycanca](docs/README.az.md) · [🇭🇷 Hrvatski](docs/README.hr.md) · [🇮🇩 Bahasa Indonesia](docs/README.id.md) · [🇻🇳 Tiếng Việt](docs/README.vi.md) · [🇹🇭 ไทย](docs/README.th.md) · [🇯🇵 日本語](docs/README.ja.md) · [🇰🇷 한국어](docs/README.ko.md) · [🇨🇳 简体中文](docs/README.zh-Hans.md) · [🇹🇼 繁體中文](docs/README.zh-TW.md) · [🇸🇦 العربية](docs/README.ar.md) · [🇮🇱 עברית](docs/README.he.md) · [🇮🇷 فارسی](docs/README.fa.md) · [🇮🇳 हिन्दी](docs/README.hi.md) · [🇵🇰 اردو](docs/README.ur.md) · [🇧🇩 বাংলা](docs/README.bn.md)

## Paid Sponsors

<table width="100%">
  <tr>
    <td align="center">
      <a href="https://proxyon.io/?ref=concat"><img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/proxyon_sponsor_logo.png" alt="Proxyon" width="80" /></a><br />
      <a href="https://proxyon.io/?ref=concat"><b>Proxyon</b></a><br />
      <sub>Pay-as-you-go proxies for developers<br />Use code <code>JUB0T</code> for 20% off</sub>
    </td>
  </tr>
</table>

</div>


## About

Concat is a free, open-source video editor and a CapCut alternative for Linux. It covers what people actually open CapCut for: auto-captions, text-to-speech, background removal, keyframe animation, effects and titles, multi-track cutting, 4K export. With none of the catches: no watermark, no account, no subscription, no upload.

Everything runs locally on a native Rust engine with a GPU compositor. Install it, drop in footage, cut. The AI models for captions, voices and cutout download once from Settings and work offline after that. Your footage never leaves your disk.

**Good for:** TikTok, Reels and Shorts, YouTube videos, tutorials and screen recordings, podcast clips, memes.

**Also for machines:** a built-in MCP server for AI agents (`concat --mcp`, [set up](docs/MCP.md)) and a JSON-RPC API for scripts, so they can cut video with it too. The agents get senses as well - scene cuts, beat grids, dead air, transcription and a spoken voiceover, all MCP jobs - and a director's brain ships in this repo: the [funcut-director skill](docs/forge/FORGE.md), one `forge run` from a finished faceless video. A second skill, [maestro](docs/MAESTRO.md), rides beside it with the design, motion and film judgment - a 157-shot vocabulary, motion principles and a sound-design method the director draws on phase by phase. The agents' craft critic runs on three rungs - measured pixels only, a bundled 0.5B local model, or a free-tier hosted vision API wired up in one call ([docs/VLM.md](docs/VLM.md)). And the edit itself is emotion-driven: the `emotion` tool reads what every line of the voiceover feels like and drives the easing, the colour, the sound, the duck and the stock choices from it - gate included ([docs/EMOTION.md](docs/EMOTION.md)).

## Highlights

- 🚫 **No watermarks. No account. No paywall.** Ever.
- 🔒 **100% local.** Nothing uploads. Works offline.
- 💬 **Auto-captions.** Local Whisper. Pick a model size, get styled captions on the timeline.
- 🗣️ **Text-to-speech + voice cloning.** Free local voices, or any voice from a few seconds of a recording.
- 🧍 **Background removal.** People, objects, or paint the mask yourself.
- 🎞️ **Keyframes.** Position, scale, rotation, opacity, volume, effect parameters. Curve editor built in.
- ✨ **127 effects, filters, transitions and audio packages.** GPU-rendered, live in the preview.
- 🎵 **A music library that costs nothing to hold.** 1,191 whole CC0 background tracks on two shelves - FreePD (Kevin MacLeod and friends) and Moby's MobyGratis instrumentals - searched locally, fetched one track at a time. The Lo-Fi pack's 554 MB zip is never downloaded: each of its tracks is one ranged read of exactly that track's span.
- ✂️ **Cut fast.** Split, trim, ripple, merge, freeze frame, speed. Magnetic timeline if you want it.
- 🎚️ **Multi-track, multi-timeline.** Several cuts in one project. Blend modes, crop, flips.
- 📝 **Titles.** Fonts, stroke, shadow, background plate. Presets to start from.
- 🎙️ **One-switch voice cleanup.** Denoise, enhance voice, level the loudness. Plus chipmunk, robot, telephone and friends.
- 📤 **Export.** H.264, HEVC, AV1. Up to 4K 60, 10-bit colour.
- 🦀 **Native Rust engine.** GPU compositor, proxies, hardware decode. 4K scrubs smoothly.
- 🤖 **Scriptable.** An MCP server built in, so Claude and other AI agents can edit with it ([one line to set up](docs/MCP.md)), and a JSON-RPC API for scripts. A build from source adds gRPC and a command-line tool. With the [Forge harness](docs/forge/FORGE.md) and the [funcut-director skill](.claude/skills/funcut-director/SKILL.md) an agent directs a whole video from a script - with [maestro](docs/MAESTRO.md), the design-and-motion judgment skill, riding shotgun - and the editor's Forge page starts one with a click.
- 🖥️ **Linux.** x86_64 and ARM. 26 languages. Same app, same project files.

## Download

Two ways in:

1. **[The website](https://concatenate.pages.dev/#download)** hands you the right build for your machine. Start here.
2. **[GitHub Releases](https://github.com/jub0t/Concat/releases)** has every build for every architecture, with installers, packages and checksums. For when you want to pick.

Concat is in **beta**: it works, and it still has edges. [Say so](https://github.com/jub0t/Concat/issues) when you find one.

**Platforms**

- ✅ **Linux** · x86_64 and ARM. `.deb`, `.rpm`, `.AppImage` and an Arch package. Also a Flatpak on **[Flatpark](https://flatpark.org/apps/app.concat.editor/)**, a community Flatpak remote that wraps the x86_64 `.deb` of each release and updates with it

✅ Supported · 🚧 Work in progress · 🧪 To be tested

**System requirements** and the optional model sizes are on [the website](https://concatenate.pages.dev/guides/system-requirements).

**[Code signing policy](CODE_SIGNING.md)**: what Concat sends over the network (nothing you did not ask for).

## Build from source

You need Rust 1.93 or newer (see `rust-version` in `src/Cargo.toml`), the FFmpeg 7+ development libraries, cmake and a C++ compiler.

- **Linux:** use your distro's FFmpeg if it is version 7 or newer, otherwise use a BtbN `shared` build (see `scripts/ci/deps.lock`) and point `FFMPEG_DIR` at it. Or run `./install-linux.sh` from the repo root, which does all of it.

Then:

```sh
cd src
cargo run -p concat          # the editor window
```

Other useful commands:

```sh
cargo build --profile app -p concat   # the shipping binary (fat LTO, stripped)
cargo test                            # run the test suite
cargo fmt --check && cargo clippy --all-targets -- -D warnings   # what CI checks
```

If you change text the interface shows, run `python3 scripts/locales.py` from the repo root so the string inventory stays in sync. Full details live in [CONTRIBUTING.md](CONTRIBUTING.md).

## How to Contribute

> [!IMPORTANT]
> The best way to contribute is to grab a build from the [Releases](https://github.com/jub0t/Concat/releases) page and use it: find where it breaks, and say where it could be better.
>
> Ready to write code? [CONTRIBUTING.md](./CONTRIBUTING.md) covers setup, the layout of the tree, the checks to run, and how contributions are licensed. Driving Concat from a script, a service or an agent? [The developer docs](https://concatenate.pages.dev/docs) cover the Concat API and its transports: JSON-RPC and gRPC; [docs/MCP.md](docs/MCP.md) connects an AI agent over MCP. [This Discussion](https://github.com/jub0t/Concat/discussions/3) is where the project was announced.
> 
> Contributors can claim a `@Contributor` role in the Discord server: just ask.

## Star History

<a href="https://www.star-history.com/?repos=jub0t%2Fconcat&type=date&releases=&legend=bottom-right">
 <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=jub0t/concat&type=date&legend=bottom-right" />
</a>

## Contributors

<a href="https://github.com/jub0t/Concat/graphs/contributors">
  <img alt="Contributors" src="https://contrib.rocks/image?repo=jub0t/concat">
</a>

## DMCA

If you believe something in this repository, its releases, or a package or model it distributes infringes your copyright, email [jub0trd@gmail.com](mailto:jub0trd@gmail.com) with a description of the work you own, a link to where the infringing material appears here, your contact details, a statement that you believe in good faith the use is not authorised by you, your agent, or the law, and a statement, under penalty of perjury, that the notice is accurate and that you are the owner or authorised to act for the owner, signed with your physical or electronic signature; valid notices are acted on promptly, and anyone whose material is removed may send a counter-notice to the same address.
