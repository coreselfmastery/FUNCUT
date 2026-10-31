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

| Machine | Scrub | Export (300 frames) | Peak RSS | Run |
| --- | --- | --- | --- | --- |
| *not yet measured* | — | — | — | this row is replaced by the first run |

No measurement of this fork has been published yet. The claim the fork
makes is the design facts above — the caches size themselves to the
machine, and the fixed sizes that made a 4 GB laptop swap are gone — and
the words "runs on a potato laptop" elsewhere in these docs are that
design intent, not a measured result. When you run the benchmark on your
machine, the block it prints is your answer, and this table is where it
goes.
