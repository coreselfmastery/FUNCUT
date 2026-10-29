#!/usr/bin/env node
// Checks whether the upstream repo the clip-skills vendor was taken from
// (appergb/ClipSkills) has moved past the pinned commit.
//
//   node scripts/clipskills/check-upstream.mjs        # report drift (exit 1 on drift)
//   node scripts/clipskills/check-upstream.mjs --pin   # re-pin the manifest to HEAD
//   node scripts/clipskills/check-upstream.mjs --json  # machine-readable
//
// Requires Node 18+ (built-in fetch). Set GITHUB_TOKEN to avoid rate limits.
// The FUNCUT delta on top of the pin is documented in docs/CLIPSKILLS.md —
// re-vendor and re-apply it when this reports drift.
import { readFileSync, writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";

const here = dirname(fileURLToPath(import.meta.url));
const manifestPath = join(here, "upstreams.json");
const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));

const pin = process.argv.includes("--pin");
const asJson = process.argv.includes("--json");

const headers = {
  "User-Agent": "funcut-clipskills-watch",
  Accept: "application/vnd.github+json",
};
if (process.env.GITHUB_TOKEN) headers.Authorization = `Bearer ${process.env.GITHUB_TOKEN}`;

const { repo } = manifest;
let latest = null;
let error = null;
try {
  const res = await fetch(`https://api.github.com/repos/${repo}/commits?per_page=1`, { headers });
  if (!res.ok) throw new Error(`HTTP ${res.status}`);
  const [c] = await res.json();
  latest = c ? { sha: c.sha, date: c.commit.committer.date } : null;
} catch (e) {
  error = e.message;
}

const pinned = manifest.pin?.upstream ?? null;
// short SHAs are allowed in the manifest — a prefix match counts as pinned
const sameCommit = (a, b) => !a || !b ? false : a === b || a.startsWith(b) || b.startsWith(a);
const drifted = Boolean(!error && latest && !sameCommit(pinned, latest?.sha));

if (pin && !error && latest) {
  manifest.pin = { ...manifest.pin, upstream: latest.sha, date: latest.date };
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2) + "\n");
}

const report = {
  repo,
  pinned,
  latest: latest?.sha ?? null,
  latestDate: latest?.date ?? null,
  drifted,
  error,
  note: "drift does not include the FUNCUT delta; see docs/CLIPSKILLS.md for the re-vendor procedure",
};

if (asJson) {
  console.log(JSON.stringify(report, null, 2));
} else {
  if (error) console.error(`error checking ${repo}: ${error}`);
  console.log(`${repo}: pinned ${pinned ?? "(none)"} -> latest ${report.latest ?? "?"} ${drifted ? "DRIFTED" : "ok"}`);
  if (drifted) console.log("  re-vendor per docs/CLIPSKILLS.md, re-apply the FUNCUT delta, then --pin");
}
process.exit(error || drifted ? 1 : 0);
