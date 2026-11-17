# The director integration plan

A file-by-file plan to make the director use the editor FUNCUT already
has - not to add another hundred effects. It answers one question: which
capabilities exist, which the director actually calls, and what it would
take to connect the difference.

Status: **plan only.** Nothing here is implemented; the "evidence" column
is what each item must produce, not what it has.

## How this was verified

Every claim below was read out of `main` (`45c86f2e`, 2026-11-17) with
`grep`, not taken on trust. Where a prior audit said something the code
contradicts, the correction is in the next section. The point of writing
the corrections down is that a plan built on a wrong premise builds the
wrong thing.

## Corrections to the prior audit

The audit this plan follows is sound in its central finding - the
director does not shop the live stock shelves - but four of its supporting
facts are wrong against the code. Plan against these, not the audit's
version:

1. **There are two AUTO EDIT paths, and the audit conflates them.** The
   Forge **AUTO EDIT button** and the **`auto_director.py` watcher** are
   different programs that share only the sibling-project handoff.
   - The **button** runs a fixed 12-stage program written in Rust
     (`src/crates/concat-mcp/src/director_program.rs`, built by
     `auto_edit_program`). It calls **no stock tool, no OpenVid tool and
     no motion tool at all** - its whole vocabulary is
     `project_open`, `transcribe`, `voice_signals`, `measure_loudness`,
     `timeline_read`, `bgm_status`, `bgm_search`, `bgm_import`,
     `sounds_search`, `sounds_add`, `analysis_wait`, `edit_apply`,
     `export_start`, `export_wait`. Its 12 stages are
     handoff-voiceover, audio-repair, transcribe, voice-signals,
     understand, timeline, captions, music, sfx, export, verify, watch.
   - The **watcher** (`scripts/auto_director.py`, 6730 lines) polls
     `media.import` and runs a 20-phase pipeline. It **does** shop stock -
     but OpenVid only (`openvid_rank_for_slot`, `openvid_import` are in
     its `REQUIRED_TOOLS`) - and it **does** render motion (`motion_reel`).

   So "AUTO EDIT does not use Pexels/Pixabay" is true of **both** paths,
   but for the button it is not a missing provider, it is a missing
   *stage*: the button never gathers stock of any kind. The fix differs
   per path.

2. **RISK-02 is closed, not open.** The audit (copied from the older
   `docs/BUG-AUDIT-FINAL.md`) says `BgmTrack` "carries no license field at
   all" and nothing enforces the MobyGratis non-commercial term. Against
   `main` this is stale: `BgmTrack.license: BgmLicense` exists
   (`src/crates/concat-api/src/message.rs:1452`), `bgm.import` refuses a
   MobyGratis track when the edit is monetized
   (`src/crates/concat-api/src/lib.rs:2275`), `plan_validate` blocks a
   monetized plan whose bed names a `lofi-` track
   (`src/crates/concat-mcp/src/tools.rs:3764`), and the runner shops a
   monetized bed on FreePD only (`scripts/auto_director.py:572`). Do not
   spend a workstream re-closing it.

3. **The named OxiMedia crates mostly do not exist.** The audit's
   expansion table names `oximedia-scene`, `oximedia-shots`,
   `oximedia-search`, `oximedia-proxy`, `oximedia-audiopost`,
   `oximedia-qc` and `oximedia-color`. None is in `Cargo.lock` or the
   registry. The real `oximedia-*` crates this workspace resolves are:
   `oximedia-audio`, `oximedia-audio-analysis`, `oximedia-auto`,
   `oximedia-codec`, `oximedia-core`, `oximedia-cv`, `oximedia-edit`,
   `oximedia-graph`, `oximedia-graphics`, `oximedia-image`, `oximedia-io`,
   `oximedia-lut`, `oximedia-mir`, `oximedia-mixer`. Any OxiMedia work
   item must name a real crate and a real API.

4. **OxiMedia *is* wired, and the audit's "entry point nothing called"
   line is now half-false.** `concat_motion::footage::suggest_footage` is
   reachable as the `footage_plan_suggest` tool
   (`src/crates/concat-mcp/src/tools.rs:4435`, dispatch at `:2690`), and
   the **watcher** calls it (phase 6). The button does not. So the gap is
   again path-specific, not "OxiMedia is unwired".

What the audit gets right, and what this plan is built on: the button
edits voiceover, captions, music and SFX and **nothing else**; the live
rights-cleared shelves (Pexels/Pixabay, `stock_*`) are reachable by tools
and by the manual UI but by **no automatic path**; and the button's
"Motion" and "Gather" work the 17-stage vocabulary describes
(`src/crates/concat-director/src/stage.rs`) is simply not in its 12-stage
program.

