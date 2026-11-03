# Performance on a small machine: what is measured, and what is not

This page exists so the fork's small-machine story can be checked rather
than believed. It has two halves: the **design facts** — what the fork
changed about how the program sizes itself, each traceable to code — and
the **measurements** — numbers a particular machine produced by running
[`scripts/benchmark.py`](../scripts/benchmark.py), each labelled with the
machine it came from. Where a number has not been run yet, the table says
so, and no claim on this page rests on a number that is not in it.

## The design facts: the machine decides

Upstream Concat sized its caches for the machine it was developed on;
this fork made every one of those sizes a function of the machine it runs
on (the whole table, number by number, is
[section 1 of the change ledger](FUNCUT-CHANGES.md)):

| What | Upstream, fixed | This fork |
| --- | --- | --- |
| Title paintings held for live redrawing | 128 MB | 42 MB |
| Ceiling for every title on the timeline | 512 MB | 170 MB |
| The playback cache | one size for every machine | a share of the RAM the machine has |
| The export's own frame pool | 512 MB | a share of RAM |

The playback cache and the frame pool read the machine's memory through
`concat-host`'s `machine` module and take their share from what is there:
a 4 GB laptop gets a sixteenth of its RAM, between 256 MB and 1 GB,
where upstream gave every machine the same 512 MB whatever it had.

Those are facts about the program, not about any machine's experience of
it. For that, there is the benchmark.

## The benchmark, and how to run it

[`scripts/benchmark.py`](../scripts/benchmark.py) measures three things,
each through the program's own paths rather than a mock-up:

- **Scrub** — the JSON-RPC API's `preview.frame`, the composite the
  paused monitor shows (decode, effects, blend, scale), asked for 120
  random instants across a 60 s 1080p30 clip through one `concat-cli
  api` process. Random instants, not sequential ones, because scrubbing
  is random access.
- **Export** — `concat-cli render`, the end-to-end slice: decode,
  composite, encode, one process, 300 frames.
- **Peak RSS** — each process's own high-water mark (`VmHWM`), read
  while it lives.

```sh
python3 scripts/benchmark.py                 # generates a testsrc2 clip with ffmpeg
python3 scripts/benchmark.py --video my.mp4  # or scrubs and renders your own
```

It prints a markdown block; paste it under Results below.

## Results

First measurement, 2026-11-01, on a 2 vCPU cloud VM (Intel Xeon) with
4.1 GB of RAM and **no GPU at all**: the compositor ran on Mesa's
lavapipe software Vulkan renderer, so every number below is CPU-only —
the very machine the fork's small-machine work is for. The build is a
release `concat-cli` from this tree; the machine's own words, as the
script read them:

| Measurement | Result |
| --- | --- |
| Machine | Intel(R) Xeon(R) Processor, 4259396 kB RAM, kernel 5.10.134-013.15.kangaroo.al8.x86_64 |
| Source | a generated 60 s 1080p30 testsrc2 clip |
| Scrub: 120 random instants, `preview.frame` | **1.9 frames/s** (63.2 s in all) |
| Export: `concat-cli render`, 300 frames | **33.8 s** (8.9 frames/s) |
| Peak RSS while scrubbing | **657.3 MiB** |
| Peak RSS while rendering | **1019.6 MiB** |

Read plainly: with 4 GB and no GPU, a paused 1080p monitor answers a
random scrub in about half a second, a 1080p30 export runs at 8.9
frames a second — a five-minute video renders in about seventeen
minutes — and the heaviest phase, the export, peaks at 1.0 GiB, a
quarter of the machine's RAM. That is what the caches sizing
themselves to the machine buy: on this VM the whole edit stayed inside
its 4 GB with room to spare. A laptop with even a modest GPU moves the
compositor's work off the CPU entirely, so these numbers are the
floor, not the ceiling.

A number from your own machine belongs beside these: run
[`scripts/benchmark.py`](benchmark.py) and paste what it prints under
the row above, labelled with what it ran on.

## The farm: parallel motion renders, `--parts N`

A motion reel renders as **one document** — every scene, every layer, the
curtain and the beds — so the whole-board render is the one place a long
reel pays serially. `geneva farm` is the engine's own answer to that, and
`concat-motion-agent render <storyboard.json> -o reel.mp4 --parts N` is
FUNCUT's door onto it: the render is cut into `N` parts, each part drawn
by its own worker process, and the parts are joined **without a
re-encode** — the join is a stream copy, so the seam is invisible by
construction rather than by luck.

```sh
# the everyday door: the scene-aware cache, one run per entry
concat-motion-agent reel storyboard.json -o reel.mp4

# the long-reel door: the whole board in one pass, cut into four
concat-motion-agent render storyboard.json -o reel.mp4 --parts 4

# the long-reel door that remembers: parts that survive their own failure
concat-motion-agent render storyboard.json -o reel.mp4 --farm farm/ --parts 4
```

