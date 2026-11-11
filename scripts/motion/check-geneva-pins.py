#!/usr/bin/env python3
"""Keep the Geneva installer URLs and SHA-256 pins aligned with CI's lockfile."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
INSTALLER = ROOT / "install-linux.sh"
LOCK = ROOT / "scripts" / "ci" / "deps.lock"


def main() -> int:
    installer = INSTALLER.read_text(encoding="utf-8")
    lock_lines = LOCK.read_text(encoding="utf-8").splitlines()
    lock = {}
    for line in lock_lines:
        fields = line.split()
        if len(fields) >= 3:
            lock[fields[0]] = (fields[1], fields[2])

    case = re.search(r'case "\$ARCH" in(.*?)\n\s*esac', installer, re.S)
    if not case:
        raise SystemExit("could not find the installer architecture case")
    block = case.group(1)

    architectures = {
        "x86_64": "geneva-linux64",
        "aarch64": "geneva-linuxarm64",
    }
    for arch, lock_key in architectures.items():
        match = re.search(
            rf"{arch}\)\s*GENEVA_URL=\"([^\"]+)\"\s*"
            rf"GENEVA_SHA256=\"([0-9a-f]{{64}})\"",
            block,
            re.S,
        )
        if not match:
            raise SystemExit(f"installer has no complete Geneva pin for {arch}")
        url, digest = match.groups()
        if lock_key not in lock:
            raise SystemExit(f"deps.lock has no {lock_key} entry")
        locked_url, locked_digest = lock[lock_key]
        if (url, digest) != (locked_url, locked_digest):
            raise SystemExit(
                f"{arch} Geneva pin differs between install-linux.sh and deps.lock"
            )
        version = re.search(r"/download/v([0-9]+\.[0-9]+\.[0-9]+)/", url)
        if not version:
            raise SystemExit(f"{arch} Geneva URL has no pinned release version")
        print(f"{arch}: Geneva {version.group(1)} URL and SHA-256 match deps.lock")

    sidecar_section = installer.split("# ── the motion sidecar", 1)[1].split(
        "# ── the motion fonts", 1
    )[0]
    if 'GENEVA_CHECKED="no-tool"' not in sidecar_section:
        raise SystemExit("installer must refuse an unchecked Geneva install")
    if "refusing to install an unverified sidecar" not in sidecar_section:
        raise SystemExit("installer must explain why an unverified sidecar was skipped")
    if "unchecked" in sidecar_section:
        raise SystemExit("installer must not retain an unchecked Geneva install path")
    print("Geneva installer integrity policy: fail closed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
