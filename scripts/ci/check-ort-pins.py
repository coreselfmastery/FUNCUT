#!/usr/bin/env python3
"""Keep the ONNX Runtime version in every build workflow aligned with deps.lock."""

from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
LOCK = ROOT / "scripts" / "ci" / "deps.lock"
WORKFLOWS = (
    ROOT / ".github" / "workflows" / "ci.yml",
    ROOT / ".github" / "workflows" / "geneva.yml",
    ROOT / ".github" / "workflows" / "build-app.yml",
)
ROWS = {
    "onnxruntime-linux-x64": "x64",
    "onnxruntime-linux-aarch64": "aarch64",
}
DIGEST = re.compile(r"^[0-9a-f]{64}$")


def main() -> None:
    entries: dict[str, tuple[str, str]] = {}
    for raw in LOCK.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        fields = line.split()
        if len(fields) >= 3:
            entries[fields[0]] = (fields[1], fields[2])

    versions: dict[str, str] = {}
    for path in WORKFLOWS:
        text = path.read_text(encoding="utf-8")
        found = re.findall(r"^\s*ORT_VERSION:\s*([0-9]+\.[0-9]+\.[0-9]+)\s*$", text, re.MULTILINE)
        if len(found) != 1:
            raise SystemExit(f"{path.relative_to(ROOT)} must declare exactly one ORT_VERSION")
        versions[path.name] = found[0]

    unique = set(versions.values())
    if len(unique) != 1:
        raise SystemExit(f"workflow ORT_VERSION values disagree: {versions}")
    version = unique.pop()

    for name, arch in ROWS.items():
        if name not in entries:
            raise SystemExit(f"deps.lock is missing {name}")
        url, digest = entries[name]
        expected = f"onnxruntime-linux-{arch}-{version}.tgz"
        if f"/download/v{version}/{expected}" not in url:
            raise SystemExit(f"{name} URL does not match ORT_VERSION {version}: {url}")
        if not DIGEST.fullmatch(digest):
            raise SystemExit(f"{name} does not have a lowercase SHA-256 digest")
        print(f"{name}: ONNX Runtime {version} URL and SHA-256 pin are aligned")

    print(f"all build workflows use ONNX Runtime {version}")


if __name__ == "__main__":
    main()
