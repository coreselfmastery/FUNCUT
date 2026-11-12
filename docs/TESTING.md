# Testing: what runs, what is skipped, and what the number is

The test suite is counted by the CI lane that runs it, not by a session
quoting a number at a moment. This page is the count, the breakdown, and
the procedure that reproduces it, so a number quoted anywhere can be
checked against a machine instead of a memory.

## The count

As of the 2026-11-12 release tree at `6f9ab45` - the `v0.2.8-funcut.12`
cut, whose engine lane ran the workspace on the tag itself - one
hundred fifty-three tests over the 1725 the 2026-11-06 footage layer
counted, on the same fourteen skips:

| Lane | What runs | Result |
| --- | --- | --- |
| Engine, workspace | `cargo nextest run --workspace` | **1878 tests run: 1878 passed, 14 skipped** |
| Engine, documentation | `cargo test --workspace --doc` | **8 passed** |
| Engine, gRPC feature | `cargo nextest run -p concat-server --features grpc` | **30 tests run: 30 passed** |
| Analysis senses | `python3 src/crates/concat-api/analysis/tests/run_tests.py` | **21 checks ok** |
| Cloud farm | `python3 src/crates/concat-mcp/sense/farm-sync.py selftest` | **8 checks ok** |

Re-read from the newest green engine lane's own log (2026-11-12, the
release run on the `v0.2.8-funcut.12` tag, whose tree is main's tip
`6f9ab45`): **1878 tests run: 1878 passed, 14 skipped**, the same 8 and
the same 30 - a fresh green run printing the number this table holds.
(Over the 1725 the table last named came the 2026-11-10 data-graphics
layer's 26, the native-engine wave, the chart-vocabulary waves, the
`voice-signals` crate's director ears, the `geneva-adapter`, and the
motion-clip, Lottie and export-queue work of the .12 release - 153 in
all, read from the lane's own log at the tag, which is the only honest
way to hold a workspace number.)

**The Rust total: 1916** (1878 + 8 + 30). Every workspace member is
counted - the workspace's test binaries cover all eighteen crates, their
integration tests and their binaries.

The suite grows with the code: the correctness audit of 2026-11-01 added
five tests after `e737286d`, the ship-blockers and the roadmap's middle
days added more, the register-closing pass of 2026-11-03 added the
discovery exec-bit regression and the authored-`at` warning's two ways,
and the same day's checks job gained the GPU-pick assertions inside the
sense selftest beside the farm, emotion and vlm selftests it already
ran. The 2026-11-04 snapshot at `4b528cfa` - 1653, with the proxy cache
identity fix's regression test in it - and the same day's follow-up
- 1663, with the close retirement's test in the window crate and the
same-size-same-time and key-stability tests beside the sub-second one
in the host, over the seven 8d43162 added whose own lane went red on
the lint and never counted them - are the table's predecessors. The
2026-11-06 footage layer at `564acdbe` adds the sixty-two the table
counts: the slot schema's own laws and its b-roll share, the suggester's
gaps and monotony runs, the continuity gate's five rules and remedies,
the revision gate's ship line and named slots, the rank tool's blend
and its hard filters held twice, and the aliases the schema spells
beside the camelCase names. The five timing benchmarks of
2026-11-03 are
`#[ignore]`d - counted as skipped, never as run. A number in a session
or a document is a snapshot of a commit; this table names its commit,
which is the only honest way to hold one.

## What the fourteen skipped are

`nextest` skips exactly the tests the source marks `#[ignore]`, and each
of them needs something a CI runner does not have - a hand, a machine,
or a network:

| Test | Why it is skipped |
| --- | --- |
| `concat-vision` `stabilize::track::time_a_frame_at_the_analysis_size` | a timing benchmark, run by hand |
| `concat-render` `upscale::upscale_speed` | a timing benchmark, run by hand |
| `concat-motion` `bench::tests::time_the_timeline_document_for_the_whole_library` | a timing benchmark, run by hand |
| `concat-motion` `bench::tests::time_track_sampling_across_a_six_second_span` | a timing benchmark, run by hand |
| `concat-motion` `bench::tests::time_the_streaming_hash_over_a_128mib_asset` | a timing benchmark, run by hand |
| `concat-motion` `bench::tests::time_the_comps_sweep_over_a_full_cache` | a timing benchmark, run by hand |
| `concat-motion` `bench::tests::time_noise_at_the_particle_cap` | a timing benchmark, run by hand |
| `concat-host` `enhance::enhance_stage_timing` | a timing benchmark, run by hand |
| `concat-media` `decode::mediacodec_decodes_a_real_file_as_the_cpu_does` | needs Android and a sample in `CONCAT_DEVICE_SAMPLE` |
| `concat-speech` `transcribe::transcribes_a_file_with_the_installed_model` | needs a downloaded `tiny.en` and `CONCAT_TRANSCRIBE_AUDIO` |
| `concat-speech` `transcribe::captions_of_a_file` | needs a downloaded model and a sample in `CONCAT_TRANSCRIBE_AUDIO` |
| `concat-host` `openvid::the_library_end_to_end` | needs `OPENVID_INDEX` naming a real index |
| `concat-host` `magick::the_real_sources_answer` | hits the live sticker sources |
| `concat-host` `bgm::the_real_sources_answer` | hits the live music sources |

