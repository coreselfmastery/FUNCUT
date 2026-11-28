#!/usr/bin/env python3
"""The coverage ledger: which of the sense readings anyone actually asks.

The eight sense doors dispatch 84 actions; 76 of them read the material and
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

Layer 3 is a gate: a reading is covered when the spine asks it or the
`NOT_APPLICABLE` ledger names why it does not apply, and CI runs this with
`--require-button`. The spine asks 67 of the 76; the other 9 are answered
by another call or do not apply to the one-click material, and each says
which. A ledger entry that goes stale - the reading is asked now, or no
door defines it - fails too.
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

# Readings the one-click spine deliberately does not ask, with the reason
# each is answered by another call or does not apply to its material. A
# reading left here is a decision, not a gap: the ledger prints it and the
# `--require-button` gate counts it as covered. Anything neither asked nor
# listed here is the gap the gate exists to catch.
NOT_APPLICABLE = {
    ("sound_sense", "describe_sound"): (
        "the shelf's hits are remote until fetched; the chosen bed is "
        "described by sense_sound_model describe, and a placed accent is "
        "held to the moment by sense_sound check_placement"
    ),
    ("sound_sense", "rank_sounds"): (
        "the accent shelf is ranked by CLAP through sense_sound_model rank "
        "on the written index; rank_sounds measures local candidate files, "
        "which the shelf does not hand over before a pick"
    ),
    ("sound_model", "plan"): (
        "the accent map is measured from the delivery - emphasis peaks, "
        "emotion, room, SNR - so a script-side plan would be a second "
        "opinion laid over the measurement that already decided"
    ),
    ("captions", "evaluate_tts_quality"): (
        "auto_edit edits the caller's own voiceover and never synthesises "
        "one, so there is no generated take to score"
    ),
    ("captions", "seal_voice"): (
        "same material: no generated voice is shipped by the one-click spine"
    ),
    ("audio_repair", "normalize_loudness"): (
        "enhance_speech's measured chain ends at the loudness target; a "
        "second explicit pass would re-encode the take again for no gain"
    ),
    ("audio_repair", "remove_hiss_hum_rumble"): (
        "enhance_speech already cuts the rumble and gates the floor, and "
        "repair_by_diagnosis applies the explicit removal when the "
        "diagnosis names it"
    ),
    ("audio_repair", "analyze_music_structure"): (
        "the bed's grid comes from the analysis job's own beats "
        "(downbeatsMs); a second structure pass would decode the same file "
        "twice for a map the cut already holds"
    ),
    ("audio_repair", "identify_music"): (
        "the bed is chosen by the sound-model gate (licence, mood, CLAP) "
        "and its music-presence veto is detect_music_segments; naming the "
        "track adds nothing the mix spends"
    ),
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
    """The `sense_* {action: ...}` pairs the AUTO EDIT program issues.

    Two shapes are read: the inline `call("sense_x", json!({ "action": ...
    }))`, and the built-then-spent `let mut ask = json!({"action": ...});
    call("sense_x", ask)` that a conditional field (visual_sync's
    `motionOnsets`) needs. The built shape is matched to the `let` that
    names the same identifier, so a variable reused for a second action
    cannot borrow its neighbour's name.
    """
    text = PROGRAM.read_text()
    asked: dict[str, set[str]] = {door: set() for door in DOORS}
    pattern = re.compile(r'"(sense_[a-z_]+)"\s*,\s*(json!\(\{|\s*([a-z_]+)\))')
    for match in pattern.finditer(text):
        door = TOOL_TO_DOOR.get(match.group(1).removeprefix("sense_"))
        if door not in asked:
            continue
        action: str | None = None
        if match.group(2).startswith("json!("):
            window = text[match.end(): match.end() + 700]
            found = re.search(r'"action"\s*:\s*"([a-z_0-9]+)"', window)
            action = found.group(1) if found else None
        else:
            ident = match.group(3)
            before = text[max(0, match.start() - 1500): match.start()]
            built = re.findall(
                r"let\s+mut\s+" + re.escape(ident)
                + r"\s*=\s*json!\(\{.*?\"action\"\s*:\s*\"([a-z_0-9]+)\"",
                before,
                re.DOTALL,
            )
            action = built[-1] if built else None
        if action:
            asked[door].add(action)
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
    # A reading counts as covered when the button asks it or the ledger
    # names why it does not apply. An entry for a reading that no door
    # defines, or for one the button now asks, is stale and fails: the
    # ledger is a record of decisions, not a place to hide a gap.
    stale = sorted(
        f"{door}.{action}"
        for (door, action) in NOT_APPLICABLE
        if action not in readings.get(door, []) or action in asked.get(door, set())
    )
    reasoned = sorted(
        f"{door}.{action}"
        for (door, action) in NOT_APPLICABLE
        if f"{door}.{action}" not in stale
    )
    covered_total = asked_total + len(reasoned)

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
        "reasoned": reasoned,
        "covered": covered_total,
        "missing": total - covered_total,
        "stale": stale,
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
        print(
            f"asked by AUTO EDIT: {asked_total}  "
            f"({asked_total * 100 // max(1, total)}%)"
        )
        print(f"reasoned not applicable: {len(reasoned)}")
        print(f"covered: {covered_total}  ({covered_total * 100 // max(1, total)}%)")
        print(f"missing: {total - covered_total}")
        print()
        for door, row in ledger["doors"].items():
            unreasoned = [a for a in row["missing"] if f"{door}.{a}" not in reasoned]
            mark = "OK " if not unreasoned else "-- "
            print(
                f"{mark}{door:14} {len(row['asked']):2}/{row['readings']:2} asked"
                f"  missing: {' '.join(row['missing']) or '-'}"
            )
        for name in reasoned:
            print(f"   reasoned {name}: {NOT_APPLICABLE[tuple(name.split('.', 1))]}")
        if stale:
            print("  stale ledger entries (asked now, or no such reading): "
                  + " ".join(stale))
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
        print("Coverage is asked + reasoned: every reading is either spent by")
        print("the spine or named here with why it does not apply. CI runs")
        print("this with --require-button, so an unledgered gap fails.")

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
    if stale:
        failures.append(f"{len(stale)} stale ledger entries")
    if require_button and covered_total < total:
        failures.append(
            f"the button covers {covered_total} of {total} readings (--require-button)"
        )

    if failures:
        print("\nFAIL:", "; ".join(failures), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