Even on a single machine `--parts` improves CPU utilization — four
workers fill four cores that one render leaves half idle — and the parts
are sized by the engine from the timeline's own frame count. A render
that cannot be split comes back as the engine's own `E504` refusal with
its diagnostics attached, not as a wrong file.

### Workers on other machines

`geneva farm` hands the parts to `geneva worker` processes, and the
workers can live anywhere the timeline's assets can reach:

- **A second machine** — run `geneva worker` there with the render
  directory shared (NFS, SSHFS, or a synced copy of what `Plan::write`
  stages — the folder is self-contained by design), and the farm's parts
  land where the workers are.
- **A container** — one worker per container, the render directory a
  volume; the image needs nothing but the `geneva` binary itself.
- **A serverless function** — a worker per invocation, the render
  directory in object storage; the parts join wherever the caller runs.

The one rule the engine holds everywhere: a worker never fetches
anything, and refuses asset paths that leave the timeline's own
directory — so a worker on another machine needs the *staged copy*, not a
route back to your project. `Plan::write` makes that copy; the render
directory is the contract.

`--parts` is between 1 and 64; the everyday reel verb (`reel`) does not
take it, because the scene-aware cache already renders a long reel run
by run — the farm pays where one *scene* is itself long.

### The resumable farm: `--farm DIR`

`--parts` parallelises a render; `--farm DIR` **remembers** one. The
engine's own `plan` verb chooses the part boundaries (it refuses parts
shorter than two seconds and never cuts a frame in half), and the
resumable farm renders each part with a plain `--frames` call, lands it
in DIR marked `.done`, and joins them with the timeline's audio,
without a re-encode. What makes it a different door from `--parts`:

- **A failed run costs only its own parts.** A render that dies at part
  seven of ten serves parts one to six on the way back and draws only
  what is missing — a worker that failed, a machine that slept, a
  pipeline that was cancelled — where `--parts` starts again at part
  one.
- **A part is a unit of work anywhere.** The render directory is
  self-contained (`Plan::write` stages every asset), so DIR plus the
  staged folder is all a worker on another machine needs: sync the
  folder, run the one part command, run the join back — and the reply's
  `farmReport` says part by part what drew and what was served.
- **The key is the timeline's bytes.** A changed plan — a re-graded
  scene, a moved layer — is a changed key, and every part re-renders;
  an unchanged one serves the lot and joins in no time.

The reply's `farmReport` carries each part's frame range and whether
this call drew it, the counts an agent reads at a glance, and the
seconds the whole call took. A `--parts` ceiling is a ceiling: the
engine's plan decides the real number and says so in a note.

### The cloud farm: object storage, `farm-sync.py` and `motion_farm_cloud`

The render directory is self-contained and the farm directory is
resumable, so the unit of work a worker anywhere is handed is *two
folders* - and `scripts` stops being the transport the moment the
machines share no disk. `farm-sync.py` (shipped as the MCP tool
`motion_farm_cloud`, and runnable by hand) is the transport, on S3-shaped
object storage, with nothing but the standard library:

```sh
# the orchestrator: render in resumable parts, push the whole unit
concat-motion-agent render board.json -o reel.mp4 --farm farm/ --parts 8
python3 src/crates/concat-mcp/sense/farm-sync.py push farm/ --staged cache/render

# every worker, anywhere: a part a call, until the answer says done
python3 src/crates/concat-mcp/sense/farm-sync.py work farm/ --staged cache/render

# back on the orchestrator: pull, re-run the render, serve every part, join
python3 src/crates/concat-mcp/sense/farm-sync.py pull farm/ --staged cache/render
concat-motion-agent render board.json -o reel.mp4 --farm farm/ --parts 8
```

