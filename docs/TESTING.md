# Testing: what runs, what is skipped, and what the number is

The test suite is counted by the CI lane that runs it, not by a session
quoting a number at a moment. This page is the count, the breakdown, and
the procedure that reproduces it, so a number quoted anywhere can be
checked against a machine instead of a memory.

## The count

As of `e737286d` (2026-11-01), the last commit whose engine lane ran to
green when this page was written:

| Lane | What runs | Result |
| --- | --- | --- |
| Engine, workspace | `cargo nextest run --workspace` | **1355 tests run: 1355 passed, 6 skipped** across 22 binaries |
| Engine, documentation | `cargo test --workspace --doc` | **8 passed** |
| Engine, gRPC feature | `cargo nextest run -p concat-server --features grpc` | **30 tests run: 30 passed** |
| Analysis senses | `python3 src/crates/concat-api/analysis/tests/run_tests.py` | **21 checks ok** |

**The Rust total: 1393** (1355 + 8 + 30). Every workspace member is
counted - the 22 binaries cover all seventeen crates, their integration
tests and their binaries.

The suite grows with the code: the correctness audit of 2026-11-01 added
five tests after `e737286d`, so the next green engine lane on `main`
should read 1360 + 8 + 30 = **1398**. A number in a session or a document
is a snapshot of a commit; this table names its commit, which is the only
honest way to hold one.

## What the six skipped are

`nextest` skips exactly the tests the source marks `#[ignore]`, and each
of the six needs something a CI runner does not have:

| Test | Why it is skipped |
| --- | --- |
| `concat-vision` `stabilize::track::time_a_frame_at_the_analysis_size` | a timing benchmark, run by hand |
| `concat-media` `decode::mediacodec_decodes_a_real_file_as_the_cpu_does` | needs Android and a sample in `CONCAT_DEVICE_SAMPLE` |
| `concat-speech` `transcribe::transcribes_a_file_with_the_installed_model` | needs a downloaded `tiny.en` and `CONCAT_TRANSCRIBE_AUDIO` |
| `concat-host` `openvid::the_library_end_to_end` | needs `OPENVID_INDEX` naming a real index |
| `concat-host` `magick::the_real_sources_answer` | hits the live sticker sources |
| `concat-host` `bgm::the_real_sources_answer` | hits the live music sources |

Nothing is skipped for the platform beyond these: the fork is
Linux-only, so there is no "ignored on macOS" class to carry.

## What is conditional besides the skips

- **The gRPC feature** (`concat-server`) is off by default - nothing in
  the workspace turns it on - so its 30 tests run in a lane of their own
  and are not part of the 1355.
- **GPU tests fail rather than skip** when no adapter is usable
  (`CONCAT_REQUIRE_GPU`), so a green run proves the parity suite ran on
  the runner's software Vulkan driver, not that it quietly looked the
  other way.
- **The Geneva lane** (`geneva.yml`) runs `cargo test -p concat-motion`
  and then renders the multi-layer example through the real sidecar on
  every push to `main` and every pull request; the motion crate's tests
  are counted in the 1355, and the render is the check the count cannot
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
cargo nextest run --workspace        # the 1355 lane
cargo test --workspace --doc         # the 8
cargo nextest run -p concat-server --features grpc   # the 30
```
