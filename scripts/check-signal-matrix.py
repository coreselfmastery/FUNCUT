#!/usr/bin/env python3
"""Keep the Director signal matrix aligned with voice_signals' runtime catalogue."""

from __future__ import annotations

import re
import runpy
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOGUE = ROOT / "src/crates/concat-mcp/sense/voice_signals.py"
SKILL_DIR = ROOT / ".claude/skills/funcut-director"
MATRIX = SKILL_DIR / "prompts/signal_matrix.md"
TOOLS_RS = ROOT / "src/crates/concat-mcp/src/tools.rs"


def main() -> int:
    namespace = runpy.run_path(str(CATALOGUE))
    catalogue = namespace.get("SIGNAL_CATALOGUE")
    if not isinstance(catalogue, list):
        print(f"signal catalogue missing or invalid: {CATALOGUE}", file=sys.stderr)
        return 1

    entries = [entry for entry in catalogue if isinstance(entry, dict)]
    expected = [entry.get("name") for entry in entries]
    expected_set = set(expected)
    family_alias = {
        "Loudness": "loudness",
        "Pauses": "pause",
        "Speech rate": "speechRate",
        "Words": "words",
        "Language": "language",
        "Sentence": "sentence",
        "Entities": "entities",
        "Motifs": "motifs",
        "Fillers": "fillers",
        "Arc": "arc",
        "Prosody": "prosody",
        "Forensics": "forensic",
        "Speakers": "speaker",
        "Motion design": "motion",
    }
    expected_aliases = {
        entry["name"]: (
            "formants"
            if entry.get("name") == "forensic.formantsHz"
            else family_alias.get(entry.get("family"))
        )
        for entry in entries
    }
    families = {entry.get("family") for entry in entries if isinstance(entry.get("family"), str)}
    unmapped = sorted(families - set(family_alias))
    if unmapped:
        # The map is hand-kept; a family the map does not know ships
        # untaught and uncheckable, which is exactly how drift starts.
        print(
            "family_alias does not cover every runtime family: " + ", ".join(unmapped),
            file=sys.stderr,
        )
        return 1
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
    actual_aliases = dict(rows)

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
    wrong_aliases = sorted(
        f"{reading}: expected {expected_aliases.get(reading)!r}, got {alias!r}"
        for reading, alias in actual_aliases.items()
        if alias != expected_aliases.get(reading)
    )
    if wrong_aliases:
        errors.append("wrong signalLog family aliases: " + "; ".join(wrong_aliases))
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

    # The consumer guards: a reading with no mapped decision is a
    # dashboard row, and a family no prompt teaches is a reading the
    # Director was never told how to spend.
    decision_rows = re.findall(
        r"^\|\s*\d+\s*\|\s*`([^`]+)`\s*\|\s*`[^`]+`\s*\|\s*([^|]+)\|",
        matrix_text,
        flags=re.MULTILINE,
    )
    decisions = {reading: decision.strip() for reading, decision in decision_rows}
    placeholder = re.compile(r"^(?:-+|tbd|todo|none|n/?a|\?)$", re.IGNORECASE)
    for reading, decision in decisions.items():
        if not decision or placeholder.match(decision):
            errors.append(f"{reading}: the decision column is empty - a reading with no decision is a dashboard row")
        elif len(decision.split()) < 3:
            errors.append(f"{reading}: the decision '{decision}' is too thin to spend")
    taught: dict[str, list[str]] = {}
    for path in SKILL_DIR.rglob("*.md"):
        if path.name == MATRIX.name:
            continue
        text = path.read_text(encoding="utf-8").lower()
        for family in families:
            spent = (
                family_alias.get(family, "").lower() in text
                or family.lower() in text
            )
            if spent:
                taught.setdefault(family, []).append(path.name)
    untaught = sorted(family for family in families if family not in taught)
    if untaught:
        errors.append(
            "families no prompt or SKILL.md teaches: " + ", ".join(untaught)
            + " (a family the skill never names is a reading the Director"
            " cannot spend)"
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
