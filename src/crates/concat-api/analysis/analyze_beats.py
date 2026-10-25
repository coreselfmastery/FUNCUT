#!/usr/bin/env python3
"""Find the tempo, the beat grid and the onsets of a piece of music (librosa).

Called by the Concat API's `analysis.beats` job, and runnable standalone:

    python3 analyze_beats.py --input bgm.mp3

Writes JSON to stdout (or --output). Times are milliseconds (beat grids are
used as ms integers by the edit plan); `duration` is seconds.
Exit codes: 0 ok · 2 bad input · 3 missing Python dependency · 4 analysis failed.
On any failure a JSON object {"error": ..., "hint": ...} is still written to
stdout, so a machine caller can always parse one format.
"""

import argparse
import json
import os
import sys

# A beat grid longer than this is trimmed from BOTH ends; a 10-minute track's
# grid is useless to an agent in full — the first and last beats matter.
MAX_GRID = 2000


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
    parser = argparse.ArgumentParser(description="BPM, beats and onsets (librosa).")
    parser.add_argument("--input", required=True, help="the audio file")
    parser.add_argument("--offset", type=float, default=0.0,
                        help="seconds into the file to start (default 0)")
    parser.add_argument("--output", default=None, help="write JSON here instead of stdout")
    args = parser.parse_args()

    if not os.path.isfile(args.input):
        emit({"error": f"no such file: {args.input}"}, 2)

    try:
        import numpy
        import librosa
    except ImportError:
        fail("missing Python dependency: librosa", 3, "pip install librosa soundfile")

    try:
        y, sr = librosa.load(args.input, sr=22050, mono=True, offset=args.offset)
    except FileNotFoundError:
        fail(f"no such file: {args.input}", 2)
    except Exception as error:
        fail(f"could not decode {args.input}: {type(error).__name__}: {error}", 4,
             "the file must carry a decodable audio stream (librosa/soundfile/ffmpeg)")

    if y.size == 0:
        fail(f"no audio in {args.input}", 2)

    try:
        tempo, beats = librosa.beat.beat_track(y=y, sr=sr)
        # librosa >= 0.10 returns a scalar tempo; older versions an ndarray.
        bpm = float(numpy.atleast_1d(tempo)[0])
        beat_times = librosa.frames_to_time(beats, sr=sr)
        onset_env = librosa.onset.onset_strength(y=y, sr=sr)
        onsets = librosa.onset.onset_detect(onset_envelope=onset_env, sr=sr)
        onset_times = librosa.frames_to_time(onsets, sr=sr)
    except Exception as error:
        fail(f"could not analyse {args.input}: {type(error).__name__}: {error}", 4)

    def ms(times, limit=MAX_GRID):
        values = [int(round(float(t) * 1000.0)) for t in times]
        truncated = len(values) > limit
        if truncated:
            half = limit // 2
            values = values[:half] + values[-half:]
        return values, truncated

    beats_ms, beats_cut = ms(beat_times)
    onsets_ms, onsets_cut = ms(onset_times, MAX_GRID * 2)
    # Downbeats: every 4th beat from the first — the approximation a beat grid
    # without meter analysis supports, and all a section-alignment needs.
    downbeats_ms = beats_ms[::4]

    payload = {
        "input": args.input,
        "offset": args.offset,
        "duration": round(float(y.size) / float(sr), 3),
        "bpm": round(bpm, 2),
        "beatCount": len(beats_ms),
        "beatsMs": beats_ms,
        "downbeatsMs": downbeats_ms,
        "onsetsMs": onsets_ms,
    }
    if beats_cut or onsets_cut:
        payload["truncated"] = True
        payload["truncationHint"] = "grid kept from both ends; analyse with --offset for the middle"
    emit(payload, 0, args.output)


if __name__ == "__main__":
    main()
