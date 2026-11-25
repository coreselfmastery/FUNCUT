#!/usr/bin/env python3
"""Keep the Director's signal documentation aligned with the runtime truth.

Three layers, one exit code:

1. The voice matrix (`.claude/skills/funcut-director/prompts/signal_matrix.md`)
   stays in exact sync with `voice_signals`' runtime catalogue, and every
   alias it teaches is one `plan_validate`'s SIGNAL_VOCABULARY accepts.
2. The skill's own claims stay true: the reading/family counts and the
   tool-surface count in SKILL.md match the runtime catalogue and the
   Director allowlist, and both sound doors are taught.
3. The sound doors stay documented and correctly routed: every
   `sense_* {action: ...}` call in the skill names a member the Rust
   dispatcher actually routes through that family, the two sound doors'
   every runtime action is documented in tool_reference.md, the placement
   doctrine in sfx_design.md is the measured one, and sense/README.md's
   per-door tool counts match the modules' own TOOLS tables.
"""

from __future__ import annotations

import json
import re
import runpy
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
CATALOGUE = ROOT / "src/crates/concat-mcp/sense/voice_signals.py"
SKILL_DIR = ROOT / ".claude/skills/funcut-director"
MATRIX = SKILL_DIR / "prompts/signal_matrix.md"
SKILL = SKILL_DIR / "SKILL.md"
TOOL_REFERENCE = SKILL_DIR / "prompts/tool_reference.md"
SFX_DESIGN = SKILL_DIR / "prompts/sfx_design.md"
SENSE_README = ROOT / "src/crates/concat-mcp/sense/README.md"
EXAMPLE_PLAN = SKILL_DIR / "templates/edit_plan.example.json"
TOOLS_RS = ROOT / "src/crates/concat-mcp/src/tools.rs"
SENSE_RS = ROOT / "src/crates/concat-mcp/src/sense.rs"
MCP_TOML = ROOT / ".forge/mcp.toml"

# family door MCP name -> python module (the runtime TOOLS tables)
FAMILY_MODULES = {
    "sense_discourse": "discourse.py",
    "sense_voice_quality": "voice_quality.py",
    "sense_prosody_style": "prosody_style.py",
    "sense_audio_repair": "audio_repair.py",
    "sense_sound": "sound_sense.py",
    "sense_sound_model": "sound_model.py",
    "sense_captions": "captions.py",
    "sense_visual": "visual_sense.py",
}

WORD_NUMBERS = {
    "one": 1, "two": 2, "three": 3, "four": 4, "five": 5, "six": 6,
    "seven": 7, "eight": 8, "nine": 9, "ten": 10, "eleven": 11, "twelve": 12,
}


def load_module_tools(module: str) -> set[str]:
    namespace = runpy.run_path(str(ROOT / "src/crates/concat-mcp/sense" / module))
    tools = namespace.get("TOOLS")
    if not isinstance(tools, dict):
        raise RuntimeError(f"TOOLS table missing from {module}")
    return {name for name in tools if name != "selftest"}


def parse_rust_families() -> dict[str, list[str]]:
    rust = SENSE_RS.read_text(encoding="utf-8")
    start = rust.index("const SENSE_FAMILIES")
    end = rust.index("];", start)
    block = rust[start:end]
    families: dict[str, list[str]] = {}
    for family, members in re.findall(r'\(\s*"(\w+)",\s*&\[([^\]]*)\]', block, flags=re.DOTALL):
        families[family] = re.findall(r'"(\w+)"', members)
    return families


def director_allowlist_count() -> int:
    text = MCP_TOML.read_text(encoding="utf-8")
    match = re.search(r'CONCAT_MCP_ALLOW\s*=\s*"([^"]+)"', text)
    if not match:
        raise RuntimeError("CONCAT_MCP_ALLOW could not be read from .forge/mcp.toml")
    return len([name for name in match.group(1).split(",") if name.strip()])


def check_voice_matrix(errors: list[str]) -> tuple[int, int, dict[str, str]]:
    namespace = runpy.run_path(str(CATALOGUE))
    catalogue = namespace.get("SIGNAL_CATALOGUE")
    if not isinstance(catalogue, list):
        print(f"signal catalogue missing or invalid: {CATALOGUE}", file=sys.stderr)
        raise SystemExit(1)

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
    return len(expected), families, family_alias


def check_skill_claims(errors: list[str], readings: int, families: int) -> None:
    skill = SKILL.read_text(encoding="utf-8")
    try:
        allowlist = director_allowlist_count()
    except RuntimeError as exc:
        errors.append(str(exc))
        return

    family_count = len(families)
    for count in re.findall(r"(\d+) families", skill):
        if int(count) != family_count:
            errors.append(
                f"SKILL.md claims {count} families; the runtime catalogue has {family_count}"
            )
    for count in re.findall(r"(\d+)\+? (?:named )?readings", skill):
        if int(count) != readings:
            errors.append(
                f"SKILL.md claims {count} readings; the runtime catalogue has {readings}"
            )
    # A "-tool" claim is the served surface (the allowlist) or the whole
    # catalogue - both are true claims, each in its own words.
    catalogue = allowlist + 8  # the eight off-by-default doors
    for count in re.findall(r"(\d+)-tool", skill):
        if int(count) not in (allowlist, catalogue):
            errors.append(
                f"SKILL.md claims a {count}-tool surface; the Director allowlist "
                f"has {allowlist} and the catalogue {catalogue}"
            )
    if not re.search(r"sense_sound(?!_model)", skill):
        errors.append("SKILL.md never teaches the sense_sound door")
    if "sense_sound_model" not in skill:
        errors.append("SKILL.md never teaches the sense_sound_model door")