Nothing is skipped for the platform beyond these: the fork is
Linux-only, so there is no "ignored on macOS" class to carry.

## What is conditional besides the skips

- **The gRPC feature** (`concat-server`) is off by default - nothing in
  the workspace turns it on - so its 30 tests run in a lane of their own
  and are not part of the 1878.
- **GPU tests fail rather than skip** when no adapter is usable
  (`CONCAT_REQUIRE_GPU`), so a green run proves the parity suite ran on
  the runner's software Vulkan driver, not that it quietly looked the
  other way.
- **The Geneva lane** (`geneva.yml`) runs `cargo test -p concat-motion`
  and then renders the multi-layer example through the real sidecar on
  every push to `main` and every pull request; the motion crate's tests
  are counted in the 1878, and the render is the check the count cannot
  express. The lane also runs **the scene-quality regression**
  (`scripts/motion/scene-quality.py`): ten reference storyboards, each
  rendered through the sidecar at sampled instants, measured with the
  sense layer's own pixel metrics, scored with the critic's own
  heuristic, and held to a floor recorded in
  `scripts/motion/quality-manifest.json`. A change that makes a scene
  render worse - while every test stays green - fails there; a floor
  moved on purpose is `--refresh`, with the reason said in the commit.
- **The golden frames** (`scripts/motion/golden.py`, same lane): the
  committed board rendered at five fixed instants - including the whip
  pan's shared window, both scenes on screen at once - and compared
  against committed golden images with DSSIM, plus one frame rendered
  **twice and held to identical bytes** on the spot: the determinism
  contract proven per run, not asserted in prose (see
  `docs/DETERMINISM.md`). A frame that moved on purpose is `--update`,
  said in the commit that moved it.
- **The engine lane is long** (about two hours) and its concurrency group
  cancels the older run when pushes land quickly, so the freshest green
  number on `main` can lag the tip by a few commits. The run's own log -
  `Starting N tests across M binaries` - is the number; the badge is
  only a colour.

## Reconciling numbers you may have heard

Sessions have quoted **445**, **1109** and **1115** at various times.
None of the three is written anywhere in the repository, and none
matches a lane: they are snapshots of partial runs - a crate subset, or
the workspace before the motion, senses and shelves suites landed -
quoted from a terminal rather than read from CI. The count that matters
is the one the engine lane prints, at the commit it ran on, which is the
table above and its successors.

## Reproducing it

The engine lane needs the pinned toolchain (`src/rust-toolchain.toml`)
and the prebuilt libraries `scripts/ci/deps.lock` pins - FFmpeg, ONNX
Runtime, sherpa-onnx - plus the system packages `ci.yml` installs.
`.github/workflows/ci.yml` is the procedure, step by step; the short
version of the count itself:

```sh
cd src
cargo nextest run --workspace        # the 1878 lane
cargo test --workspace --doc         # the 8
cargo nextest run -p concat-server --features grpc   # the 30
```


## Measuring Geneva on the target Linux machine

Hosted CI proves that the pinned sidecar and the rendered examples work;
it does not measure the speed or memory use of a user's laptop. Run the
reproducible 1080p30 workload on the machine whose performance matters:

```sh
python3 scripts/motion/benchmark-geneva.py
```

The script builds the release `concat-motion` layers example, generates
its deterministic assets, removes the previous cache, and renders a cold
10-second, 1920x1080, 30 fps reel through the installed Geneva sidecar.
By default it writes to `~/funcut-geneva-benchmark/benchmark.json`, the
MP4, and the renderer's stdout/stderr logs; use `--output-dir DIR` to
choose another location. The report records the OS, architecture, CPU, sidecar
version, wall time, output size and sampled process-tree RSS when Linux
`/proc` is available. RSS is the sum of sampled process RSS values, so
shared pages may be counted more than once; it is an operational estimate,
not a kernel peak-memory guarantee.

Run it on AC power and close other heavy applications for a comparable
measurement. Keep the generated JSON with the machine label; do not copy
hosted-runner numbers into the target-machine row. Re-run after a sidecar,
renderer or major scene-quality change.
