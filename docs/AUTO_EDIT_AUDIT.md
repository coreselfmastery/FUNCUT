# AUTO EDIT — audit

> This audit predates the flat 25-phase ladder. The current Director
> contract is `SKILL.md`; the latest changes are in `FUNCUT-CHANGES.md` §0.

Every claim below is read from this tree, with the file and line that
answers it. Where a claim in the improvement list could not be reproduced,
it says so: a refuted item is as useful as a confirmed one, because it
stops work being spent on a problem that does not exist.

The two engines behind the button are real and separate, and the audit is
organised around that split:

- **The Director path** — `launch_auto_director` in
  `src/crates/concat/src/panes/forge.rs` spawns `forge run` with
  `auto_director_prompt`, and the agent follows
  `.claude/skills/funcut-director/SKILL.md` through the editor's MCP
  server (`CONCAT_MCP_ALLOW` in `.forge/mcp.toml`).
- **The fallback path** — `auto_edit_program` in
  `src/crates/concat-mcp/src/director_program.rs` builds a fixed
  17-stage program that `concat-director`'s `Runner` walks in-process
  against the window's own backend
  (`concat_mcp::backend::open(Mode::Window)`, forge.rs:757).

A finding belongs to one path or the other. Confusing the two is how a
missing capability in one looks like a missing capability in both.

## P0-1 — Is success an exit code, or a verified export?

**CONFIRMED for the Director path.** `auto_director_ended` (forge.rs:790)
branches on `code == 0` alone:

```rust
if code == 0 {
    self.auto.outcome = 0;
    self.auto.stage = t("forge.autoEditDirectorDoneStage");
    ...
    match concat_host::projects::open(&path) ...
}
```

Nothing on that path opens the export, measures it, or reads a gate
result. The doc comment above it (forge.rs:783-788) states the choice
plainly: the pane "reports the run rather than a measured export - the
Director's own `craft_critique` and its final `watch_render` are what
judged the file". That is a prompt's promise, not a measurement: an agent
that ends its turn without exporting still exits 0, and the pane opens the
project and says the edit shipped.

The **fallback path does not have this bug**. Its program ends with a
`verify` stage that probes the real export
(`director_program.rs:3452`, `motion_probe`) and gates on the loudness
reading and the words verification (`ReplyGate::True { field:
"platformTargetMet" }` and `ReplyGate::True { field: "verdict", equals:
"pass" }`, director_program.rs:3465-3474). Its terminal state carries the
verification value, and since the receipt landed
(`src/crates/concat-director/src/receipt.rs`) it also carries independent
gates and a verdict.

**So the gap is one path, and the fix is to give the Director path the
check the fallback already has** — not to build a second validator beside
the one that exists.

## P0-2 — Does the Director's allowlist serve what the skill requires?

**REFUTED as stated, CONFIRMED in a narrower and more serious form.**

The list claimed `motion_inspect` / `motion_frame` are excluded from
`.forge/mcp.toml`. They are served: both appear in the active
`CONCAT_MCP_ALLOW` at `.forge/mcp.toml:46`, and the same line serves
`motion_presets`, `motion_bake`, `motion_reel`, `motion_import` and
`motion_overlay`.

**The serious part:** before this work, `motion_probe` was omitted, and
`motion_probe` is the tool that proves an export is a readable file. The
fallback can call it because it runs in-process against the window
backend, where no allowlist applies; the Director reaches the server
through the allowlist and could not. So the Director path — the preferred
path — was the one that could not verify its own export with the tool
built for it. That is the concrete shape of P0-1: a one-line allowlist
fix plus a preflight that refuses to start when the tools the run will
need are not served.

**Fixed.** The default surface is now **106 of the 114** catalogued tools
(`DIRECTOR_ALLOWLIST` in `src/crates/concat-mcp/src/protocol.rs`, and the
same list in `.forge/mcp.toml`), and `DIRECTOR_REQUIRED` names the calls a
run cannot finish without. `director_ready` refuses the handover when the
surface does not serve one of them, and a test holds `DIRECTOR_REQUIRED`
to a subset of `DIRECTOR_ALLOWLIST` so the button can never refuse the
surface it starts with.

