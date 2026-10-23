# FUNCUT versus Concat: what changed, and what it cost

This is the honest ledger of this fork against the upstream it stands on
([jub0t/Concat](https://github.com/jub0t/Concat), the commit this fork's `main`
is built from). Every number here is either read straight out of the code or
marked as an estimate; nothing is a benchmark this fork ran on your machine,
because the point of the changes is that the *machine decides* now.

Three kinds of change live here: **memory sizing** (the caches that had one
fixed size now scale to the machine) and **two new features** (the OpenVid
stock library, and pause removal for voiceovers). Everything else in the program is upstream's own, including
its existing small-machine machinery: proxies for large files, preview quality
tiers, hardware decode where it wins, and the swap-friendly design of the
playback cache.

## 1. Memory: fixed sizes versus the machine's own

Upstream's frame cache was already a share of installed RAM (a sixteenth,
clamped 256 MB to 1 GB). Every other cache that matters on a small machine
was a constant, tuned on workstations. This fork makes them all answer the
same question. A "small machine" is **6 GB of RAM or less** - the class of a
budget laptop - and nothing changes at all on a machine with 8 GB or more.

| Cache | Where | Upstream (raw) | FUNCUT on a small machine | On 8 GB+ |
| --- | --- | --- | --- | --- |
| Frame cache (decoded pictures) | `concat-media`, `ReaderPool::frame_budget` | 1/16 of RAM, 256 MB - 1 GB | **unchanged** (this was already right; on 4 GB it means 256 MB) | unchanged |
| Warm decoders kept for scrubbing | `concat-media`, `ReaderPool::readers_for` | 8 open decoders | **4** - a decoder holds tens of MB of its own buffers; the 5th reopens at the price of one seek | 8 |
| In-memory stills (title drags) | `concat-media`, `stills_for` | fixed 24 frames | **8** (rides the frame budget) | 16-24, by the budget |
| Title paintings (live redraw) | `concat-host`, `live_budgets` | 128 MB budget, 512 MB ceiling | **~42 MB / ~170 MB** | unchanged |
| Decoded audio spans kept mapped | `concat-host`, `memory_cache_cap` | 64 spans | **16** (they are file-backed; the kernel can evict either way) | unchanged |
| The export's own frame pool | `concat-export` | fixed 512 MB beside the encoder | **a share of RAM** like the editor's own pool | now RAM-sized too (1 GB at 16 GB+) |
| The CLI renderer's pool | `concat-cli` | fixed 512 MB | **a share of RAM** | as above |

What that means on a 4 GB laptop cutting a 20-minute YouTube video:

- The editor's caches now add up to roughly **350-450 MB** where upstream's
  fixed sizes added up to **900 MB - 1.2 GB** - beside the encoder, the GPU
  driver's own share and the desktop itself, all competing for the same 4 GB.
- The export path was the sharpest edge: upstream opened a *second*, fixed
  512 MB frame pool on top of the encoder and the compositor during an
  export. On this fork the export pool is 256 MB (the machine's share), the
  same as the editor's.
- The idle-decoder sweep (close after 10 s unused) is upstream's own and
  stays; the fork only halves how many sit warm in the first place.

What it costs on a small machine: a fifth file scrubbed back and forth
alongside four others pays one seek per jump; a ninth title dragged at once
is painted again instead of held; undo history across a long voiceover
session re-decodes a little more often. None of these are visible in normal
editing - they are the difference between *held longer* and *held at all*.

Where the detection lives: `concat-media`'s `memory` module (upstream had it
in `concat-host`; the fork moved it down one crate so the export and the
command-line renderer - which hold pools of their own - can size them for
the same machine). The line itself is `total <= 6 GiB`, tested from both
sides.

## 2. The feature: the OpenVid stock library

The library pane grew a fourth stock page beside Media and Audio:
**OpenVid** - 937,957 stock clips from the OpenVid-1M dataset on Hugging
Face (Pixabay, Pexels and CelebV footage, cut to 512x512 with a caption
and quality scores beside every clip), searched in a local index, fetched
one clip at a time. The same library is scriptable: six `openvid.*` tools
on the MCP server and JSON-RPC API, so an agent can search by caption and
quality numbers, fetch and place clips without a window.

