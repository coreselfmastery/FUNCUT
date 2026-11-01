#!/usr/bin/env python3
"""Builds the Fluent Emoji sticker index the shelf ships.

The sticker library's index is a TSV of every emoji in Microsoft's
fluentui-emoji repository that carries a 3D PNG - 1,285 of them, all MIT -
with the words a search reads and the path a fetch asks for:

    unicode <TAB> cldr <TAB> keywords <TAB> group <TAB> license <TAB> path

Run from the repo root. It reads the repository's own tree (one GitHub
API call, no token needed at the default rate) and every emoji's own
metadata.json (1,285 small GETs), and writes the index to the path named
on the command line. Nothing is invented: the counts, the words and the
paths are what the repository holds, and the printed digest is what the
build's INDEX_SHA256 is pasted from.

    python3 scripts/fluent-emoji-index.py fluent_index.tsv
"""

import json
import sys
import urllib.parse
import urllib.request

REPO = "microsoft/fluentui-emoji"
BRANCH = "main"
API = f"https://api.github.com/repos/{REPO}"
RAW = f"https://raw.githubusercontent.com/{REPO}/{BRANCH}/"


def fetch(url: str, token: str | None = None) -> bytes:
    request = urllib.request.Request(url)
    if token:
        request.add_header("Authorization", f"Bearer {token}")
    with urllib.request.urlopen(request, timeout=60) as answer:
        return answer.read()


def main() -> None:
    out = sys.argv[1] if len(sys.argv) > 1 else "fluent_index.tsv"
    token = None  # set from the environment when the rate limit bites

    tree = json.loads(fetch(f"{API}/git/trees/{BRANCH}?recursive=1"))
    if tree.get("truncated"):
        raise SystemExit("the repository tree came back truncated; page through it by hand")

    # Every emoji folder that carries a 3D PNG, with the path a fetch asks
    # for. The other styles - Color, Flat, High Contrast - are the same
    # 1,285 at other weights; 3D is the one the shelf shows.
    folders: dict[str, str] = {}
    for entry in tree["tree"]:
        if entry["path"].startswith("assets/") and entry["path"].endswith("_3d.png"):
            folder = entry["path"].split("/")[1]
            folders[folder] = entry["path"]
    if not folders:
        raise SystemExit("no 3D PNGs found; the repository layout has moved")

    rows = []
    for folder, path in sorted(folders.items()):
        metadata = json.loads(fetch(RAW + urllib.parse.quote(f"assets/{folder}/metadata.json"), token))
        cldr = metadata.get("cldr", folder).strip()
        keywords = " ".join(metadata.get("keywords", []))
        group = metadata.get("group", "").strip()
        codepoints = [
            part for part in metadata.get("unicode", "").split() if part
        ]
        if not cldr or not codepoints:
            continue
        rows.append(
            (
                "-".join(codepoints).lower(),
                cldr,
                keywords,
                group,
                "MIT",
                path,
            )
        )

    # The same emoji twice would be a repository anomaly; said, not kept.
    ids = [row[0] for row in rows]
    if len(set(ids)) != len(ids):
        raise SystemExit("two rows share one id; look before shipping")

    with open(out, "w", encoding="utf-8") as handle:
        handle.write("unicode\tcldr\tkeywords\tgroup\tlicense\tpath\n")
        for row in rows:
            handle.write("\t".join(row) + "\n")

    import hashlib

    digest = hashlib.sha256(open(out, "rb").read()).hexdigest()
    print(f"{len(rows)} emoji written to {out}")
    print(f"sha256 {digest}")


if __name__ == "__main__":
    main()
