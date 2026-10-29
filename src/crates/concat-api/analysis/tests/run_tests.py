#!/usr/bin/env python3
"""Runs the three analysis tools against the synthetic fixtures and asserts
sane output. Usage:  python3 tests/run_tests.py

Exits nonzero on the first failure. Requires the dependencies in
requirements.txt (and ffmpeg for the fixtures).
"""

import json
import math
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
MEDIA = os.path.join(HERE, "test_media")
ROOT = os.path.dirname(HERE)


def _strict_constants(value):
    """serde_json, the Rust caller of all three tools, refuses the NaN and
    Infinity literals Python's json module happily writes and reads. Parsing
    with this raises on them, so a tool that prints NaN fails HERE, in the
    test, instead of as "the analysis did not print JSON" in production."""
    raise ValueError(f"not JSON to the Rust caller: {value}")


def run(tool, *args):
    script = os.path.join(ROOT, tool)
    done = subprocess.run(
        [sys.executable, script, *args],
        capture_output=True, text=True, timeout=300,
    )
    try:
        payload = json.loads(done.stdout, parse_constant=_strict_constants)
    except ValueError as error:
        print(f"FAIL {tool} {' '.join(args)}: {error}")
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
        return done.returncode, json.loads(done.stdout, parse_constant=_strict_constants)
    except ValueError:
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

    print("analyze_scenes (rapid cuts merge):")
    # red 2s, a 0.6 s blue/white/blue flicker, green 2s. The merge must
    # absorb the flicker into the scene BEFORE it and leave the 2 s green
    # scene alone - the old group-start comparison swallowed the green
    # scene into the flicker instead.
    result = run("analyze_scenes.py", "--input", os.path.join(MEDIA, "rapid.mp4"),
                 "--min-scene-len", "0.8")
    check("flicker merged into one scene", result["sceneCount"] == 2,
          f'{result["sceneCount"]} scenes: {[(s["start"], s["end"]) for s in result["scenes"]]}')
    if result["scenes"]:
        check("merge lengthens the FIRST scene to cover the flicker",
              abs(result["scenes"][0]["end"] - 2.6) < 0.15,
              f'first scene ends {result["scenes"][0]["end"]}s')
        check("the long scene after the flicker survives",
              abs(result["scenes"][-1]["start"] - 2.6) < 0.15
              and abs(result["scenes"][-1]["end"] - 4.6) < 0.15,
              f'last scene {result["scenes"][-1]["start"]}-{result["scenes"][-1]["end"]}s')

    print("analyze_beats:")
    result = run("analyze_beats.py", "--input", beats)
    check("bpm near 120", 100 <= result["bpm"] <= 145, f'{result["bpm"]} bpm')
    check("beats found", len(result["beatsMs"]) >= 18, f'{len(result["beatsMs"])} beats')
    check("beat spacing", all(400 <= b - a <= 700 for a, b in zip(result["beatsMs"], result["beatsMs"][1:])),
          "60 ms tolerance around 500 ms")

    print("analyze_beats (beatless):")
    result = run("analyze_beats.py", "--input", os.path.join(MEDIA, "silence.wav"))
    # librosa hands a NaN tempo for audio with no pulse, and the literal NaN
    # this used to print is exactly what serde_json refuses to parse.
    check("bpm is a finite number", isinstance(result["bpm"], (int, float))
          and math.isfinite(result["bpm"]), f'{result["bpm"]}')
    check("bpm is the honest zero", result["bpm"] == 0.0, f'{result["bpm"]} bpm')
    check("tempo hint explains it", "tempoHint" in result, str(result.get("tempoHint", ""))[:60])
    check("onsets still measured", isinstance(result.get("onsetsMs"), list),
          f'{len(result.get("onsetsMs", []))} onsets')

    print("analyze_silence:")
    result = run("analyze_silence.py", "--input", gaps, "--min-silence-ms", "400")
    check("two gaps", 2 <= result["spanCount"] <= 3, f'{result["spanCount"]} spans')
    lengths = [s["length"] for s in result["spans"]]
    check("a ~1s and a ~2s gap", any(0.7 < l < 1.4 for l in lengths) and any(1.7 < l < 2.4 for l in lengths),
          f"lengths {lengths}")
    check("speech complement", len(result["speech"]) >= 3, f'{len(result["speech"])} speech spans')

    print("analyze_silence (video input):")
    # -vn made video input audio-only work; it must not have made video
    # input stop working.
    result = run("analyze_silence.py", "--input", os.path.join(MEDIA, "gaps.mp4"),
                 "--min-silence-ms", "400")
    check("method is ffmpeg on a video file", result["method"] == "ffmpeg-silencedetect",
          f'{result["method"]}')
    check("same gaps through the picture track", result["spanCount"] == 2,
          f'{result["spanCount"]} spans: {[s["length"] for s in result["spans"]]}')

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