def check_door_routing(errors: list[str]) -> None:
    """Every `sense_* {action: "..."}` call in the skill routes a member the
    Rust dispatcher really serves through that family - a member named
    through the wrong door is refused at dispatch, so teaching one is
    teaching a broken call."""
    families = parse_rust_families()
    door_to_family = {
        "sense_discourse": "discourse",
        "sense_voice_quality": "voiceq",
        "sense_prosody_style": "prosody",
        "sense_audio_repair": "audiorepair",
        "sense_sound": "sound",
        "sense_sound_model": "soundmodel",
        "sense_captions": "captions",
        "sense_visual": "visual",
    }
    for path in sorted((ROOT / ".claude/skills/funcut-director").rglob("*.md")):
        text = path.read_text(encoding="utf-8")
        for door, action in re.findall(r'(sense_\w+) \{action: "(\w+)"', text):
            family = door_to_family.get(door)
            if family is None:
                continue
            if action not in families.get(family, []):
                owner = next(
                    (f for f, members in families.items() if action in members),
                    "no family",
                )
                errors.append(
                    f"{path.relative_to(ROOT)} routes {door} {{action: \"{action}\"}}, "
                    f"but that member belongs to the {owner} family - the dispatcher refuses it"
                )


def check_sound_door_docs(errors: list[str]) -> None:
    reference = TOOL_REFERENCE.read_text(encoding="utf-8")
    for door, module in (("sense_sound", "sound_sense.py"), ("sense_sound_model", "sound_model.py")):
        actions = load_module_tools(module)
        position = reference.find(door)
        if position < 0:
            errors.append(f"tool_reference.md never documents the {door} door")
            continue
        for action in sorted(actions):
            if f"`{action}`" not in reference[position:]:
                errors.append(
                    f"tool_reference.md documents {door} but never its `{action}` action"
                )

    design = SFX_DESIGN.read_text(encoding="utf-8")
    for token in ("check_placement", "peakAt", "rank_sounds"):
        if token not in design:
            errors.append(
                f"sfx_design.md lost its measured placement doctrine ({token} missing)"
            )


def check_readme_counts(errors: list[str]) -> None:
    readme = SENSE_README.read_text(encoding="utf-8")
    for door, module in FAMILY_MODULES.items():
        expected = len(load_module_tools(module))
        row = re.search(rf"^\| `{door}` \|[^\n]*$", readme, flags=re.MULTILINE)
        if not row:
            continue  # a door without a README row is not a count drift
        claimed = re.search(r"(\w+) tools?\b", row.group(0))
        if not claimed:
            continue
        word = claimed.group(1)
        if word in WORD_NUMBERS:
            if WORD_NUMBERS[word] != expected:
                errors.append(
                    f"sense/README.md claims {door} has {word} tools; "
                    f"the module's TOOLS table has {expected}"
                )


def check_example_plan_signal_log(errors: list[str]) -> None:
    try:
        plan = json.loads(EXAMPLE_PLAN.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as exc:
        errors.append(f"example edit plan unreadable: {exc}")
        return
    entries = plan.get("signalLog")
    if not isinstance(entries, list) or not entries:
        errors.append(
            "the example edit plan carries no signalLog - the complete worked "
            "example would fail the creativeReady gate it demonstrates"
        )


def check_consumer_guards(
    errors: list[str],
    families: set[str],
    family_alias: dict[str, str],
) -> None:
    """A reading with no mapped decision is a dashboard row, and a family
    no prompt teaches is a reading the Director was never told how to
    spend."""
    matrix_text = MATRIX.read_text(encoding="utf-8")
    decision_rows = re.findall(
        r"^\|\s*\d+\s*\|\s*`([^`]+)`\s*\|\s*`[^`]+`\s*\|\s*([^|]+)\|",
        matrix_text,
        flags=re.MULTILINE,
    )
    decisions = {reading: decision.strip() for reading, decision in decision_rows}
    placeholder = re.compile(r"^(?:-+|tbd|todo|none|n/?a|\?)$", re.IGNORECASE)
    for reading, decision in decisions.items():
        if not decision or placeholder.match(decision):
            errors.append(
                f"{reading}: the decision column is empty - a reading with no decision is a dashboard row"
            )
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


def main() -> int:
    errors: list[str] = []
    readings, families, family_alias = check_voice_matrix(errors)
    check_skill_claims(errors, readings, families)
    check_door_routing(errors)
    check_sound_door_docs(errors)
    check_readme_counts(errors)
    check_example_plan_signal_log(errors)
    check_consumer_guards(errors, families, family_alias)

    if errors:
        print("Signal matrix check failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    print(
        f"Signal matrix OK: {readings} exact "
        f"readings across {len(families)} runtime families; SKILL.md claims, door routing, "
        "sound-door docs and README counts all hold."
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