One thing the audit misses, and it changes the framing of Workstreams A
and B: **the button's omission is deliberate.** The program's own doc
comment says "The B-roll, grade and motion design stay the Director
agent's: those stages' registers are simply absent"
(`src/crates/concat-mcp/src/director_program.rs:971`). The five stages the
12-stage program drops - plan, gather, motion, preview, revise - are the
judgement stages the design reserved for the agent. So A and B are not
bug fixes; they are **product decisions** to move a deterministic subset
of that judgement into the unattended button. Worth making, but make it
knowingly: a deterministic stock shopper with no creative brief will pick
plausible-but-dull B-roll, which is exactly why the design left it to the
agent.

## Workstream A - the button gathers stock (P0)

The button's program is the highest-value target: it is what the user
presses, and it currently cannot place a single B-roll clip. Add a
**Gather** stage between `understand` and `timeline`, shopping the live
shelves with the licence carried in the reply.

The first thing to fix is upstream of the shopping: **the button has no
B-roll shopping list.** `AutoEditProgramSource`
(`src/crates/concat-mcp/src/director_program.rs:48`) carries takes,
placements, an export path and a sheet name - no plan, no segments. The
B-roll segments the watcher shops come from its `plan.json`, which is the
agent's product and one of the five stages the button deliberately omits.
So the button must **derive** its segments deterministically before it can
shop them - and the tool for that already exists and is deterministic:
`footage_plan_suggest` (the OxiMedia planner, `tools.rs:4435`), which
turns a transcript and a beat grid into the dialogue gaps and monotony
runs to cover. The button already has the transcript (`transcribe`).

- `src/crates/concat-mcp/src/director_program.rs` - add a `Gather` stage
  between `Understand` and `Timeline`. First call `footage_plan_suggest`
  with the transcript the run already has, to get the slots. Then, per
  slot, a `FanStep` calling `stock_search` with the slot's topic tags and
  orientation, `stock_contact_sheet` for the human-visible candidates, and
  `stock_import` for the chosen hit. Emit each slot's chosen media id into
  the log the `Timeline` stage reads.
- `src/crates/concat-mcp/src/director_program.rs` - the `Timeline` stage
  already trims and places media; feed it the gathered ids the same way
  the watcher feeds `openvid_import`'s reply.
- `src/crates/concat-mcp/src/director_program.rs` (tests) - extend the
  stage-order assertion (the `assert_eq!(order, vec![...])` at `:1020`)
  to include `"gather"`, and add a test that a program built with no stock
  key refuses the gather stage with a named refusal rather than an empty
  timeline.
- `src/crates/concat/src/panes/forge.rs` - no signature change expected;
  the gather stage derives its slots from the transcript, not from a plan
  field. Confirm `AutoEditProgramSource` needs no new field.
- `src/crates/concat-mcp/src/tools.rs` - no change; `footage_plan_suggest`
  and `stock_*` are already registered and monetized-safe.

Evidence: implemented + connected + tested (a `--self-test`-style run with
a stubbed `stock_search` reply proves the placement). Accepted needs a real
key and a real run.

## Workstream B - the button renders motion (P1)

The button's program has no motion stage, so a data-driven brief ("revenue
up 40%") gets no chart.

Same upstream caveat as A: the button has no plan naming which segments
want a graphic. A deterministic motion stage can only key on cheap signals
it can compute itself - a segment whose transcript carries a number, a
comparison word ("double", "versus") or an enumeration ("three causes"),
detected by pattern, not judgement. That is a weak heuristic and should be
scoped as such.

- `src/crates/concat-mcp/src/director_program.rs` - add a **Motion** stage
  after `Timeline`, calling `motion_reel` (or `motion_overlay` over the
  placed plate) for each segment the transcript's own text flags as
  numeric or enumerative. Gate it so a segment with no flag produces no
  call.
- `src/crates/concat-mcp/src/director_program.rs` (tests) - a test that a
  segment whose text carries a figure produces a `motion_reel` call and a
  segment without one produces none.
- `src/crates/concat-director/src/stage.rs` - `Stage::Motion` already
  exists; confirm the label and add the program's use.

Evidence: implemented + connected + tested. Accepted needs a real Geneva
sidecar run.

## Workstream C - the watcher shops the live shelves (P0)

