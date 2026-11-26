#!/usr/bin/env python3
"""The coverage ledger: which of the sense readings anyone actually asks.

The eight sense doors dispatch 83 actions; 75 of them read the material and
8 install, index, bench or self-test. This script holds three layers:

1. The catalogue is the code. Every reading is read out of the door
   modules' own `TOOLS` tables, so the denominator cannot drift from the
   implementation.
2. The matrix is complete. Every reading is taught in the Director's
   `signal_matrix.md`, so a reading that exists and nobody knows about is a
   failure rather than a surprise.
3. The button's coverage is measured, not asserted. Every `sense_*` call in
   `director_program.rs` is matched to the action it asks, and the ledger
   prints asked / missing per door.

Layer 3 prints and does not fail: the spine asks 10 of the 75 today, and a
check that fails on a known gap is a check that gets deleted. It becomes a
gate the day the wiring lands, by adding `--require-button` to the CI step.
"""

from __future__ import annotations

import json
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
SENSE = ROOT / "src/crates/concat-mcp/sense"
MATRIX = ROOT / ".claude/skills/funcut-director/prompts/signal_matrix.md"
PROGRAM = ROOT / "src/crates/concat-mcp/src/director_program.rs"
TOOLS_RS = ROOT / "src/crates/concat-mcp/src/tools.rs"

# The MCP tool name is `sense_<x>`; the module that answers it is not
# always `<x>_sense` (`sense_visual` -> visual_sense, `sense_sound` ->
# sound_sense), so the two are named apart rather than derived.
TOOL_TO_DOOR = {
    "visual": "visual_sense",
    "sound": "sound_sense",
    "sound_model": "sound_model",
    "voice_quality": "voice_quality",
    "prosody_style": "prosody_style",
    "captions": "captions",
    "audio_repair": "audio_repair",
    "discourse": "discourse",
}

DOORS = (
    "visual_sense",
    "sound_sense",
    "sound_model",
    "voice_quality",
    "prosody_style",
    "captions",
    "audio_repair",
    "discourse",
)

# Installing, indexing, benching and self-testing measure nothing about the
# material; they are how a reading becomes possible.
MACHINERY = {
    "models",
    "download",
    "hf_search",
    "hf_files",
    "bench",
    "index_library",
    "index",
    "selftest",
}

# Tools the skill may name that are not door readings. Kept small and
# explicit so a hallucinated reading is caught rather than waved through.
OTHER_TOOLS = {
    "measure_clips",
    "plan_validate",
    "edit_apply",
    "edit_undo",
    "watch_render",
    "craft_critique",
    "timeline_read",
    "project_open",
    "project_save",
    "project_import",
    "stock_search",
    "stock_contact_sheet",
    "sounds_search",
    "sounds_import",
    "hf_search",
    "hf_files",
    "download",
    "preview_frame",
    "motion_frame",
    "voice_signals",
    "transcribe",
    "auto_edit",
    "models_list",
    "models_download",
    "emotion",
    # door names, not readings
    "sense_visual",
    "sense_sound",
    "sense_sound_model",
    # the six roles `sense_sound` scores a file against
    "quick_transition",
    "cinematic_reveal",
    "action_impact",
    "gentle_entrance",
    "emotional_texture",
    "tension_build",
}

# The families `plan_validate` accepts for the picture (tools.rs,
# SIGNAL_VOCABULARY). A picture reading cited in a plan's signalLog has to
# roll up into one of these or the plan fails validation.
PICTURE_FAMILIES = (
    "visualMotion",
    "shotScale",
    "camera",
    "visualQuality",
    "saliency",
    "scene",
)


def tools_table(path: Path) -> list[str]:
    """The action names in a door module's `TOOLS` table, in source order.

    Read textually: these modules import numpy, onnxruntime and ffmpeg
    bindings at call time, and a documentation check must not need them.
    """
    lines = path.read_text().split("\n")
    try:
        start = next(i for i, line in enumerate(lines) if line.strip() == "TOOLS = {")
    except StopIteration:
        return []
    depth, names = 0, []
    for line in lines[start:]:
        depth += line.count("{") - line.count("}")
        match = re.match(r'\s*"([a-z_0-9]+)"\s*:', line)
        if match:
            names.append(match.group(1))
        if depth == 0:
            break
    return names


