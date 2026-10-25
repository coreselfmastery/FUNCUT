#!/usr/bin/env python3
"""Detect scene cuts in a video with PySceneDetect.

Called by the Concat API's `analysis.scenes` job, and runnable standalone:

    python3 analyze_scenes.py --input clip.mp4
    python3 analyze_scenes.py --input clip.mp4 --detector content --threshold 27

Writes JSON to stdout (or --output). Times are seconds.
Exit codes: 0 ok · 2 bad input · 3 missing Python dependency · 4 analysis failed.
On any failure a JSON object {"error": ..., "hint": ...} is still written to
stdout, so a machine caller can always parse one format.
"""

import argparse
import json
import os
import sys


def emit(payload, code=0, output=None):
    text = json.dumps(payload, indent=2)
    if output:
        with open(output, "w", encoding="utf-8") as handle:
            handle.write(text + "\n")
    else:
        print(text)
    sys.exit(code)


def fail(message, code=4, hint=None):
    payload = {"error": message}
    if hint:
        payload["hint"] = hint
    emit(payload, code)


def main():
    parser = argparse.ArgumentParser(description="Detect scene cuts (PySceneDetect).")
    parser.add_argument("--input", required=True, help="the video file")
    parser.add_argument("--detector", default="adaptive", choices=["adaptive", "content", "threshold"],
                        help="adaptive (default) is robust to slow lighting; content is the classic cut detector")
    parser.add_argument("--threshold", type=float, default=None,
                        help="adaptive: 3.0 default; content: 27.0 default; higher = fewer cuts")
    parser.add_argument("--min-scene-len", type=float, default=0.6,
                        help="merge cuts closer than this many seconds (default 0.6)")
    parser.add_argument("--output", default=None, help="write JSON here instead of stdout")
    args = parser.parse_args()

    if not os.path.isfile(args.input):
        emit({"error": f"no such file: {args.input}"}, 2)

    try:
        from scenedetect import ContentDetector, AdaptiveDetector, ThresholdDetector, detect
    except ImportError:
        fail("missing Python dependency: scenedetect (with an OpenCV backend)", 3,
             "pip install scenedetect opencv-python-headless")

    try:
        if args.detector == "content":
            threshold = 27.0 if args.threshold is None else args.threshold
            detector = ContentDetector(threshold=threshold)
            label = "content"
        elif args.detector == "threshold":
            threshold = 60.0 if args.threshold is None else args.threshold
            detector = ThresholdDetector(threshold=threshold)
            label = "threshold"
        else:
            threshold = 3.0 if args.threshold is None else args.threshold
            # scenedetect 0.6 named it `threshold`; 0.7+ `adaptive_threshold`.
            import inspect
            names = set(inspect.signature(AdaptiveDetector.__init__).parameters)
            if "adaptive_threshold" in names:
                detector = AdaptiveDetector(adaptive_threshold=threshold)
            else:
                detector = AdaptiveDetector(threshold=threshold)
            label = "adaptive"
        scenes = detect(args.input, detector)
    except FileNotFoundError:
        fail(f"no such file: {args.input}", 2)
    except Exception as error:  # a decode error, a corrupt file, an OpenCV failure
        fail(f"could not analyse {args.input}: {type(error).__name__}: {error}", 4,
             "the file must be a readable video (OpenCV backend)")

    # Merge runs of cuts closer than min_scene_len into their predecessors.
    merged = []
    for start, end in scenes:
        seconds = (start.get_seconds(), end.get_seconds())
        if merged and seconds[0] - merged[-1][0] < args.min_scene_len:
            merged[-1] = (merged[-1][0], seconds[1])
        else:
            merged.append(seconds)
    # A zero-length tail (possible when the video ends on a detected cut) is noise.
    merged = [(start, end) for start, end in merged if end - start > 0.05]

    payload = {
        "input": args.input,
        "detector": label,
        "threshold": threshold,
        "sceneCount": len(merged),
        "scenes": [
            {"start": round(start, 3), "end": round(end, 3), "length": round(end - start, 3)}
            for start, end in merged
        ],
    }
    if merged:
        payload["duration"] = round(merged[-1][1], 3)
    emit(payload, 0, args.output)


if __name__ == "__main__":
    main()
