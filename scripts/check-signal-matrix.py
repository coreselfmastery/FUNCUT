#!/usr/bin/env python3
"""Keep the Director signal matrix aligned with voice_signals' runtime catalogue."""

from __future__ import annotations

import re
import runpy
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOGUE = ROOT / "src/crates/concat-mcp/sense/voice_signals.py"
MATRIX = ROOT / ".claude/skills/funcut-director/prompts/signal_matrix.md"
TOOLS_RS = ROOT / "src/crates/concat-mcp/src/tools.rs"


def main() -> int:
    namespace = runpy.run_path(str(CATALOGUE))
    catalogue = namespace.get("SIGNAL_CATALOGUE")
    if not isinstance(catalogue, list):
        print(f"signal catalogue missing or invalid: {CATALOGUE}", file=sys.stderr)
        return 1

    expected = [entry.get("name") for entry in catalogue if isinstance(entry, dict)]
    expected_set = set(expected)
    families = {
        entry.get("family")
        for entry in catalogue
        if isinstance(entry, dict) and isinstance(entry.get("family"), str)
    }
    matrix_text = MATRIX.read_text(encoding="utf-8")
    rows = re.findall(
        r"^\|\s*\d+\s*\|\s*`([^`]+)`\s*\|\s*`([^`]+)`\s*\|",
        matrix_text,
        flags=re.MULTILINE,
    )
    actual = [reading for reading, _ in rows]
    actual_set = set(actual)
    rust_tools = TOOLS_RS.read_text(encoding="utf-8")
    vocabulary_match = re.search(
        r"const SIGNAL_VOCABULARY:\s*&\[&str\]\s*=\s*&\[(.*?)\];",
        rust_tools,
        flags=re.DOTALL,
    )
    vocabulary = set(re.findall(r"\"([^\"]+)\"", vocabulary_match.group(1))) if vocabulary_match else set()
    aliases = {alias for _, alias in rows}

    errors: list[str] = []
    if len(expected) != len(expected_set):
        errors.append("runtime catalogue contains duplicate reading names")
    if len(actual) != len(actual_set):
        errors.append("signal matrix contains duplicate reading names")
    missing = sorted(expected_set - actual_set)
    stale = sorted(actual_set - expected_set)
    if missing:
        errors.append("missing from matrix: " + ", ".join(missing))
    if stale:
        errors.append("not in runtime catalogue: " + ", ".join(stale))
    if not vocabulary_match:
        errors.append("Rust SIGNAL_VOCABULARY could not be read")
    rejected_aliases = sorted(aliases - vocabulary)
    if rejected_aliases:
        errors.append(
            "signalLog aliases rejected by plan_validate: " + ", ".join(rejected_aliases)
        )

    declared = re.search(
        r"contains \*\*(\d+) named readings across (\d+) families\*\*",
        matrix_text,
    )
    if not declared:
        errors.append("matrix introduction must declare its reading and family counts")
    elif (int(declared.group(1)), int(declared.group(2))) != (
        len(expected),
        len(families),
    ):
        errors.append(
            "declared counts do not match runtime catalogue: "
            f"declared {declared.group(1)} readings/{declared.group(2)} families, "
            f"actual {len(expected)} readings/{len(families)} families"
        )

    if errors:
        print("Signal matrix check failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print(
        f"Signal matrix OK: {len(expected)} exact readings across "
        f"{len(families)} runtime families."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
