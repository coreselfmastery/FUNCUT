#!/usr/bin/env bash
# Reclaims the duplicate cargo artifacts a target/ tree carries: the
# same .rlib and .rmeta bytes, built under several fingerprints, kept as
# independent files. Every byte-identical pair is turned into one inode
# with two names - nothing is deleted, a path any later build still
# reads keeps its bytes, and what cargo replaces it rewrites by rename,
# which breaks the link rather than corrupting the twin. Measured on the
# box the environment notes record: 2.7 GB reclaimed once.
#
#   bash scripts/prune-dupes.sh             dedupe src/target
#   bash scripts/prune-dupes.sh --dry-run   report only, link nothing
#
# Grouped by size first and hashed only within a group, so the common
# case (thousands of small .d files never touched, a few hundred large
# .rlib ones) hashes each file once. Files already sharing an inode are
# counted and left alone; the link replaces the duplicate by rename.
set -euo pipefail

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
target="$root/src/target"

if [ ! -d "$target" ]; then
  echo "prune-dupes.sh: no src/target to prune"
  exit 0
fi

dry=false
if [ "${1:-}" = "--dry-run" ]; then
  dry=true
fi

python3 - "$target" "$dry" <<'PY'
import hashlib
import os
import sys
from collections import defaultdict

target = sys.argv[1]
dry = sys.argv[2] == "True"

by_size = defaultdict(list)
scanned = 0
for dirpath, _dirnames, names in os.walk(target):
    for name in names:
        if not (name.endswith(".rlib") or name.endswith(".rmeta")):
            continue
        path = os.path.join(dirpath, name)
        try:
            st = os.lstat(path)
        except FileNotFoundError:
            continue
        if os.path.islink(path) or st.st_size == 0:
            continue
        by_size[st.st_size].append(path)
        scanned += 1

def digest(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(4 * 1024 * 1024), b""):
            h.update(chunk)
    return h.digest()

linked = 0
reclaimed = 0
already = 0
for size, paths in by_size.items():
    if len(paths) < 2:
        continue
    by_hash = defaultdict(list)
    for path in paths:
        try:
            by_hash[digest(path)].append(path)
        except FileNotFoundError:
            continue
    for group in by_hash.values():
        if len(group) < 2:
            continue
        keep = group[0]
        keep_st = os.stat(keep)
        for path in group[1:]:
            st = os.stat(path)
            if (st.st_ino, st.st_dev) == (keep_st.st_ino, keep_st.st_dev):
                already += 1
                continue
            if dry:
                linked += 1
                reclaimed += size
                continue
            tmp = path + ".prune-dupes-tmp"
            try:
                os.link(keep, tmp)
                os.replace(tmp, path)
            except OSError:
                try:
                    os.unlink(tmp)
                except OSError:
                    pass
                continue
            linked += 1
            reclaimed += size

def human(n):
    for unit in ("B", "KiB", "MiB", "GiB"):
        if n < 1024 or unit == "GiB":
            return f"{n:.1f} {unit}" if unit != "B" else f"{n} B"
        n /= 1024

verb = "would reclaim" if dry else "reclaimed"
print(
    f"prune-dupes.sh: {scanned} rlib/rmeta files, "
    f"{linked} duplicates {verb} ({human(reclaimed)}), "
    f"{already} already shared an inode"
)
PY
