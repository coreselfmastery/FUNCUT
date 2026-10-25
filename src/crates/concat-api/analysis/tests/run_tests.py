#!/usr/bin/env python3
"""Runs the three analysis tools against the synthetic fixtures and asserts
sane output. Usage:  python3 tests/run_tests.py

Exits nonzero on the first failure. Requires the dependencies in
requirements.txt (and ffmpeg for the fixtures).
"""

import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MEDIA = os.path.join(HERE, "test_media")
ROOT = os.path.dirname(HERE)


def run(tool, *args):
    script = os.path.join(ROOT, tool)
    done = subprocess.run(
        [sys.executable, script, *args],
        capture_output=True, text=True, timeout=300,
    )
    try:
        payload = json.loads(done.stdout)
    except json.JSONDecodeError:
        print(f"FAIL {tool} {' '.join(args)}: no JSON on stdout (exit {done.returncode})")
        print(done.stdout[-500:], done.stderr[-500:])
        sys.exit(1)
    if done.returncode != 0:
        print(f"FAIL {tool} {' '.join(args)}: exit {done.returncode}: {payload}")
        sys.exit(1)
    return payload


def run_status(tool, *args, env=None):
    """Runs a tool and returns (exit code, parsed stdout JSON or None)."""
    script = os.path.join(ROOT, tool)
    done = subprocess.run(
        [sys.executable, script, *args],
        capture_output=True, text=True, timeout=300, env=env,
    )
    try:
        return done.returncode, json.loads(done.stdout)
    except json.JSONDecodeError:
        return done.returncode, None


def check(name, condition, detail):
    if not condition:
        print(f"FAIL {name}: {detail}")
        sys.exit(1)
    print(f"  ok {name}: {detail}")


def main():
    scenes = os.path.join(MEDIA, "scenes.mp4")
    beats = os.path.join(MEDIA, "beats.wav")
    gaps = os.path.join(MEDIA, "gaps.wav")
    for missing in (scenes, beats, gaps):
        if not os.path.exists(missing):
            print("fixtures missing; run tests/make_test_media.sh first")
            sys.exit(2)

    print("analyze_scenes:")
    result = run("analyze_scenes.py", "--input", scenes)
    check("scene count", 2 <= result["sceneCount"] <= 4, f'{result["sceneCount"]} scenes found')
    check("cut at 2s", any(abs(s["start"] - 2.0) < 0.15 for s in result["scenes"]),
          f'first cut {result["scenes"][0]["start"]}s')
    check("times ordered", all(a["end"] <= b["start"] for a, b in zip(result["scenes"], result["scenes"][1:])),
          "contiguous")
    check("duration", 5.5 < result["duration"] <= 6.1, f'{result["duration"]}s')

    print("analyze_beats:")
    result = run("analyze_beats.py", "--input", beats)
    check("bpm near 120", 100 <= result["bpm"] <= 145, f'{result["bpm"]} bpm')
    check("beats found", len(result["beatsMs"]) >= 18, f'{len(result["beatsMs"])} beats')
    check("beat spacing", all(400 <= b - a <= 700 for a, b in zip(result["beatsMs"], result["beatsMs"][1:])),
          "60 ms tolerance around 500 ms")

    print("analyze_silence:")
    result = run("analyze_silence.py", "--input", gaps, "--min-silence-ms", "400")
    check("two gaps", 2 <= result["spanCount"] <= 3, f'{result["spanCount"]} spans')
    lengths = [s["length"] for s in result["spans"]]
    check("a ~1s and a ~2s gap", any(0.7 < l < 1.4 for l in lengths) and any(1.7 < l < 2.4 for l in lengths),
          f"lengths {lengths}")
    check("speech complement", len(result["speech"]) >= 3, f'{len(result["speech"])} speech spans')

    print("error contract:")
    code, payload = run_status("analyze_scenes.py", "--input", os.path.join(MEDIA, "nope.mp4"))
    check("missing file: exit 2 + JSON error", code == 2 and payload is not None and "error" in payload,
          f"exit {code}: {str(payload)[:60]}")

    if shutil.which("ffmpeg"):
        stripped = dict(os.environ)
        stripped["PATH"] = "/nonexistent"
        code, payload = run_status("analyze_silence.py", "--input", gaps,
                                    "--min-silence-ms", "400", env=stripped)
        check("librosa fallback finds the gaps",
              code == 0 and payload and payload.get("method") == "librosa-rms" and payload.get("spanCount", 0) >= 2,
              f"exit {code}, method {payload.get('method') if payload else '?'}, "
              f"{payload.get('spanCount') if payload else '?'} spans")

    print("ALL TESTS PASSED")


if __name__ == "__main__":
    main()
