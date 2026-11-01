#!/usr/bin/env python3
"""The scene-quality regression: ten reference scenes, held to floors.

The contact-sheet critic scores what a change does to a scene only when a
person runs it. This is that loop as a check: ten storyboards - the comps
that carry the engine's feature space - each rendered through the real
sidecar at sampled instants, measured with the sense layer's own pixel
metrics, scored with the critic's own heuristic, and held to a floor
recorded in quality-manifest.json. A change that makes a scene render
worse - a font that stopped loading, an entrance that stopped entering, a
grade that flattened the colour - drops its score below the floor and
fails here, in CI, instead of in a video a person already exported.

    python3 scripts/motion/scene-quality.py [--agent BIN] [--out DIR] [--refresh]

      --agent BIN   the concat-motion-agent binary (default: built from
                    src/ by the caller; see geneva.yml)
      --out DIR     where frames and the report land (default: a temp dir)
      --refresh     re-measure every scene and print the manifest rows to
                    paste back, rather than checking floors

The scorer is sense.heuristic with no audio (still frames carry no sound,
so every scene's sound axis sits at the same unmeasured 5.5 - a constant
baseline, which is what makes stills comparable). The brief is the
manifest's own, complete for every scene, so notes stay about the pixels
and not about a missing field.
"""
from __future__ import annotations

import json
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
sys.path.insert(0, str(REPO / "src" / "crates" / "concat-mcp" / "sense"))
import sense  # noqa: E402 - the critic's own metrics and heuristic

MANIFEST = HERE / "quality-manifest.json"

# A brief the heuristic accepts as complete, shared by every scene: the
# reference scenes are about the craft of the render, not the story, and
# an identical brief keeps the comparison honest.
BRIEF = {
    "audience": "creators editing on small Linux laptops",
    "platform": "short-form video",
    "emotionalArc": ["curious", "convinced", "satisfied"],
    "thesis": "a tuned editor makes motion graphics cheap",
    "curiosityGap": "what does a scene engine buy an edit?",
    "payoff": "scenes that render right, first time",
}


def render(agent: Path, board: Path, at: float, out: Path, cache: Path):
    """One frame of one storyboard, through the agent's own reel verb."""
    proc = subprocess.run(
        [str(agent), "reel", str(board), "--frame", str(at), "-o", str(out),
         "--cache", str(cache)],
        capture_output=True, text=True, timeout=300, cwd=str(REPO),
    )
    if proc.returncode != 0 or not out.exists():
        sys.stderr.write(proc.stdout + proc.stderr)
        raise SystemExit(f"the render of {board.name} at {at}s failed")
    # The agent prints its report on stdout; only the frame matters here.
    return out


def measure(frames: list[Path]):
    """The craft facts of sampled frames, built exactly as the sheet's
    own loop builds them: tile_metrics a frame, duplicates by dHash
    hamming, cut energy between neighbours, saturation averaged."""
    metas = []
    for i, path in enumerate(frames):
        rgb, w, h = sense.png_decode(path.read_bytes())
        m = sense.tile_metrics(rgb, w, h)
        m.update({"index": i, "at": i, "label": str(i + 1)})
        metas.append(m)
    dup_groups, seen = [], {}
    for m in metas:
        match = None
        for hsh, members in seen.items():
            if sense._hamming(m["hash"], hsh) <= 6:
                match = members
                break
        if match is not None:
            m["duplicateOf"] = match[0]
            match.append(m["index"])
        else:
            seen[m["hash"]] = [m["index"]]
    diffs = []
    for a, b in zip(metas, metas[1:]):
        dm = abs(a["luma"] - b["luma"]) / 25.0 + abs(a["detail"] - b["detail"]) / 8.0
        dm += sense._hamming(a["hash"], b["hash"]) / 24.0
        diffs.append(round(min(4.0, dm), 3))
        b["cutEnergy"] = diffs[-1]
    static = [i for i, d in enumerate(diffs) if d < 0.45]
    craft = {
        "samples": len(metas),
        "distinctShots": len(set(m["hash"] for m in metas)),
        "blackFrames": sum(1 for m in metas if m["blackFraction"] > 0.55),
        "letterboxedFrames": sum(1 for m in metas if m["barRatio"] > 0.6),
        "flatFrames": sum(1 for m in metas if m["contrast"] < 18),
        "overexposedFrames": sum(1 for m in metas if m.get("blownFraction", 0) > 0.3),
        "duplicateFrames": sum(1 for m in metas if "duplicateOf" in m),
        "staticTransitions": len(static),
        "meanCutEnergy": round(sum(diffs) / len(diffs), 3) if diffs else 0.0,
        "meanSaturation": round(
            sum(m.get("saturation", 0.0) for m in metas) / max(1, len(metas)), 3
        ),
    }
    return metas, craft


def main() -> int:
    agent = Path("target/debug/concat-motion-agent")
    out_dir = Path(tempfile.mkdtemp(prefix="scene-quality-"))
    refresh = "--refresh" in sys.argv
    args = sys.argv[1:]
    if "--agent" in args:
        agent = Path(args[args.index("--agent") + 1])
    if "--out" in args:
        out_dir = Path(args[args.index("--out") + 1])
        out_dir.mkdir(parents=True, exist_ok=True)
    if not agent.exists():
        print(f"no agent binary at {agent}: build it first "
              "(cargo build -p concat-motion --bin concat-motion-agent)", file=sys.stderr)
        return 2

    manifest = json.loads(MANIFEST.read_text())
    scenes = manifest["scenes"]
    failures = 0
    print(f"{'scene':32s} {'overall':>7s} {'floor':>6s}  axes")
    print("-" * 88)
    for scene in scenes:
        board = HERE / "quality" / f"{scene['id']}.storyboard.json"
        cache = out_dir / "cache"
        frames = []
        for i, at in enumerate(scene["at"]):
            frame = out_dir / f"{scene['id']}-{i}.png"
            render(agent, board, at, frame, cache)
            frames.append(frame)
        metas, craft = measure(frames)
        sheet = {"craft": craft, "tiles": metas, "audio": None, "notes": []}
        scored = sense.heuristic(BRIEF, sheet)
        overall = scored["overall"]
        axes = " ".join(f"{k}={v:.1f}" for k, v in scored["scores"].items())
        if refresh:
            print(f"{scene['id']:32s} {overall:>7.2f} {'-':>6s}  {axes}")
            scene["floor"] = scene.get("floor")
            scene["measured"] = overall
            continue
        floor = scene["floor"]
        verdict = "ok" if overall >= floor else "BELOW FLOOR"
        print(f"{scene['id']:32s} {overall:>7.2f} {floor:>6.2f}  {axes}  {verdict}")
        if overall < floor:
            failures += 1
            for note in sheet.get("notes") or []:
                print(f"    {note}")
    if refresh:
        print("\nmanifest rows (paste into quality-manifest.json, then set the floors):")
        print(json.dumps({"scenes": scenes}, indent=2))
        return 0
    if failures:
        print(f"\n{failures} scene(s) below their floor: the render got worse, "
              "not the test. Find the change, fix it, or - if the drop is "
              "intended and better - re-measure with --refresh and say why.")
        return 1
    print("\nevery scene at or above its floor")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