**How it stays small.** The dataset is 7.5 TB; what this machine holds is
one SQLite index of 435 MB (six hundredths of one percent), downloaded
once from this repository's releases and checked against a digest. Every
search - captions, quality floors, camera motion, duration, shelf, frame
shape - runs against that index in milliseconds and costs no network.
The index knows each clip's byte range inside the dataset's data files,
and Hugging Face's CDN honours a Range header, so fetching a clip is one
ranged GET of the clip's own size. A clip lands in `<data>/openvid/clips`
and is copied into the project's own `openvid` folder when it is laid on
a timeline, so a project owns its files the way a sound's copy does.

**What it costs on a small machine.** The index connection keeps
SQLite's default two-megabyte page cache; nothing about the library is
resident beyond that and the page of results on screen. The captions in
the index are zstd-compressed against a dictionary the index carries, and
a page of forty decompresses in well under a millisecond. The one-time
download is the only large moment, and it resumes if it is interrupted.

**The honest parts.** The dataset's metadata carries no frame sizes, and
the clips are in fact uniformly 512x512 (OpenVid-1M was cut for
text-to-video training). The page does not take that on trust: a clip's
frame size is read from its own MP4 header the first time the clip is
fetched, and kept in a side table. The orientation filter narrows to the
clips measured so far and says so in words, and the page's note tells
someone filtering for vertical footage why nothing matches and how a
square clip still sits on a vertical timeline. Search words are matched
over the first 300 characters of each caption - the scene descriptions -
and everything past that is one `openvid_get` away.

## 3. The feature: pause removal for voiceovers

Upstream records voiceovers well - count-in, teleprompter, retake, landing
the take on a lane under the picture - but cutting the pauses out of a take
was a manual business of scrubbing and splitting. This fork adds one button.