The nine still left off are `export_cancel`, `geneva_explain`,
`motion_farm_cloud`, `motion_farm_progress`, `motion_preview`,
`project_close`, `scene_analysis`, `scene_timeline` and `track_subject`:
each one a deliberate opt-in (a harness must never close a project or
cancel someone else's render), not a missing piece.

**No preflight exists for this.** `probe_setup` (forge.rs:2067) asks the
harness only two questions: is `funcut` an MCP server it can reach
(`forge mcp get funcut`), and does it know the skill (`forge commands`).
The start guard (`forge.rs:1815-1831`) checks a saved model key, the
connection and the skill — so **item #18 (model key absent) already
exists**, and **item #17 (required MCP tool missing) does not**.

## P0-3 — Are progress stages structured events or text matches?

**CONFIRMED: text matches.** `director_phase` (forge.rs:2038) lowercases a
line of the harness's stdout and looks for `"phase N"` with a digit
boundary, falling back to a keyword table (`DIRECTOR_PHASES`,
forge.rs:1997). The boundary check is careful — it exists precisely
because "phase 1" sits inside "phase 12" — but the mechanism is still a
substring scan of prose. A line that mentions a later phase in passing
moves the bar. The fallback path does not have this problem: its stages
are typed (`Stage`, `src/crates/concat-director/src/stage.rs:25`) and its
progress is an index into the program.

## P0-4 — Cancellation, checkpoints and resume

**CONFIRMED: the two paths are not in the same league.**

The **fallback** has a real resume record. `Runner::resume`
(runner.rs:243) seeds completed stages, `Runner::completed`
(runner.rs:226) reports them, a cancelled run rolls its applied stages out
of the record (runner.rs:1043-1045, and the test at runner.rs:1959),
and the state file is the record a restart reads. Cancel is cooperative:
`Runner::cancel` raises a flag (runner.rs:199) checked at stage
boundaries.

The **Director path** cancels by killing the child. A supervisor thread
polls `child.try_wait()` and calls `child.kill()` when the flag is up
(forge.rs:1017-1032). There is no checkpoint, no resume, and no
guarantee the agent's half-applied edits are rolled back — the edits
already landed through `edit_apply` are on the sibling project. The
sibling is kept, which is the only recovery there is.

## P0-5 — Two stock workflows, neither complete

**CONFIRMED.** The fallback (`Stage::Gather`,
director_program.rs:2251) shops live providers only: `stock_search`
(2292) → `best_hit` (330) → `stock_import` (2345), with an explicit
licence requirement in the match arm at 2332-2338. It calls
`stock_contact_sheet` **zero** times, `footage_continuity` **zero** times,
and never touches OpenVid.

The watcher (`scripts/auto_director.py`) is the mirror image: 12
`stock_contact_sheet` references, 16 `openvid_import`, 7
`captions_animate`, and no `stock_search`/`stock_import` at all — it
cannot shop a licensed shelf.

So the licensed shelf has no visual inspection, and the inspected shelf
has no licence. Neither path is complete, and a video published from
either is missing half of what the other knows.

## P1-23 / P1-25 — Motion inspection and overlay

**Both confirmed as gaps in the fallback, both already served to the
Director.**

`motion_overlay` exists (`src/crates/concat-mcp/src/tools.rs:1050`,
dispatch at 1804) and is in the allowlist, so the Director can composite
transparent motion. The fallback never calls it, and never calls
`motion_inspect` or `motion_frame` either: `Stage::Motion`
(director_program.rs:2653) reads `motion_presets` for the text-slot label,
then bakes, then places — it never previews a frame before baking, so a
preset that renders illegibly is only discovered after the bake.

## P2-12 / P2-31 — Contact sheets and animated captions in the fallback

**CONFIRMED gaps, and they are the same gap as P0-5.**
`stock_contact_sheet` and `captions_animate` are served to the Director
and absent from `director_program.rs`. The fallback places segment-timed
captions from the measured transcript (director_program.rs:2498-2530) but
never asks for word-timed kinetic captions, which is the difference the
skill's own floor describes.

## What this audit changes about the plan

The list's Step 3 ordering assumed #1 and #2 were independent. They are
not: #2's fix (serve `motion_probe`, preflight the tools) is what makes
#1's fix possible on the Director path, and the gate machinery #1 needs
already exists in `concat-director`. The corrected order is:

1. Serve `motion_probe` to the Director and preflight the run's tools.
2. Have the Director path read a receipt instead of an exit code — the
   same `AcceptanceReceipt` the fallback already writes.
3. Unify the stock stage so one path has both a licence and a contact
   sheet.
4. Word-timed captions in the fallback.
5. Structured progress events for the Director.

Item #18 (model key preflight) needs no work. Item #2's original claim
about `motion_inspect` needs no work either — the tools are served.
