# Analysis tools (the agent's senses)

Three standalone Python scripts the Concat API runs as jobs, giving an agent
editor its eyes and ears. They are embedded into the binary with
`include_str!` from this folder and materialized at first use under the app's
data directory — this folder is the source of truth.

| Script | Wraps | Result (JSON, seconds unless noted) |
| --- | --- | --- |
| `analyze_scenes.py` | PySceneDetect (AdaptiveDetector default) | `{scenes: [{start, end, length}], sceneCount, duration}` |
| `analyze_beats.py` | librosa | `{bpm, beatsMs, downbeatsMs, onsetsMs, duration}` (grids in ms) |
| `analyze_silence.py` | ffmpeg `silencedetect`, librosa RMS fallback | `{spans: [{start, end}], speech: [{start, end}], duration}` |

## The contract (the API relies on it)

- exit 0: the JSON result on stdout (or `--output file`);
- exit 2: bad input — `{"error": ...}`;
- exit 3: a missing dependency — `{"error": ..., "hint": "pip install ..."}` —
  surface the hint, do not retry;
- exit 4: analysis failure — `{"error": ..., "hint": ...}`.

One-time setup: `pip install -r requirements.txt`
(scenedetect + OpenCV headless, librosa, soundfile, numpy).
`analyze_silence` prefers an `ffmpeg` binary on PATH and falls back to
librosa when there is none.

## Tests

```bash
bash tests/make_test_media.sh   # tiny synthetic fixtures (ffmpeg only)
python3 tests/run_tests.py     # asserts real detection on them
```

The fixtures are generated files (not committed): `tests/test_media/`.
Last full run: scenes cut at 2.0 s detected, 117 BPM measured on a 120 BPM
click track, 1.0 s + 2.0 s silence gaps found exactly, error contract and the
librosa fallback verified.