**Where it is:** the voiceover panel, under the transport, enabled once a
take has landed ("Remove silence" / your language's words for it).

**What it does, end to end:**

1. The take's file is streamed through FFmpeg - mono, 16 kHz - and folded
   into 20 ms windows, holding only one window at a time (the waveform
   module's own doctrine: an hour-long take costs the same memory as a
   jingle). A window is *quiet* when its loudest sample is under -40 dBFS.
2. A run of quiet windows **0.35 s or longer** is a pause; shorter gaps are
   the rhythm of speech and stay. Each pause is padded 60 ms a side so word
   onsets are not clipped, and padded pauses that touch merge.
3. Two guards keep a failed take from being edited instead of retaken: a
   take with nothing loud in it yields no pauses, and so does one whose
   pauses would leave under a fifth of a second of speech.
4. The pauses are mapped through the clip's in-point and speed onto the
   timeline and applied as **one** new edit command, `removeSpans` - one
   undo step, scriptable over the JSON-RPC API like every other command.
   The lane closes up behind the cuts (the app's own ripple semantics:
   other lanes keep their timing).

**How it is built** - the part that makes it robust rather than a bolt-on:

- The measurement (`concat-media`'s `silence` module) is pure past its
  input: six unit tests cover the finding, including one that writes a real
  WAV and reads it back through FFmpeg.
- The edit (`concat-project`'s `Command::RemoveSpans`) is a first-class
  command in the engine, alongside Split and Freeze Frame: spans are
  clamped into the clip, overlaps merge, a kept piece too short to stand
  alone folds into the pause beside it, the first piece keeps the clip's
  id and every look, later pieces are minted carrying the same looks, the
  way in stays with the first piece and the way out with the last, and
  keyframes are re-windowed per piece the way a split re-windows them.
  Eleven engine tests cover it: rippling, non-rippling, head/tail pauses,
  speed mapping, fade sharing, whole-clip coverage, no-ops, sliver
  folding, unsorted overlapping spans, old-document parsing, and NaN
  refusal.
- The window thread is never blocked: the measurement runs on the project
  thread pool and comes back as a message, the same shape as every other
  long thing the panel does.

**What it deliberately does not do:** it does not touch clips other than
the last take (select a clip, use the API's `removeSpans` if you want it
generally); it does not close other lanes' gaps; and it does not analyse
anything but a constant-speed clip - a clip with a speed curve is not one a
pause's place can be mapped onto, so the button waits until that is not the
case.

## 3. The Intel graphics question, answered honestly

A Celeron's integrated GPU (Intel UHD/HD Graphics) is already used by this
program exactly where it helps, and deliberately not where it does not:

- **Drawing the preview and the export: yes, always.** The compositor runs
  on wgpu over Vulkan, and the Celeron's Intel driver (ANV) is a Vulkan
  driver. Every frame of the monitor and of an export is drawn on the iGPU
  in both upstream and this fork - nothing to enable.
- **Export encoding in HEVC or AV1: yes, automatically.** The encoder list
  tries Intel Quick Sync (`hevc_qsv`, `av1_qsv`) first when the export asks
  for hardware - many times faster than x265 on two weak cores, which is
  the difference between an export that sips CPU and one that owns the
  machine for an hour. This is upstream behaviour and stays.
- **Export encoding in H.264: on the CPU, deliberately.** x264 makes a
  better picture per bit than the hardware H.264 encoders, and H.264 is
  the compatibility choice; the codebase measured this and chose quality.
  On a Celeron that means a 20-minute 1080p H.264 export is slow - the
  honest advice is to export HEVC, which the chip does for nothing.
- **Decoding: on the CPU, deliberately.** The engine links VAAPI (the
  Linux decode API your iGPU speaks) and can be pointed at it, but two
  facts keep it off by default: upstream measured 8-bit H.264 - the format
  of essentially all YouTube footage - decoding *faster* on the CPU than
  through the iGPU-plus-copy-back path; and on Intel i915 drivers VAAPI
  has hung whole machines until reboot (upstream issue #70). A hard freeze
  is the one thing a "no compromises on this potato" build must not ship.
  Heavier codecs (HEVC, 10-bit) would win on the chip, but the safe
  default is the CPU, and the proxy system covers 4K sources with
  quarter-size copies that decode smoothly anywhere.

## 4. Everything else: verified, not changed

The rest of the fork's work was verification, and it is worth saying what
that means concretely. The full engine test suites were run against every
change: `concat-media` (125 tests), `concat-project` (174), `concat-host`
(111), `concat-export` (39), plus clippy and rustfmt clean across all
touched crates, and the locales inventory in sync for all 26 languages.
The three HDR/HLG tests that fail in that local run fail on the untouched
upstream tree in the same sandbox for the same reason (Debian's minimal
FFmpeg build lacks the HDR filters); GitHub's CI, which builds against a
full FFmpeg, is the authority and runs on every push.

Keyframes, transitions, effects, the cutout models, captions, the proxy
system, speed ramps - none of it is modified by this fork. The two things
that touch it are the memory table above (which changes *how much* is held,
never what is computed) and `removeSpans`, which is exercised by the eleven
engine tests listed above, including the keyframe and fade sharing.

## 5. The files

| File | What it is |
| --- | --- |
| `src/crates/concat-media/src/memory.rs` | new: the machine's RAM, and the small-machine line |
| `src/crates/concat-media/src/silence.rs` | new: the pause finder (streaming, pure, tested) |
| `src/crates/concat-media/src/pool.rs` | readers and stills scale with the machine |
| `src/crates/concat-host/src/titles.rs` | painting budgets scale with the machine |
| `src/crates/concat-host/src/playback.rs` | audio mappings scale with the machine |
| `src/crates/concat-project/src/commands/` | new `removeSpans` command + tests |
| `src/crates/concat/src/panes/voiceover.rs` | the button's wiring |
| `src/crates/concat/ui/dialogs/voiceover.slint` | the button itself |
| `src/crates/concat-export/`, `src/crates/concat-cli/` | pools sized for the machine |
| `install-linux.sh`, `docs/FUNCUT.md` | the one-line installer; the small-machine notes |
