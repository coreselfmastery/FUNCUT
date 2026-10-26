# FUNCUT on a small machine

FUNCUT is this repository: a fork of [Concat](https://github.com/jub0t/Concat),
the free and open-source video editor, tuned to run well on a small, cheap
Linux laptop - 4 GB of RAM, a modest CPU, integrated graphics - while cutting
real videos: 20-minute YouTube pieces, not 20-second clips. The program is
the same one Concat ships (the binary is still called `concat`, and project
files move between the two); what this fork changes is how it sizes itself to
the machine, and how easy it is to get onto one. The full ledger of what
changed against upstream, number by number, is in
[FUNCUT-CHANGES.md](FUNCUT-CHANGES.md).

## The sticker library

The library's **Stickers** page puts 149,758 transparent PNGs beside the
stock clips and the music, none of them in the repository or the app:
**MAGICK** - OneOverZero's dataset of 145,193 captioned AI-generated
pictures with the backgrounds already taken away - and **OpenMoji**, the
colour emoji set (4,565, CC-BY 4.0). The indexes download once, ~18 MB;
a picture's PNG arrives only when it is picked, one plain GET, and a
plus lays it *over* the video at the playhead, never beneath it. The
search reads captions the way the sound library reads names - words
ANDed, ranked by where and how rarely they matched - and the same
engine answers the window and the five `magick` MCP tools
(`magick_status`, `magick_prepare`, `magick_search`, `magick_fetch`,
`magick_add`) the AI agent drives. The honest words - the pictures are
AI-generated, MAGICK is CC BY-NC 4.0 (personal and non-commercial use,
not commercial) - are in [MAGICK.md](MAGICK.md).

## The voiceover's pause removal

Record a voiceover and the panel offers to cut the pauses out of the take:
one button, under the transport. The take is streamed through the engine and
measured (pauses are runs of quiet at least 0.35 s long, padded 60 ms a side
so words are not clipped), and the cut lands as one undoable edit - the lane
closes up behind it, the pieces keep every look the take had, and undo puts
the whole take back. A take with nothing loud in it, or one that is almost
all silence, is reported as having no pauses to cut: that one is a retake,
not an edit. The same cut is scriptable as the `removeSpans` command over
the JSON-RPC API.

## Install

The quickest way, from a checkout of this repository:

```sh
./install-linux.sh
```

That downloads the newest published release for your architecture - an
AppImage it drops into `~/.local/bin`, no root, no packages - and checks it
against the release's checksum first. Beside the app it also fetches the
motion sidecar (Geneva v1.6.0, ~16 MB, checksum-checked, into
`~/.local/bin`) that markup motion comps bake with - best effort: the
editor runs without it, and a comp with no markup bakes on the native
engine. Other shapes:

```sh
./install-linux.sh --deb      # through apt, asks for sudo
./install-linux.sh --rpm      # through dnf, asks for sudo
./install-linux.sh --list     # show what the releases hold, install nothing
```

The script looks for a release **on this fork first**, and falls back to
Concat's own releases when the fork has none yet. The fallback is a working
editor, but it is not this one: upstream carries none of FUNCUT's own work -
no OpenVid, sticker or sound shelves, no Forge page, 42 MCP tools instead of
67 - so if the fork has a published release, that is the one you want. (A
release the workflows build starts as a draft, and a draft is invisible to
this script; see the note below.) No checkout handy? Run it straight off the
repository:

```sh
curl -fsSL https://raw.githubusercontent.com/coreselfmastery/FUNCUT/main/install-linux.sh | bash
```

Or pick a file by hand from the [Releases page](https://github.com/coreselfmastery/FUNCUT/releases):
on Linux, `Concat-<version>-linux-x86_64.deb` (or `aarch64`), the matching
`.rpm`, and `Concat-<version>-x86_64.AppImage` - the AppImage needs nothing
installed, just `chmod +x` and run.

> **A release built by the workflows starts as a draft.** Nothing - not this
> script, not the website, not a friend - can see a draft but you. Publish it
> from the repository's Releases page after it finishes building. See
> [Getting your own builds](#getting-your-own-builds).

### First run

The window opens on Vulkan, which every mainstream Linux GPU driver speaks.
If it refuses to start, install your distribution's Vulkan drivers:

```sh
sudo apt install mesa-vulkan-drivers   # Debian, Ubuntu, Mint
sudo dnf install mesa-vulkan-drivers   # Fedora
```

A machine with no GPU at all still runs: `mesa-vulkan-drivers` includes
lavapipe, a software renderer - slower, but complete. On a laptop that
already plays video, everything is usually already in place.

#### Runtime libraries

The AppImage bundles the editor, FFmpeg and the models' runtime, but not the
system's sound and display libraries. On a full desktop install they are
already there; on a **minimal** Debian, Ubuntu or Mint (a server image, a
container, a bare netinst) the window dies before it opens with:

```text
error while loading shared libraries: libasound.so.2
```

```sh
sudo apt install libasound2t64     # Debian 13+ and Ubuntu 24.04+
sudo apt install libasound2        # Debian 12 and older
sudo dnf install alsa-lib          # Fedora
```

Nothing else is needed beyond the Vulkan driver above. The `.deb` and `.rpm`
packages declare their dependencies themselves, so `apt` and `dnf` bring
these in for you - it is the no-root AppImage that cannot.

## What this fork changes

Upstream Concat sizes its memory for the machine it is on: the frame cache
is a sixteenth of the RAM, between 256 MB and 1 GB. That was the whole of
it. Everything *else* that holds memory had one fixed size, tuned for a
workstation, and on a 4 GB laptop those fixed sizes were the difference
between scrubbing and swapping. This fork makes every cache answer the same
question - *how much memory does this machine have?* - and step down
together on a small one (6 GB or less):

| What it holds                                   | Workstation | Small machine |
| ----------------------------------------------- | ----------- | ------------- |
| Warm decoders kept open for scrubbing           | 8           | 4             |
| In-memory stills from title drags               | 16-24       | 8             |
| Title paintings held for live redrawing         | 128 MB      | 42 MB         |
| Ceiling for every title on the timeline         | 512 MB      | 170 MB        |
| Decoded audio spans kept mapped                 | 64          | 16            |
| The export's own frame pool                     | 512 MB      | a share of RAM|

The export and the command-line renderer had a fixed 512 MB pool of their
own regardless of the machine; they now take the same machine-sized share
the window's editor does. The detection lives in one place
(`concat-media`'s `memory` module), so every part of the program - the
editor, an export, a headless render - sizes itself for the same machine.
The full comparison, including what the Celeron's integrated graphics does
and does not help with, is in [FUNCUT-CHANGES.md](FUNCUT-CHANGES.md).

Nothing is lost on a small machine, only held closer: a decoder closed is
reopened at the price of one seek, a still not held is painted again, a
cache that evicts sooner evicts to the same LRU it always did. The proxy
system (a smaller working copy of any file larger than HD) and the preview
quality tiers (half-size until you ask for more) are upstream's own
potato-laptop machinery, and this fork keeps all of it.

## Getting your own builds

A fork of Concat carries its build workflows with it. To have GitHub build
FUNCUT's releases for you:

1. **Enable Actions.** On your fork's GitHub page: *Actions* tab → the
   banner offering to enable workflows → *I understand my workflows, go
   ahead and enable them*.
2. **Push a version tag.** The tag must be the workspace's version, alone
   or with a suffix - `v0.2.8-funcut.1` for the version this fork carries:

   ```sh
   git tag v0.2.8-funcut.1
   git push origin v0.2.8-funcut.1
   ```

3. **Wait, then publish.** The Release workflow runs the tests, builds
   every platform and architecture, launches each bundle before it is
   uploaded, and files everything under a **draft** release on the
   Releases page. Look it over and publish it - until you do, only you can
   see it.

Code signing is the only thing that needs secrets, and the workflows skip
it when they are absent: a build from a fork is ad-hoc signed and opens
with the platform's usual warning for unsigned software. Linux packages
are unaffected.

## Building from source on 4 GB

A build needs Rust 1.93+ (see `src/rust-toolchain.toml`), the FFmpeg 7+
development libraries, cmake and a C++ compiler. On Debian, Ubuntu or
Mint:

```sh
sudo apt install build-essential cmake clang libavcodec-dev libavformat-dev \
     libavfilter-dev libavutil-dev libswscale-dev libswresample-dev \
     libasound2-dev pkg-config
```

(Fedora: `sudo dnf install build-essential cmake clang ffmpeg-devel alsa-lib-devel`.)

Then:

```sh
cd src
cargo run --profile quick -p concat --jobs 2
```

`--profile quick` matters on a small machine: it optimises the engine
crates the same way the shipping build does, but skips the fat LTO pass
that wants several gigabytes of memory and an hour of a potato's time.
The `--jobs 2` keeps two `rustc`s from squabbling over your RAM; expect
the first build to take an hour or three. Later builds are incremental
and much faster.

**Give yourself swap first.** Even the quick profile has moments where one
crate wants more memory than a 4 GB machine has free, and a compile that
dies to the out-of-memory killer has to start over. Zram (compressed swap
in RAM) is the kindest form:

```sh
sudo apt install zram-tools        # Debian/Ubuntu/Mint; ~half your RAM as swap
```

For a shipping binary, `cargo build --profile app -p concat` is the real
thing - fat LTO, stripped, the works - but on 4 GB it needs the swap above
and a great deal of patience. If you are only building for yourself,
`--profile quick` produces a binary you will not feel the difference of.

## Editing 20-minute videos on 4 GB

The editor meets you more than halfway if you meet it a little too:

- **Let the proxies finish.** Any file larger than full HD gets a
  quarter-size working copy written in the background the first time you
  bring it in; playback reads the copy while the paused monitor and the
  export read the original. On a potato this is *the* feature - a 4K phone
  clip goes from a few frames a second to smooth scrubbing. Watch the
  first import's background work finish before you judge playback.
- **Keep the preview at half.** The monitor's quality picker (half is the
  default) is the biggest single lever on decode and composite cost. The
  export always renders at full quality whatever the preview shows.
- **Whisper model sizes are memory sizes.** For auto-captions on 4 GB,
  `tiny` (~75 MB) and `base` (~150 MB) are the comfortable choices;
  `small` (~470 MB) works but leaves less room while a transcription
  runs. The model stays loaded between runs, ready for the next clip.
- **Close the browser.** On a 4 GB machine, FUNCUT fits comfortably with
  the desktop - and does not fit beside forty tabs. The app itself is
  sized to leave the desktop its share; everything past that is yours.
- **zram, again.** If you skipped it above: `sudo apt install zram-tools`.
  Not because the app wants to swap - because the day something else
  eats memory, the machine degrades instead of dying, and your unsaved
  edit survives.

## Syncing with upstream

This fork lives a `git remote` away from everything upstream does next:

```sh
git remote add upstream https://github.com/jub0t/Concat.git
git fetch upstream
git merge upstream/main   # or rebase, your call
```

The changes this fork carries are small and local - one new module in
`concat-media`, a handful of constants that became functions of the
machine - so merges should stay quiet. When upstream bumps the version in
`src/Cargo.toml`, tag yours with the same number and a new suffix
(`v0.3.0-funcut.1`) to keep the Release workflow's version check happy.
