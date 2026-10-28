# Third-party notices

## The sticker library (MAGICK and OpenMoji)

The library's Stickers page serves 149,758 transparent-PNG overlays from
two shelves, neither of them redistributed by this repository; what a
user's machine fetches is kept in that machine's own cache, one picture
at a time. See `docs/MAGICK.md`.

**MAGICK** (145,193 pictures after the index's own not-safe-for-work
rows are left out) is the OneOverZero MAGICK dataset, presented at CVPR
2024 ("MAGICK: A Large-scale Captioned Dataset Matting Generated Images
using Chroma Keying", Burgert, Price, Kuen, Li, Ryoo), hosted at
https://huggingface.co/datasets/OneOverZero/MAGICK and licensed **CC
BY-NC 4.0** (https://creativecommons.org/licenses/by-nc/4.0/, the
project page's own link): the pictures are AI-generated and may be used
for personal and non-commercial video, not commercial video. The
dataset's small TSV index (page ids and captions, ~16 MB) is mirrored as
the `magick-index-v1` release asset of this repository so the app's one
download is digest-checked against this build; the pictures themselves
are fetched only from the Hub.

**OpenMoji** (4,565 emoji) is the OpenMoji project's colour set,
https://openmoji.org, by the HfG Schwäbisch Gmünd and contributors,
licensed **CC-BY 4.0**
(https://creativecommons.org/licenses/by/4.0/). Its index (the project's
own `openmoji.json`, annotations and tags) and its pictures (the
618x618 colour PNGs) are fetched from the project's GitHub repository;
the licence's one condition, attribution, is met by this notice and by
`docs/MAGICK.md`.

## FFmpeg

Concat links FFmpeg's libraries - libavformat, libavcodec, libavfilter,
libavdevice, libavutil, libswscale and libswresample - through the
`ffmpeg-the-third` crate. The Slint app spawns no `ffmpeg` or `ffprobe`
process.

FFmpeg is licensed under the LGPL-2.1-or-later; builds that include x264
(which the H.264 export uses) are GPL-2.0-or-later. Concat's own sources are
AGPL-3.0-or-later, and section 13 of the GPL-3.0 and AGPL-3.0 expressly
permits linking the two, so a distributed build may be conveyed on those
terms. The Linux bundles carry a BtbN `gpl-shared` build
(https://github.com/BtbN/FFmpeg-Builds), unmodified, as BtbN publishes
it - the same build CI links against.
FFmpeg source code: https://ffmpeg.org/download.html

## whisper.cpp

Transcription compiles whisper.cpp (https://github.com/ggml-org/whisper.cpp,
MIT) and ggml into the app through the `whisper-rs` crate. Whisper models
originate at https://huggingface.co/ggerganov/whisper.cpp (MIT); Concat
mirrors them and downloads them on demand from that mirror, and never
bundles them. See "The model mirror" below.

## Slint (used under GPL-3.0-only)

The `concat` crate (`src/crates/concat`) builds against
[Slint](https://github.com/slint-ui/slint), which its authors offer under
**any one** of three licences, at the user's choice: a Royalty-free licence, a
paid commercial licence, or **GNU GPL-3.0-only**.

**Concat uses Slint under the GPL-3.0-only option.** That choice is deliberate
and it is recorded here because nothing in the source tree would otherwise say
which of the three applies. The Royalty-free and commercial options are not
used: both are aimed at shipping proprietary applications, and neither can
grant downstream recipients the freedoms Concat's own licence promises them.

Section 13 of the GPL-3.0 exists for exactly this combination:

> Notwithstanding any other provision of this License, you have permission to
> link or combine any covered work with a work licensed under version 3 of the
> GNU Affero General Public License into a single combined work, and to convey
> the resulting work.

So the combined binary is conveyable. Slint's portion remains GPL-3.0-only,
Concat's portion remains AGPL-3.0-or-later, and the AGPL's section 13 network
requirement applies to the combination as a whole. Anyone forking Concat who
would rather not be bound by the GPL must take Slint under one of its other
two licences and remove or replace Concat's AGPL-licensed code accordingly;
the two cannot be mixed.

Slint pulls in its Skia renderer (see `src/crates/concat/Cargo.toml`) along
with winit and their transitive crates, which are
predominantly MIT/Apache-2.0/BSD licensed. The Skia renderer crate,
`i-slint-renderer-skia`, is built from a patched copy in
`src/patches/i-slint-renderer-skia`; its changes are listed in
`src/patches/README.md` and marked `Concat:` in the source.
`cargo tree -p concat` gives the resolved set of any given build.

## Fonts

The window and the title painter embed the interface's font into the binary
(`src/crates/concat/build.rs`, `EmbedResourcesKind::EmbedFiles`, and
`include_bytes!` in `concat-text`), so a distributed binary carries it and its
licence travels with it. The files and the full licence text are in
`src/crates/concat-text/fonts/`.

- **Hanken Grotesk**: Copyright 2021 The Hanken Grotesk Project Authors
  (https://github.com/marcologous/hanken-grotesk), SIL Open Font License
  1.1. The Regular, Medium, SemiBold, Bold and Italic static instances are
  embedded. See `fonts/LICENSE-HankenGrotesk.txt`.

These are the base fonts every title can use, embedded by `concat-text`
(`BASE_FONTS`) and registered with the window for the text presets. Each
file's licence is beside it in `src/crates/concat-text/fonts/`, as
`LICENSE-<Family>.txt`; OFL is the SIL Open Font License 1.1.

- **Inter**: Copyright 2016 The Inter Project Authors (https://github.com/rsms/inter). OFL. See `fonts/LICENSE-Inter.txt`.
- **Montserrat**: Copyright 2024 The Montserrat Project Authors (https://github.com/JulietaUla/Montserrat). OFL. See `fonts/LICENSE-Montserrat.txt`.
- **Space Grotesk**: Copyright Florian Karsten. OFL. See `fonts/LICENSE-SpaceGrotesk.txt`.
- **Archivo Black**: Copyright 2017 The Archivo Black Project Authors (https://github.com/Omnibus-Type/ArchivoBlack). OFL. See `fonts/LICENSE-ArchivoBlack.txt`.
- **Bebas Neue**: Copyright 2010 Dharma Type. OFL. See `fonts/LICENSE-BebasNeue.txt`.
- **Anton**: Copyright 2020 The Anton Project Authors (https://github.com/googlefonts/AntonFont). OFL. See `fonts/LICENSE-Anton.txt`.
- **DM Serif Display**: Copyright 2014-2018 Adobe, with Reserved Font Name 'Source'; Copyright 2019 Colophon Foundry. OFL. See `fonts/LICENSE-DMSerifDisplay.txt`.
- **Abril Fatface**: Copyright 2011 TypeTogether. OFL. See `fonts/LICENSE-AbrilFatface.txt`.
- **Bangers**: Copyright 2010 The Bangers Project Authors. OFL. See `fonts/LICENSE-Bangers.txt`.
- **Permanent Marker**: Copyright Font Diner. Apache License 2.0. See `fonts/LICENSE-PermanentMarker.txt`.
- **Pacifico**: Copyright 2018 The Pacifico Project Authors. OFL. See `fonts/LICENSE-Pacifico.txt`.
- **Lobster**: Copyright 2010 The Lobster Project Authors, with Reserved Font Name "Lobster". OFL. See `fonts/LICENSE-Lobster.txt`.
- **Caveat Brush**: Copyright 2015 Google Inc. OFL. See `fonts/LICENSE-CaveatBrush.txt`.
- **Staatliches**: The Staatliches Project Authors. OFL. See `fonts/LICENSE-Staatliches.txt`.
- **Black Ops One**: The Black Ops One Project Authors. OFL. See `fonts/LICENSE-BlackOpsOne.txt`.
- **Germania One**: The Germania One Project Authors. OFL. See `fonts/LICENSE-GermaniaOne.txt`.
- **Atomic Age**: The Atomic Age Project Authors. OFL. See `fonts/LICENSE-AtomicAge.txt`.
- **Hammersmith One**: The Hammersmith One Project Authors. OFL. See `fonts/LICENSE-HammersmithOne.txt`.
- **Secular One**: The Secular One Project Authors. OFL. See `fonts/LICENSE-SecularOne.txt`.
- **Londrina Solid**: The Londrina Solid Project Authors. OFL. See `fonts/LICENSE-LondrinaSolid.txt`.
- **Bungee Hairline**: The Bungee Project Authors. OFL. See `fonts/LICENSE-BungeeHairline.txt`.
- **Rubik Scribble**: The Rubik Scribble Project Authors. OFL. See `fonts/LICENSE-RubikScribble.txt`.
- **Shadows Into Light Two**: The Shadows Into Light Two Project Authors. OFL. See `fonts/LICENSE-ShadowsIntoLightTwo.txt`.
- **Pixelify Sans**: The Pixelify Sans Project Authors. OFL. See `fonts/LICENSE-PixelifySans.txt`.
- **Google Sans Code**: The Google Sans Code Project Authors. OFL. See `fonts/LICENSE-GoogleSansCode.txt`.
- **Kosugi Maru**: Copyright MOTOYA Co., Ltd. Apache License 2.0. See `fonts/LICENSE-KosugiMaru.txt`.
- **Frijole**: The Frijole Project Authors. OFL. See `fonts/LICENSE-Frijole.txt`.
- **Emilys Candy**: The Emilys Candy Project Authors. OFL. See `fonts/LICENSE-EmilysCandy.txt`.
- **Mystery Quest**: The Mystery Quest Project Authors. OFL. See `fonts/LICENSE-MysteryQuest.txt`.
- **Zen Tokyo Zoo**: The Zen Tokyo Zoo Project Authors. OFL. See `fonts/LICENSE-ZenTokyoZoo.txt`.

The Noto faces in `src/crates/concat-text/fonts/` are the fallback for the
scripts the other faces do not draw, in the interface and in titles and
captions alike, compiled into the binary once
(`src/crates/concat-text/src/lib.rs`, `SCRIPT_FALLBACKS`). Each is the
Regular face from the notofonts project's own release, cut down with
fonttools' `pyftsubset`; the release, the SHA-256 of the archive it came
in and the command that cut it are recorded beside each face there. A subset is a Modified
Version under the OFL, and keeps the family name the release gave it, as
the OFL allows for Noto, which reserves no font name.

- **Noto Sans CJK SC** (a subset: GB 2312, Big5 level 1, JIS X 0208, the Hangul of KS X 1001 and the kana): Copyright 2014-2021 Adobe (http://www.adobe.com/), with Reserved Font Name 'Source'. OFL. See `src/crates/concat-text/fonts/LICENSE-NotoSansCJK.txt`.
- **Noto Sans Arabic UI** 2.011 (a subset: the Arabic block and the characters the Arabic, Persian and Urdu interface uses): Copyright 2022 The Noto Project Authors (https://github.com/notofonts/arabic). OFL. See `src/crates/concat-text/fonts/LICENSE-NotoSansArabicUI.txt`.
- **Noto Sans Hebrew** 3.001 (a subset: the Hebrew block): Copyright 2022 The Noto Project Authors (https://github.com/notofonts/hebrew). OFL. See `src/crates/concat-text/fonts/LICENSE-NotoSansHebrew.txt`.
- **Noto Sans Devanagari** 2.007 (a subset: the Devanagari block): Copyright 2022 The Noto Project Authors (https://github.com/notofonts/devanagari). OFL. See `src/crates/concat-text/fonts/LICENSE-NotoSansDevanagari.txt`.
- **Noto Sans Bengali** 3.011 (a subset: the Bengali block): Copyright 2022 The Noto Project Authors (https://github.com/notofonts/bengali). OFL. See `src/crates/concat-text/fonts/LICENSE-NotoSansBengali.txt`.
- **Noto Sans Thai** 2.002 (a subset: the Thai block): Copyright 2022 The Noto Project Authors (https://github.com/notofonts/thai). OFL. See `src/crates/concat-text/fonts/LICENSE-NotoSansThai.txt`.

The licence does not permit selling the font on its own; shipping the
`fonts/` directory as it stands satisfies it.

## sherpa-onnx and Kokoro voices

Text to speech uses the sherpa-onnx runtime
(https://github.com/k2-fsa/sherpa-onnx, Apache-2.0), linked statically on
Linux. It carries onnxruntime
(MIT), piper-phonemize (MIT) and espeak-ng
(**GPL-3.0-or-later**, https://github.com/espeak-ng/espeak-ng) for
grapheme-to-phoneme conversion. Because espeak-ng is compiled into the app
binary or into the sherpa-onnx libraries shipped with it, distributed builds
must comply with the GPL-3.0 for that combined work. Concat's own sources are AGPL-3.0-or-later; section 13 of both GPL-3.0
and AGPL-3.0 expressly permits that combination, so the combined binary may be
conveyed on those terms.

Kokoro voice model bundles (Apache-2.0,
https://huggingface.co/hexgrad/Kokoro-82M) originate in the sherpa-onnx
releases - including espeak-ng's data files. Concat mirrors those bundles
and downloads them on demand from that mirror, and never bundles them with
the app. See "The model mirror" below.

The Pocket TTS bundle is Kyutai's Pocket TTS (CC-BY-4.0,
https://huggingface.co/kyutai/pocket-tts, https://kyutai.org) as exported
to ONNX by KevinAHM (https://huggingface.co/KevinAHM/pocket-tts-onnx,
CC-BY-4.0) and packaged in the sherpa-onnx releases, with two sample
recordings from Kyutai's delayed-streams-modeling repository as its named
voices. It is downloaded on demand like the Kokoro bundles. Reading in the
voice of a recording is a use the person doing it is answerable for:
Concat offers it for the narrator's own voice, and Kyutai asks that no
one's voice be cloned without their consent.

Chatterbox Turbo (MIT, https://huggingface.co/ResembleAI/chatterbox-turbo-ONNX,
Resemble AI) is driven through ONNX Runtime directly, from Resemble's own
ONNX export, downloaded on demand like the other voices. Its GPT-2
tokenizer is read by the tokenizers crate (Apache-2.0). Resemble's Python
package stamps a Perth watermark on what it speaks; the ONNX pipeline
carries no such step and Concat adds none, which MIT permits and which
anyone passing the sound off as a person's should know is not there.

## ONNX Runtime

The cutout models run on Microsoft's ONNX Runtime
(https://github.com/microsoft/onnxruntime, MIT), through the `ort` crate
(https://github.com/pykeio/ort, MIT OR Apache-2.0). The Linux bundles ship
Microsoft's own shared build of ONNX Runtime 1.28.0 beside the binary -
`lib/libonnxruntime.so.1.28.0`, one runtime that the
`ort` crate and sherpa-onnx share - unmodified, as Microsoft publishes it
at https://github.com/microsoft/onnxruntime/releases/tag/v1.28.0. Its
licence (MIT, Copyright (c) Microsoft Corporation) and third-party
notices are in that release's archive.

## The cutout models

Remove background runs four models, none of which ship inside the app
except the first:

- Google's MediaPipe Selfie Segmentation (Apache-2.0), in the ONNX
  conversion published by the ONNX Community
  (https://huggingface.co/onnx-community/mediapipe_selfie_segmentation,
  Apache-2.0), compiled into the `concat-vision` crate; see
  `src/crates/concat-vision/models/NOTICE.md`. The answer when nothing
  has been downloaded.
- Robust Video Matting, the MobileNetV3 variant, by Peter Lin and others
  (https://github.com/PeterL1n/RobustVideoMatting, GPL-3.0), from that
  repository's releases, downloaded on first use. The person model.
- IS-Net from "Highly Accurate Dichotomous Image Segmentation" by Qin and
  others (https://github.com/xuebinqin/DIS, Apache-2.0), in the ONNX
  export the rembg project publishes
  (https://github.com/danielgatis/rembg, MIT), downloaded on first use.
  The object model.
- SlimSAM (https://github.com/czg1225/SlimSAM, Apache-2.0), in the ONNX
  export published at https://huggingface.co/Xenova/slimsam-77-uniform
  (Apache-2.0), downloaded on first use. The brushes' model.

Downloaded models live in the app's data directory under `cutout-models`
and are never bundled; the three that are downloaded come from Concat's own
mirror of the sources named above, described below. They are all run by
ONNX Runtime
(https://github.com/microsoft/onnxruntime, MIT) through the `ort` crate
(https://github.com/pykeio/ort, MIT OR Apache-2.0), with whatever
accelerator the runtime brings on this machine. How the runtime is
carried is under "ONNX Runtime" above.

## The model mirror

Every downloaded model named above is mirrored onto a release of Concat's own
repository, and the app fetches it from there; the upstream each was
obtained from is the fallback and is named above in every case. The table of
what is mirrored, and the digest each download is checked against, is
`models/manifest.toml`; `.github/workflows/models.yml` is what fills the
mirror.

Mirroring is redistribution, and each model keeps the licence it arrived
under - Apache-2.0 for the Kokoro bundles, SlimSAM and IS-Net, CC-BY-4.0
for Pocket TTS, BSD-3-Clause for Real-ESRGAN, MIT for Chatterbox Turbo and
the whisper conversions, GPL-3.0 for Robust Video Matting. Those terms are met
by the attributions above and by the licence files each mirrored archive
carries; a mirrored file is a verbatim copy, never a modification. Nothing
in the mirror is bundled with the app, and Concat claims no rights over any
of it.

## Effect preview photograph

The effect catalogue cards are drawn from a photograph by Vitaly Gariev on
Unsplash (https://unsplash.com/@silverkblack), used under the Unsplash
License. The still lives at `src/crates/concat-host/assets/card-still.jpg`
and is embedded in the app; each card is the effect's own shader run over
it on the GPU when the app first needs the card (`concat-host`'s
`cards.rs`).

## The background-music library (FreePD and MobyGratis)

The library's Music page (#71) serves two shelves of whole tracks, fetched
on demand from their own publishers and never bundled with the app or held
in this repository.

**FreePD** (~1,000 tracks) is Kevin MacLeod's CC0 library, fetched from the
Internet Archive's mirror of the site (https://archive.org/details/freepd),
which is the source freepd.com itself pointed at before it closed. The
catalog is the Archive item's own file listing; each track is the item's
own file. The tracks are CC0 1.0 (public domain), contributed by Kevin
MacLeod, Frank Nora, Alexander Nakarada, Rafael Krux, Bryan Teoh, Komiku,
Monplaisir, *imp*, DimensionBucket, Magdalen, Hussalonia, Manny the
Martyr and others; the app names each track's artist on its card, which
CC0 does not require and the library does anyway. FreePD:
https://freepd.com (site closed; the Archive item is the source).

**Lo-Fi** (166 tracks) is Moby's MobyGratis instrumentals, republished as
the OpenLo-Fi pack (https://github.com/btahir/open-lofi) under CC0 1.0,
with Moby's own blessing at https://mobygratis.com. The catalog is the
pack's own manifest and the tracks are read, one ranged read each, out of
the pack's single zip on its GitHub release; the app credits each track
to "Moby (MobyGratis)". MobyGratis: https://mobygratis.com.

Nothing here is redistributed by this repository: the URLs are constants
in `src/crates/concat-host/src/bgm.rs`, and what a user's machine fetches
is kept in that machine's own cache. See `docs/BGM.md`.

## The built-in sound library (the `audio/` folder)

The Audio page's built-in shelf serves 430 CC0 sound effects, which this
repository redistributes directly from `audio/` at its root (see
`docs/AUDIO_LIBRARY.md`); a user's machine fetches them one at a time
from this repository's `audio-v1` release or its `main` branch, checks
each against the SHA-256 in `audio/manifest.json`, and keeps them in that
machine's own cache.

110 of them are curated from Kenney's audio packs and CC0 packs on
OpenGameArt (each sound's `source` line in the manifest names the page a
reviewer can check the licence on), plus ten risers and sweeps generated
for this repository and dedicated to CC0. They were trimmed of head
silence, normalised to −1 dBFS and encoded Ogg Vorbis q5 at 44.1 kHz.

The other 320 are Wyzowl's sfx-kit
(https://huggingface.co/datasets/wyzowl/sfx-kit), a CC0 1.0 collection of
sound effects built for video editing, re-encoded here to 48 kHz Ogg
Vorbis q5 from the dataset's 24-bit WAV masters with no re-gain and no
trimming, so the dataset's loudness tiers and sync-point metadata survive
the conversion (the manifest carries each sound's `syncAt`). The
manifest's `source` line on every one of them points back at the dataset.


## The motion sidecar (Geneva)

`motion_bake`, `motion_frame` and the sequence tools use **Geneva**
(https://github.com/geneva-render/geneva), a browserless HTML/CSS
renderer distributed under the **MIT licence**. The Linux `.deb`, `.rpm`
and AppImage built by `build-app.yml` carry the unmodified pinned v1.6.0
binary beside the Concat executable (which is where
`Geneva::discover` looks first), checksum-checked against the same
SHA-256 digests `install-linux.sh` uses. Source installs and tarball
users get it from `install-linux.sh` instead. Concat's own sources are
AGPL-3.0-or-later; Geneva is a separate executable invoked as a process,
not linked into Concat.

## The maestro skill (design, motion and video judgment for agents)

**maestro** (https://github.com/leobbaroni/maestro), by leobbaroni, is a
unified design, motion, 3D and video skill for AI coding agents. This
repository redistributes it at `.claude/skills/maestro/` — the project-skill
install path its own documentation defines — vendored verbatim at maestro
5.0.0, commit `64e1d6ba443dc477547ad646c8b090afd15a0c04`, along with its
root `NOTICE.md` and `LICENSE`. Maestro's own text is **MIT**
(`.claude/skills/maestro/LICENSE`). The skill has two layers: `references/`
(28 distilled modules — rewritten and editorially reconciled, no verbatim
redistribution) and `library/` (unmodified verbatim copies of nine
permissively licensed corpora, each carrying its own license file alongside):

- **taste-skill** (https://github.com/Leonxlnx/taste-skill), by Leonxlnx — MIT
- **genjutsu** (https://github.com/AThevon/genjutsu), by Adrien Thevon — MIT
- **gsap-skills** (https://github.com/greensock/gsap-skills), by GreenSock — MIT
- **design-dna** (https://github.com/zanwei/design-dna), by zanwei — MIT
- **motion-design-skill** (https://github.com/lottiefiles/motion-design-skill),
  by LottieFiles — MIT
- **higgsfield-ai/skills** (https://github.com/higgsfield-ai/skills), by
  Higgsfield AI — MIT
- **comfy-skills** (https://github.com/Comfy-Org/comfy-skills), by
  Comfy-Org — MIT
- **impeccable** (https://github.com/pbakaus/impeccable), by Paul Bakaus —
  **Apache-2.0**, its upstream NOTICE.md carried alongside per §4(d)
- **video-shotcraft** (https://github.com/Vincentwei1021/video-shotcraft),
  by Wei Yihao — **Apache-2.0**, partially vendored: shot cards, pipeline,
  demos and components only; its preview gallery, template project and audio
  binaries are not redistributed, and third-party audio it references carries
  separate per-file licensing recorded in the ATTRIBUTION manifests inside
  the vendored copy

A tenth `library/` directory, `higgsfield-directors/`, is the maestro
author's own material and tracks no upstream. MIT and Apache-2.0 are both
compatible with this repository's AGPL-3.0-or-later. The full upstream
attribution table — including sources that informed the distilled layer only
(threejs-skills, remotion, hyperframes, hallmark) — is
`.claude/skills/maestro/NOTICE.md`; the upstream-drift pins for re-syncing
the vendor live in `scripts/maestro/` (see `scripts/maestro/README.md`).

## Tools fetched at runtime by the sense layer (not redistributed here)

The MCP sense tools (`watch_render`, `stock_contact_sheet`,
`motion_overlay`, `craft_critique`, `local_vlm`) can fetch optional
binaries and models into the user's own application-data directory on
first use. None of these files are committed to or redistributed by this
repository:

- **FFmpeg static build** from https://johnvansickle.com/ffmpeg/ (Linux
  x86_64/arm64), used only when there is no `ffmpeg` on `PATH` and a
  rendered movie must be decoded. These builds are **GPL-3.0** because
  they include x264 and other GPL components. Set `FUNCUT_FFMPEG` to a
  system FFmpeg to avoid the download entirely.
- **llama.cpp** `llama-server` from
  https://github.com/ggml-org/llama.cpp (MIT), pinned to release
  `b11930`.
- **SmolVLM2-500M-Video-Instruct** and **SmolVLM-500M-Instruct**
  GGUF conversions from `ggml-org` on Hugging Face (Apache-2.0), and
  **MiniCPM-V 2** from `openbmb` (Apache-2.0).

Users who redistribute a build containing these runtime-fetched files
must satisfy those components' own licences. See
`src/crates/concat-mcp/sense/README.md`.
