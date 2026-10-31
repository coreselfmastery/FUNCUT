<p align="center">
  <img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/concat-mark-dark.png" alt="Concat" width="100" />
</p>

<h1 align="center">Concat</h1>
<h3 align="center">De gratis, open-source CapCut-vervanger voor Linux.</h3>

<div align="center">

<img src="https://cdn.jsdelivr.net/gh/jub0t/Concat@main/assets/editor.png" alt="Concat editor" width="100%" />

<p align="center">
  <a href="https://github.com/coreselfmastery/FUNCUT/releases"><img src="https://img.shields.io/github/downloads/coreselfmastery/FUNCUT/total?style=flat&logo=github&logoColor=F8F8F8&label=Downloads&labelColor=212123&color=0568FD" alt="Total Downloads" /></a>
  <a href="https://github.com/coreselfmastery/FUNCUT/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/coreselfmastery/FUNCUT/ci.yml?style=flat&logo=githubactions&logoColor=F8F8F8&label=Build&labelColor=000000" alt="Build Status" /></a>
  <a href="https://github.com/coreselfmastery/FUNCUT/releases"><img src="https://img.shields.io/badge/Version-0.2.8-0568FD?style=flat&logo=semver&logoColor=F8F8F8&labelColor=212123" alt="Concat Version 0.2.8" /></a>
  <a href="https://discord.gg/DVuPfpXfqP"><img src="https://img.shields.io/badge/Discord-Join%20the%20server-5865F2?style=flat&logo=discord&logoColor=F8F8F8&labelColor=000000" alt="Join Concat Discord" /></a>
  <a href="../LICENSE"><img src="https://img.shields.io/badge/License-AGPL%20v3-0568FD?style=flat&logo=gnu&logoColor=F8F8F8&labelColor=212123" alt="License: AGPL-3.0-or-later" /></a>
</p>

<p align="center">
  <a href="https://www.star-history.com/jub0t/concat"><img src="https://api.star-history.com/badge?repo=jub0t/concat&type=rank" alt="Star History Rank" height="62" /></a>
  <a href="https://trendshift.io/repositories/191379?utm_source=trendshift-badge&amp;utm_medium=badge&amp;utm_campaign=badge-trendshift-191379" target="_blank" rel="noopener noreferrer"><img src="https://trendshift.io/api/badge/trendshift/repositories/191379/daily?language=Rust" alt="jub0t%2FConcat | Trendshift" width="250" height="55"/></a>
  <a href="https://concatenate.pages.dev/#download"><img src="../assets/download_button.svg" alt="Download Concat" width="251" /></a>
</p>

[🇬🇧 English](../README.md) · [🇩🇪 Deutsch](README.de.md) · [🇪🇸 Español](README.es.md) · [🇫🇷 Français](README.fr.md) · [🇮🇹 Italiano](README.it.md) · **🇳🇱 Nederlands** · [🇵🇱 Polski](README.pl.md) · [🇧🇷 Português (Brasil)](README.pt-BR.md) · [🇷🇺 Русский](README.ru.md) · [🇺🇦 Українська](README.uk.md) · [🇹🇷 Türkçe](README.tr.md) · [🇦🇿 Azərbaycanca](README.az.md) · [🇭🇷 Hrvatski](README.hr.md) · [🇮🇩 Bahasa Indonesia](README.id.md) · [🇻🇳 Tiếng Việt](README.vi.md) · [🇹🇭 ไทย](README.th.md) · [🇯🇵 日本語](README.ja.md) · [🇰🇷 한국어](README.ko.md) · [🇨🇳 简体中文](README.zh-Hans.md) · [🇹🇼 繁體中文](README.zh-TW.md) · [🇸🇦 العربية](README.ar.md) · [🇮🇱 עברית](README.he.md) · [🇮🇷 فارسی](README.fa.md) · [🇮🇳 हिन्दी](README.hi.md) · [🇵🇰 اردو](README.ur.md) · [🇧🇩 বাংলা](README.bn.md)

> Vertaald uit de Engelse README van 11 oktober 2026. De Engelse versie is degene die actueel wordt gehouden.

</div>


## Over Concat

Concat is een gratis, open-source video-editor en een alternatief voor CapCut op Linux. Het doet waarvoor mensen CapCut eigenlijk openen: automatische ondertitels, tekst-naar-spraak, achtergrond verwijderen, keyframe-animatie, effecten en titels, monteren op meerdere sporen, exporteren in 4K. En dat zonder addertjes onder het gras: geen watermerk, geen account, geen abonnement, geen upload.

Alles draait lokaal, op een native Rust-engine met een GPU-compositor. Installeren, beeldmateriaal erin slepen, knippen. De AI-modellen voor ondertitels, stemmen en uitsnijden download je één keer via Instellingen, daarna werken ze offline. Je beeldmateriaal verlaat je schijf nooit.

**Geschikt voor:** TikTok, Reels en Shorts, YouTube-video’s, tutorials en schermopnames, podcastfragmenten, memes.

**Ook voor machines:** een JSON-RPC-API, zodat ook scripts en AI-agents er video mee kunnen knippen.

