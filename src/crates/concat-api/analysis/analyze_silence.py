#!/usr/bin/env python3
"""Find the silent spans of an audio or video file (dead air, pauses).

Called by the Concat API's `analysis.silence` job, and runnable standalone:

    python3 analyze_silence.py --input voiceover.wav
    python3 analyze_silence.py --input vo.mp3 --threshold-db 35 --min-silence-ms 500

Writes JSON to stdout (or --output). Times are seconds, matching the engine's
clip_cut_spans spans. The complement (`speech`) is included: the spans where
sound actually happens.
Exit codes: 0 ok · 2 bad input · 3 missing Python dependency · 4 analysis failed.
On any failure a JSON object {"error": ..., "hint": ...} is still written to
stdout, so a machine caller can always parse one format.
"""

import argparse
import json
import os
import re
import shutil
import subprocess
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


START = re.compile(r"silence_start:\s*([-0-9.]+)")
END = re.compile(r"silence_end:\s*([-0-9.]+)")


def spans_from_ffmpeg(path, threshold_db, min_seconds):
    """FFmpeg's silencedetect filter, when the ffmpeg binary is on PATH."""
    command = [
        shutil.which("ffmpeg"), "-hide_banner", "-nostdin", "-nostats",
        "-i", path,
        # -vn: silencedetect reads the audio only, and decoding the video
        # track of an hour-long file cost minutes of CPU for nothing.
        "-vn",
        "-af", "silencedetect=noise=-%gdB:d=%g" % (threshold_db, min_seconds),
        "-f", "null", "-",
    ]
    done = subprocess.run(command, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, text=True)
    if done.returncode != 0:
        tail = (done.stderr or "").strip().splitlines()[-1:]
        raise RuntimeError("ffmpeg failed: %s" % (tail[0] if tail else "no output"))
    total = re.search(r"Duration:\s*([0-9:.]+)", done.stderr or "")
    parts = [float(part) for part in total.group(1).split(":")] if total else []
    duration = parts[0] * 3600 + parts[1] * 60 + parts[2] if parts else None
    spans = []
    start = None
    for line in (done.stderr or "").splitlines():
        begin = START.search(line)
        finish = END.search(line)
        if begin:
            start = max(0.0, float(begin.group(1)))
        if finish and start is not None:
            end = float(finish.group(1))
            if end > start:
                spans.append((start, end))
            start = None
    if start is not None and duration is not None and duration > start:
        # a silence that runs to the end of the file never reports its end
        spans.append((start, duration))
    return spans, duration


def spans_from_librosa(path, threshold_db, min_seconds):
    """The librosa fallback when no ffmpeg binary is around: an RMS envelope."""
    import librosa
    import numpy
    y, sr = librosa.load(path, sr=22050, mono=True)
    if y.size == 0:
        return [], 0.0
    hop = 256
    rms = librosa.feature.rms(y=y, frame_length=1024, hop_length=hop)[0]
    # Absolute full-scale floor, matching ffmpeg's noise=-NdB: the old
    # relative floor (rms < floor * rms.max()) answered a different
    # question - the same file gave different spans depending on whether
    # an ffmpeg binary happened to be on PATH. librosa loads audio in
    # [-1, 1], so dBFS below full scale is the same arithmetic on both
    # paths. RMS sits under a window's peak, so this floor is slightly
    # stricter than ffmpeg's per-sample one; the method field says which
    # path answered.
    floor = 10.0 ** (-threshold_db / 20.0)
    quiet = rms < floor
    duration = float(y.size) / float(sr)
    spans = []
    start = None
    for index, silent in enumerate(quiet):
        time = index * hop / float(sr)
        if silent and start is None:
            start = time
        elif not silent and start is not None:
            if time - start >= min_seconds:
                spans.append((start, time))
            start = None
    if start is not None:
        spans.append((start, duration))
    return spans, duration


def main():
    parser = argparse.ArgumentParser(description="Find silent spans (dead air).")
    parser.add_argument("--input", required=True, help="the audio or video file")
    parser.add_argument("--threshold-db", type=float, default=35.0,
                        help="quiet = this many dB below full scale (35 default; 30 for a hot VO, 40 for noisy rooms)")
    parser.add_argument("--min-silence-ms", type=int, default=500,
                        help="ignore spans shorter than this (500 ms default; 350 for tight pacing)")
    parser.add_argument("--padding-ms", type=int, default=80,
                        help="reported for the caller's clip_cut_spans padding: kept, not applied")
    parser.add_argument("--output", default=None, help="write JSON here instead of stdout")
    args = parser.parse_args()

    if not os.path.isfile(args.input):
        emit({"error": f"no such file: {args.input}"}, 2)

    threshold_db = abs(args.threshold_db)
    min_seconds = max(0.05, args.min_silence_ms / 1000.0)

    try:
        if shutil.which("ffmpeg"):
            method = "ffmpeg-silencedetect"
            spans, duration = spans_from_ffmpeg(args.input, threshold_db, min_seconds)
        else:
            method = "librosa-rms"
            spans, duration = spans_from_librosa(args.input, threshold_db, min_seconds)
        if duration is None:
            duration = max([end for _, end in spans], default=0.0)
    except FileNotFoundError:
        fail(f"no such file: {args.input}", 2)
    except ImportError:
        fail("neither an ffmpeg binary nor librosa is available", 3,
             "install ffmpeg on PATH, or pip install librosa soundfile")
    except Exception as error:
        fail(f"could not analyse {args.input}: {type(error).__name__}: {error}", 4)

    spans = [(max(0.0, start), end) for start, end in spans if end - start >= min_seconds]
    speech = []
    cursor = 0.0
    for start, end in spans:
        if start - cursor >= 0.05:
            speech.append((cursor, start))
        cursor = end
    if cursor < duration:
        speech.append((cursor, duration))

    payload = {
        "input": args.input,
        "method": method,
        "thresholdDb": threshold_db,
        "minSilenceMs": args.min_silence_ms,
        "paddingMs": args.padding_ms,
        "duration": round(duration, 3),
        "spanCount": len(spans),
        "spans": [
            {"start": round(start, 3), "end": round(end, 3), "length": round(end - start, 3)}
            for start, end in spans
        ],
        "speech": [
            {"start": round(start, 3), "end": round(end, 3)}
            for start, end in speech
        ],
    }
    emit(payload, 0, args.output)


if __name__ == "__main__":
    main()