The watcher's stock step is OpenVid-only. For a monetized brief it
currently *refuses* rather than falling back to a cleared shelf - the
audit's "commercial-safe alternative instead of simply failing" point,
which is correct and worth doing.

- `scripts/auto_director.py` - add `stock_search` / `stock_import` to the
  B-roll gather step as a fallback when the brief is monetized (OpenVid is
  refused) or when OpenVid returns nothing. Keep `openvid_*` as the
  non-monetized default; the live shelf is the cleared path.
- `scripts/auto_director.py` - `REQUIRED_TOOLS` unchanged (the shelf is a
  fallback, not a hard dependency); add the tools to `ENRICHMENT_TOOLS` so
  an older server skips the fallback with a logged line.
- `scripts/auto_director.py` (self-test) - add a case: monetized brief,
  OpenVid refused, `stock_search` stub returns a hit, the clip is placed,
  and the reply's `license` is recorded on the segment.
- `docs/STOCK_LICENSING.md` - add the fallback to the "other shelves"
  section so the doc matches the behaviour.

Evidence: implemented + connected + tested in the self-test.

## Workstream D - Geneva linked motion clips (P1)

Today `motion_reel` flattens to an MP4 the director imports; the comp is
not kept. This is a real product gap but a large one - it touches the
project model, not just the director.

- `src/crates/concat-project` (model) - a timeline clip that carries a
  `motionComp` reference beside its rendered path: the comp document, its
  parameters and its asset references, so it can be reopened and
  re-rendered.
- `src/crates/concat/src/panes/*` - a "reopen comp" affordance on such a
  clip; re-render writes the same path the clip plays.
- `src/crates/concat-mcp/src/tools.rs` - a `motion_relink` tool, or a
  field on `motion_reel` that records the comp reference on the clip it
  places.
- Cache: an unchanged comp re-renders nothing (keyed on the comp hash).

Evidence: implemented + connected + tested + accepted (save, reopen,
re-render, export). **Do not start this before A and C land** - it is the
largest item here and the least urgent.

## Workstream E - the quality-control loop (P0)

The button's `Verify` and `Watch` stages exist; the gap is the final
**plan-vs-export** comparison the audit names.

- `src/crates/concat-mcp/src/director_program.rs` - the `Verify` stage
  already re-reads the render, measures loudness and checks the timeline;
  add the export-vs-plan duration/resolution comparison and a report of
  passed / failed / skipped checks.
- `scripts/auto_director.py` - the same report on the watcher's side, into
  its `signalLog`.

Evidence: implemented + connected + tested.

## Workstream F - OxiMedia, against real crates (P2)

Only if a real API earns its keep. Candidates that exist and are not yet
used by the director: `oximedia-audio-analysis` (audio/music analysis),
`oximedia-cv` (computer vision), `oximedia-mir` (music information
retrieval), `oximedia-lut` (colour). Each is a separate decision with its
own benchmark; none is a "connect the ecosystem" gesture.

- Rule: one crate, one real editor workflow, one regression test, one
  measured benefit. Anything that duplicates `concat-motion`'s existing
  footage engine (`suggest_footage`, `fill_from_pool`) is refused.

## Non-goals

- Re-closing RISK-02 (already closed).
- "More motion presets", "more effects", "more transitions" - the audit's
  own conclusion is that the value is in connection, not count.
- Replacing the native compositor with Geneva, or the reverse. Each draws
  what the other cannot; the plan keeps both.
- Any work that cannot be measured on the target low-memory laptop within
  its bounded CPU/RAM budget.

## The evidence model, and the tracked checklist

An item is done at four levels, in order, and no level may be skipped:

1. **Implemented** - code and UI exist.
2. **Connected** - the real editor or director calls it.
3. **Tested** - an integration test exercises the whole operation, not a
   unit in isolation.
4. **Accepted** - a real project proves the result on the intended
   hardware.

Checklist (tick only with the named evidence):

- [ ] A. Button gathers live stock - connected + tested
- [ ] B. Button renders motion - connected + tested
- [ ] C. Watcher falls back to the cleared shelf - tested in self-test
- [ ] D. Geneva linked motion clips - implemented + connected + tested
- [ ] E. Export-vs-plan QC report - tested
- [ ] F. One OxiMedia crate with a measured benefit - benchmarked
- [ ] All of the above: accepted on the target laptop

The remaining release blockers (target-hardware performance, VAAPI
acceptance, AI Director end-to-end) stay tracked on issue #82; this plan
feeds that tracker, it does not replace it.