## Hoogtepunten

- 🚫 **Geen watermerken. Geen account. Geen betaalmuur.** Nooit.
- 🔒 **100% lokaal.** Er wordt niets geüpload. Werkt offline.
- 💬 **Automatische ondertitels.** Lokale Whisper. Kies een modelgrootte en je krijgt opgemaakte ondertitels op de tijdlijn.
- 🗣️ **Tekst-naar-spraak + stemklonen.** Gratis lokale stemmen, of elke stem op basis van een paar seconden opname.
- 🧍 **Achtergrond verwijderen.** Mensen, objecten, of teken het masker zelf.
- 🎞️ **Keyframes.** Positie, schaal, rotatie, dekking, volume, effectparameters. Met ingebouwde curve-editor.
- ✨ **100 effecten, filters en overgangen.** Gerenderd op de GPU, live in het voorbeeld.
- ✂️ **Snel knippen.** Splitsen, trimmen, ripple, samenvoegen, stilstaand beeld, snelheid. Een magnetische tijdlijn als je dat wilt.
- 🎚️ **Meerdere sporen, meerdere tijdlijnen.** Verschillende montages in één project. Overvloeimodi, bijsnijden, spiegelen.
- 📝 **Titels.** Lettertypen, omlijning, schaduw, achtergrondvlak. Presets om mee te beginnen.
- 🎙️ **Stem opschonen met één schakelaar.** Ruis verwijderen, stem verbeteren, luidheid gelijktrekken. Plus chipmunk, robot, telefoon en meer.
- 📤 **Exporteren.** H.264, HEVC, AV1. Tot 4K 60, 10-bit kleur.
- 🦀 **Native Rust-engine.** GPU-compositor, proxy’s, hardwaredecodering. Zelfs 4K scrubt soepel.
- 🤖 **Scriptbaar.** Een JSON-RPC-API waarmee scripts en AI-agents video kunnen knippen. Een build vanaf de broncode voegt gRPC en een opdrachtregeltool toe.
- 🖥️ **Linux.** 26 talen. Dezelfde app, dezelfde projectbestanden.

## Downloaden

Twee manieren:

1. **[De website](https://concatenate.pages.dev/#download)** geeft je de juiste build voor je computer. Begin hier.
2. **[GitHub Releases](https://github.com/coreselfmastery/FUNCUT/releases)** heeft elke build voor elk platform, met installers, pakketten en checksums. Voor als je zelf wilt kiezen.

Concat is in **bèta**: het werkt, maar er zitten nog ruwe randjes aan. [Laat het weten](https://github.com/jub0t/Concat/issues) als je er een tegenkomt.

**Platforms**

- ✅ **Linux** · x86_64 en ARM. `.deb`, `.rpm`, `.AppImage` en een Arch-pakket. Ook als Flatpak op **[Flatpark](https://flatpark.org/apps/app.concat.editor/)**, een Flatpak-remote uit de community die de x86_64-`.deb` van elke release verpakt en met elke release meegaat

✅ Ondersteund · 🚧 In ontwikkeling · 🧪 Nog te testen

**Systeemvereisten** en de groottes van de optionele modellen staan op [de website](https://concatenate.pages.dev/guides/system-requirements).

**[Code signing policy](../CODE_SIGNING.md)**

## Bijdragen

> [!IMPORTANT]
> De beste manier om bij te dragen is een build van de [Releases](https://github.com/coreselfmastery/FUNCUT/releases)-pagina te halen en die te gebruiken: zoek uit waar hij stukgaat, en zeg wat er beter kan.
>
> Klaar om code te schrijven? [CONTRIBUTING.md](../CONTRIBUTING.md) beschrijft de installatie, de indeling van de broncode, de controles die je moet draaien en onder welke licentie bijdragen vallen. Stuur je Concat aan vanuit een script, een service of een agent? [De ontwikkelaarsdocumentatie](https://concatenate.pages.dev/docs) beschrijft de Concat-API en de transports ervan: JSON-RPC en gRPC. In [deze Discussion](https://github.com/jub0t/Concat/discussions/3) werd het project aangekondigd.
> 
> Bijdragers kunnen een `@Contributor`-rol krijgen op de Discord-server: vraag er gewoon om.

## Sterrengeschiedenis

<a href="https://www.star-history.com/?repos=jub0t%2Fconcat&type=date&releases=&legend=bottom-right">
 <img alt="Star History Chart" src="https://api.star-history.com/chart?repos=jub0t/concat&type=date&legend=bottom-right" />
</a>

## Bijdragers

<a href="https://github.com/jub0t/Concat/graphs/contributors">
  <img alt="Contributors" src="https://contrib.rocks/image?repo=jub0t/concat">
</a>

## DMCA

If you believe something in this repository, its releases, or a package or model it distributes infringes your copyright, send a notice through [GitHub's DMCA takedown process](https://docs.github.com/en/site-policy/content-removal-policies/dmca-takedown-policy), which covers this repository's content and releases; a valid notice is acted on through that process. Material this repository inherits from upstream Concat can also be reported to upstream at jub0trd@gmail.com, the address upstream's README publishes for the purpose.
