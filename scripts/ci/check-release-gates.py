#!/usr/bin/env python3
"""Guard the release workflow's same-tag validation contract."""
from pathlib import Path
import re

ROOT = Path(__file__).resolve().parents[2]


def fail(message: str) -> None:
    raise SystemExit(f"release-gate contract failed: {message}")


def block(text: str, header: str) -> str:
    lines = text.splitlines()
    try:
        start = lines.index(header)
    except ValueError:
        fail(f"missing section {header!r}")
    for end in range(start + 1, len(lines)):
        if re.fullmatch(r"  [A-Za-z0-9_-]+:\s*", lines[end]):
            return "\n".join(lines[start:end])
    return "\n".join(lines[start:])


release = (ROOT / ".github/workflows/release.yml").read_text()
geneva = (ROOT / ".github/workflows/geneva.yml").read_text()
build = (ROOT / ".github/workflows/build-app.yml").read_text()
nix = (ROOT / ".github/workflows/nix.yml").read_text()
security = (ROOT / ".github/workflows/security-audit.yml").read_text()

geneva_job = block(release, "  geneva:")
if "uses: ./.github/workflows/geneva.yml" not in geneva_job:
    fail("release geneva job must call the reusable Geneva workflow")
if "ref: ${{ needs.check.outputs.tag }}" not in geneva_job:
    fail("Geneva must test the exact tag selected by the release check")

publish = block(release, "  publish:")
needs_match = re.search(r"(?m)^\s+needs:\s*\[([^]]+)\]", publish)
if not needs_match:
    fail("publish job must declare its release gates")
needs = {item.strip() for item in needs_match.group(1).split(",")}
required = {"check", "engine", "geneva", "build", "nix", "security"}
if not required.issubset(needs):
    fail(f"publish is missing gates: {', '.join(sorted(required - needs))}")

call = block(geneva, "  workflow_call:")
if not re.search(r"(?m)^    inputs:\s*$", call) or not re.search(r"(?m)^      ref:\s*$", call):
    fail("Geneva must accept a reusable-workflow ref input")
checkouts = re.findall(
    r"(?m)^      - uses: actions/checkout@[^\n]+\n((?:        [^\n]*\n)*)",
    geneva,
)
if len(checkouts) != 2 or any(
    "ref: ${{ inputs.ref || github.ref }}" not in checkout
    for checkout in checkouts
):
    fail("both Geneva jobs must check out the requested tag/ref")

for name, workflow in (
    ("Build app", build),
    ("Nix", nix),
    ("RustSec", security),
):
    pull_request = block(workflow, "  pull_request:")
    if ".github/workflows/release.yml" not in pull_request:
        fail(f"{name} must run on PRs that change release.yml")

print("release gates verified: CI, Geneva, Linux packages, Nix and RustSec")