def catalogue() -> dict[str, list[str]]:
    out = {}
    for door in DOORS:
        path = SENSE / f"{door}.py"
        if not path.is_file():
            raise SystemExit(f"missing door module: {path}")
        out[door] = tools_table(path)
    return out


def spine_asked() -> dict[str, set[str]]:
    """The `sense_* {action: ...}` pairs the AUTO EDIT program issues."""
    text = PROGRAM.read_text()
    asked: dict[str, set[str]] = {door: set() for door in DOORS}
    pattern = re.compile(r'"(sense_[a-z_]+)"\s*,\s*json!\(\{')
    for match in pattern.finditer(text):
        door = TOOL_TO_DOOR.get(match.group(1).removeprefix("sense_"))
        window = text[match.end(): match.end() + 700]
        action = re.search(r'"action"\s*:\s*"([a-z_0-9]+)"', window)
        if door in asked and action:
            asked[door].add(action.group(1))
    return asked


def matrix_text() -> str:
    return MATRIX.read_text()


def taught(text: str) -> set[str]:
    return set(re.findall(r"`([a-z][a-z0-9]*(?:_[a-z0-9]+)*)`", text))


def main() -> int:
    as_json = "--json" in sys.argv
    require_button = "--require-button" in sys.argv

    cat = catalogue()
    asked = spine_asked()
    text = matrix_text()
    named = taught(text)

    readings = {
        door: [a for a in actions if a not in MACHINERY]
        for door, actions in cat.items()
    }
    total = sum(len(v) for v in readings.values())
    asked_total = sum(
        len([a for a in readings[door] if a in asked[door]]) for door in readings
    )

    untaught = sorted(
        f"{door}.{action}"
        for door, actions in readings.items()
        for action in actions
        if action not in named
    )
    unknown = sorted(
        name
        for name in named
        if "_" in name
        and name not in MACHINERY
        and name not in OTHER_TOOLS
        and not any(name in actions for actions in cat.values())
    )

    families_missing = [
        family for family in PICTURE_FAMILIES if f"`{family}`" not in text
    ]

    ledger = {
        "readings": total,
        "asked": asked_total,
        "missing": total - asked_total,
        "untaught": untaught,
        "unknown_names": unknown,
        "picture_families_missing": families_missing,
        "doors": {
            door: {
                "readings": len(readings[door]),
                "asked": sorted(a for a in readings[door] if a in asked[door]),
                "missing": sorted(a for a in readings[door] if a not in asked[door]),
            }
            for door in readings
        },
    }

    if as_json:
        print(json.dumps(ledger, indent=2, sort_keys=True))
    else:
        print(f"sense readings: {total}")
        print(f"asked by AUTO EDIT: {asked_total}  ({asked_total * 100 // max(1, total)}%)")
        print(f"missing: {total - asked_total}")
        print()
        for door, row in ledger["doors"].items():
            mark = "OK " if len(row["asked"]) == row["readings"] else "-- "
            print(
                f"{mark}{door:14} {len(row['asked']):2}/{row['readings']:2} asked"
                f"  missing: {' '.join(row['missing']) or '-'}"
            )
        print()
        print(f"taught in signal_matrix.md: {total - len(untaught)}/{total}")
        if untaught:
            print("  untaught: " + " ".join(untaught))
        if unknown:
            print("  names in the matrix that no door defines: " + " ".join(unknown))
        if families_missing:
            print("  picture families plan_validate accepts but the matrix omits: "
                  + " ".join(families_missing))
        print()
        print("The button's coverage is reported, not enforced: a check that")
        print("fails on a known gap gets deleted. Add --require-button to gate it.")

    failures = []
    if untaught:
        failures.append(f"{len(untaught)} readings are not taught in the matrix")
    if unknown:
        failures.append(f"{len(unknown)} matrix names match no door action")
    if families_missing:
        failures.append(
            f"{len(families_missing)} picture families are accepted by plan_validate "
            "and absent from the matrix"
        )
    if require_button and asked_total < total:
        failures.append(
            f"the button asks {asked_total} of {total} readings (--require-button)"
        )

    if failures:
        print("\nFAIL:", "; ".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