The wire is SigV4 over HTTPS, signed with the standard AWS HMAC chain;
an upload streams a block at a time (the file is hashed once, sent once,
never held whole in memory - the same megabyte-at-a-time rule the
cache's own hashes follow), and a push HEADs each file first and skips
the ones whose stored sha256 already matches, so a re-push after a dead
worker costs what changed. `work` renders one missing part through the
sidecar with the farm's own atomic part-and-marker shapes and the
joiner's own FNV key - computed here exactly as `cache::stream_hash`
computes it, or the part would be re-rendered by the joiner for nothing
- and never touches `progress.jsonl`, which is the orchestrator's story
to tell.

Everything the storage needs comes from the environment the operator
set - `FUNCUT_FARM_BUCKET`, `FUNCUT_FARM_PREFIX`,
`FUNCUT_FARM_ENDPOINT` (MinIO, R2, a self-hosted one), the region, the
AWS credentials - and never from a tool's arguments: the same posture
the VLM endpoint allow-list takes, so a prompted agent cannot aim a
render, or the credentials beside it, at a host the operator did not
choose. The selftest proves the whole thing with no network: a mock S3
that verifies every signature it serves, and a stub sidecar that answers
the farm's two verbs.

### The cache's own lifecycle: hashed in chunks, published in one piece, held to a budget

The caches work because their keys are honest, and three rules keep
them that way (`cache.rs`, `reel.rs`, `geneva.rs`):

- **Content, hashed in chunks.** A key covers the comp's JSON *and the
  bytes of every file it names* — a re-saved logo under the same name is
  a new bake, which is the whole answer to "changed the logo, still the
  old logo". Files are hashed a megabyte at a time
  (`cache::stream_hash`), so hashing a 2 GB plate costs a megabyte of
  RAM and not the whole file — the difference between a key and an OOM
  on the 4 GB machine this renders on. The chunked hash answers exactly
  the whole-file number, so no key in the wild missed on the upgrade.
- **Published in one piece.** Every cache entry lands beside its place
  and is renamed in — a concurrent render (another process, another
  agent) never reads a half-written segment — and the `.done` marker,
  written after the rename and atomically itself, is the sentinel: a
  marker that exists promises an entry that is whole. A bake's
  `manifest.json` is its sentinel, written last the same way.
- **Held to a budget.** Every `render_reel` sweeps its own cache root
  under **2 GiB** (`cache::DEFAULT_BUDGET_BYTES`; `render_reel_budgeted`
  takes the number): the least-recently-used *complete* entries go
  first — serving one counts as a use — and junk (orphan markers,
  markerless entries, a crashed render's stale temp files) goes before
  anything useful does. The report carries what the sweep did. The
  comps bake cache (`cache/comps/`) is swept the same way by every
  `motion_bake` (`cache::sweep_comps`): a bake directory's
  `manifest.json` is its entry and its clock — a cache hit touches it,
  so a served bake counts as used — the bake just written is kept but
  counted, and an in-flight bake (no manifest yet, younger than an
  hour) is never touched. The budget is the machine's own: **2 GiB**,
  **1 GiB** on a machine with six gigabytes or less, and
  `CONCAT_MOTION_CACHE_GIB` names another.

The GPU compositor behind the project's own preview and render is the
`wgpu` path: **always built** — it is the one compositor there is
(`concat-render` depends on wgpu unconditionally; the `gpu` feature is
an empty compatibility shim so the crates that name it need not change)
— drawing on Vulkan, which every mainstream Linux driver speaks, with
Mesa's lavapipe the software adapter on a machine with no GPU at all.
Built on the window's own device, a preview frame never leaves the GPU.
The editor's compositor is therefore GPU work everywhere, by default,
with no flag to forget.

**The motion overlay's own GPU door.** The overlay path — where a baked
RGBA comp is composited over real footage and encoded (see
`motion_overlay` in MCP.md) — is ffmpeg, and its encode rides the
machine's GPU the same honest way: `h264_vaapi` where a VAAPI driver
and a render node exist, the finished frame uploaded once as nv12 and
the encoder fed in hardware; `libx264` everywhere else, with no
pretence. `gpu: auto` (the default) uses what the machine has; `on`
asks for it and falls back **with a note that says so** rather than
refusing; `off` encodes in software; `FUNCUT_OVERLAY_ENCODER` is the
operator's pin, and `FUNCUT_VAAPI_DEVICE` names another render node.
The reply says which encoder ran. The compositing arithmetic itself
stays in the filter graph — an overlay of two planes is trivial next
to the encode, and keeping it there keeps the path honest on the
machines with no GPU at all, which is half of what this fork is for.

## The motion engine's own numbers: the timing benchmarks

The editor's benchmark above measures the machine's whole pipeline on a
real video. The motion engine's hot paths have their own suite — five
timing tests, `#[ignore]`d so no lane ever fails on a clock, run by hand
in release and read as printed numbers:

```sh
cd src
cargo test -p concat-motion --release -- --ignored --nocapture bench
```

What each one measures, and the numbers a two-core sandbox printed on
3 November 2026 (a machine weaker than the laptop this fork is for, so
they are floors, not boasts):

| Benchmark | What it costs | On a two-core sandbox |
| --- | --- | --- |
| `time_the_timeline_document_for_the_whole_library` | the pure half of every bake: all 18 presets turned into the JSON the sidecar reads | 0.3 ms the library, 0.1 ms the slowest comp |
| `time_track_sampling_across_a_six_second_span` | the per-animated-property per-frame cost, 30 properties over 6 s at 30 fps | 39 ns a sample |
| `time_the_streaming_hash_over_a_128mib_asset` | the bake key over a big asset — the megabyte-at-a-time walk that keeps a 2 GB plate at a megabyte of RAM | 689 MiB a second |
| `time_the_comps_sweep_over_a_full_cache` | the LRU sweep a bounded cache runs once a bake: 48 entries, 96 MiB walked | 8 ms |
| `time_noise_at_the_particle_cap` | the organic layers at their worst, 64 motes a frame plus drift | under a microsecond a frame |

The shape the numbers draw: the document layer — the comp, its tracks,
its noise — is microseconds to fractions of a millisecond, and the
engine's real costs are the sidecar's frame draws and the disk's hashes,
which is where the farm and the cache spend their effort. The streaming
hash at two-thirds of a gigabyte a second says a 2 GB plate keys in
about three seconds, once, and serves from the cache forever after.
