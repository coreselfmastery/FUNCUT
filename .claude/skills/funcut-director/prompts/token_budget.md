# Token budget — a 10-minute cut without losing the film

There is no tokenizer in the editor. These are planning estimates from the
shape of the tool calls, not a meter: treat them as a budget, then measure the
run's own usage when the harness reports it.

## The estimate

| Runtime | Typical | Loop-heavy | Hard stop |
| --- | --- | --- | --- |
| 60 s | 35–70k | 90k | 120k |
| 3 min | 90–180k | 240k | 300k |
| 10 min | 250–500k | 700k+ | 800k |

A 10-minute film usually carries 1,300–1,800 spoken words, 60–140 picture
segments, 8–20 motion beats, 8–20 stock slots and 3–6 critique passes.
The estimate breaks down roughly as:

| Work | Tokens |
| --- | --- |
| Skill, playbooks and the one-click prompt | 20–35k |
| Transcript, word times and `voice_signals` | 15–40k |
| Edit plan and its validation | 10–20k |
| Stock search, contact sheets and ranking | 50–120k |
| Geneva inspect/frame/bake/reel | 40–120k |
| Captions, sound, grade and their proofs | 15–40k |
| `watch_render` + `craft_critique` passes | 60–150k |
| Revisions and re-watches | 30–100k |

## Where tokens actually go

Not into the writing. Most of the bill is **tool output that is read more
than once**:

1. The whole `voice_signals` JSON pasted into three later prompts.
2. Every stock candidate's full metadata instead of the contact sheet and
   the top three.
3. A full comp JSON printed in the reply, then printed again in the plan.
4. The same `craft_critique` sheet re-read after every small edit.
5. Re-reading `SKILL.md` and all playbooks on every phase.
6. A long transcript re-sent with every caption call.

## The reduction rules

- **Read once, digest once.** Keep a compact working note per section:
  `emotion/act`, 2–3 signal readings, the chosen cut type, the picture, the
  sound. Do not re-paste the source JSON.
- **Read the matrix, not the whole profile, at decision time.** Use
  `signal_recipes.md` to ask for the six readings a seam needs; the full
  profile stays in the tool result, not in every prompt.
- **Contact sheet first, candidates second.** `stock_contact_sheet` one
  sheet per slot; only the top 3 candidates get their metadata read.
- **One plan, one source of truth.** Validate the plan once; after that,
  change only the segment the critique names.
- **Motion: inspect the comp, not the catalogue.** `motion_inspect` the one
  preset; `motion_frame` three stills; bake once at cut size. Do not re-render
  a comp to "see if it works".
- **Critique in passes, not per edit.** `watch_render` the whole cut, one
  critique, one high-leverage revision, one re-watch. A third pass only if a
  gate is still red.
- **Use a subagent for read-only fan-out.** Stock ranking, one motion beat,
  one sound search and a fresh-context critic can each return a short JSON
  verdict instead of filling the Director's context with raw results.
- **Cap the fan-out.** 3 stock candidates per slot, 2 motion options per
  beat, 2 sound options per accent. More choices rarely improve the cut.
- **Batch edits.** One `edit_apply` batch per section, not one per clip.
- **Stop at the hard stop.** If the budget is gone, ship the strongest
  timeline you have, name what is unmeasured and what was not attempted.
  A finished 7/10 cut beats an unfinished 9/10.

## What must never be cut to save tokens

- The **first three seconds** and the **turn** — the two places the film is
  won.
- The **voice read** before the cut; cutting blind is the expensive mistake.
- The **contact sheet** before a stock decision; a text-only stock choice is
  a guess.
- The **frame proof** before a motion bake; a broken comp costs more to
  discover later.
- The **whole-cut watch** before handoff; a local fix can hide a global sag.
- The **licence provenance** and the **honest report**; those are not
  optional output.
