# Maestro upstream drift tracking

`.claude/skills/maestro/` is vendored from
[leobbaroni/maestro](https://github.com/leobbaroni/maestro) at maestro 5.0.0,
commit `64e1d6ba443dc477547ad646c8b090afd15a0c04`. The two files here are
maestro's own drift tracker, vendored so this repository can re-sync its copy
instead of freezing it:

- `upstreams.json` — the twelve upstream knowledge bases maestro was distilled
  from, with the exact commit SHA each watched path was last distilled at
  (pins dated 2026-01 through 2026-09), which maestro reference modules each
  upstream feeds, and the authority hierarchy in its `$comment` fields.
- `check-upstreams.mjs` — the checker (Node 18+, zero dependencies, built-in
  `fetch` only).

## Check for drift

```bash
node scripts/maestro/check-upstreams.mjs
```

Unauthenticated GitHub API rate limits apply (60 requests/hour per IP); the
checker makes one request per watched path. Exit code 1 plus a per-module
DRIFT list means one or more upstream corpora changed since the pins; exit 0
means the vendor is current. Note what the pins are: maestro's own
distillation points, frozen when maestro 5.0.0 was released (2026-09-04).
Drift here means the upstream knowledge bases moved after that — it is a
signal to watch for a new maestro release and re-vendor from it, not
something to fix in this repository.

```bash
node scripts/maestro/check-upstreams.mjs --json   # machine-readable
```

## Re-vendor on drift

1. Clone or pull [leobbaroni/maestro](https://github.com/leobbaroni/maestro)
   and read its `CHANGELOG.md` and `UPDATING.md` — maestro's re-distillation
   playbook, including a paste-ready agent prompt for the update.
2. Copy its `skills/maestro/` over this repo's `.claude/skills/maestro/`
   verbatim, and carry its root `NOTICE.md` and `LICENSE` alongside (the
   `SKILL.md` footer points at `NOTICE.md`).
3. Copy the fresh `upstreams.json` over the one here.
4. Update the pinned maestro commit in `docs/MAESTRO.md` (first paragraph)
   and re-check the funcut-director skill's §6 routing table against the new
   module list (`references/` module names have changed before — maestro
   retired a corpus at 5.0.0).
5. Update `THIRD_PARTY_NOTICES.md` if the set of vendored corpora or their
   licenses changed.

Do not run `check-upstreams.mjs --pin` here: pinning records a re-distillation
that happened in the maestro repository, not in this one. This copy's pins
should always match the maestro commit named in `docs/MAESTRO.md`.
