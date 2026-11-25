#!/usr/bin/env python3
"""Every unsafe block carries a SAFETY comment saying why it is sound.

The audit counts the blocks and the comments; this keeps the two from
drifting apart. A comment may sit on the line or in the ten lines above
the block, which is enough for the multi-line explanations the engine
uses. `unsafe fn` and `unsafe impl` are not counted: their callers carry
the contract, and the audit's 108 blocks are `unsafe {` bodies.
"""
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parents[2] / "src" / "crates"
LOOK_BACK = 10
missing = []
blocks = 0
for path in sorted(ROOT.rglob("*.rs")):
    lines = path.read_text().splitlines()
    for number, line in enumerate(lines, 1):
        if "unsafe {" not in line or line.lstrip().startswith("//"):
            continue
        blocks += 1
        window = "\n".join(lines[max(0, number - 1 - LOOK_BACK) : number])
        if "SAFETY" not in window:
            missing.append(f"{path.relative_to(ROOT.parent.parent)}:{number}")
if missing:
    print("unsafe blocks without a SAFETY comment:", file=sys.stderr)
    for where in missing:
        print(f"  {where}", file=sys.stderr)
    sys.exit(1)
print(f"{blocks} unsafe blocks, each with a SAFETY comment")
