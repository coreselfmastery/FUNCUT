# Voice signals: reading the voiceover as a score

The emotion read ([docs/EMOTION.md](EMOTION.md)) says what every line of
the voiceover **feels like**. This is the other half: what the voiceover
**is** as sound — measured, per line, with the library that measured it
named in every reply. It is what turns the director from an auto-cutter
that knows *what was said* and *how it felt* into one that also knows
*how it was said, which word matters, whether it is a question, what is
named, whether there is noise, who is speaking, and where every beat
lands*.

The MCP tool is `voice_signals`; the director skill calls it in Phase 1c
and obeys its "A — the voice is a score" law. The Python half lives in
`src/crates/concat-mcp/sense/voice_signals.py` (pure stdlib + ffmpeg,
embedded in the binary, proxied by `sense.py`'s `voice` command); the Rust
half is the `voice-signals` crate (`cargo build -p voice-signals`), one
JSON spec in and one JSON reading out — the same contract every sense
script keeps.

```json
voice_signals {
  "audio": "/project/audio/voiceover.wav",
  "transcript": <the transcribe job's output>,
  "emotion": <an emotion extract's output, for the arc>,
  "language": "en"
}
```

## The signal inventory

**Per line** (index-aligned with the transcript's segments):

| Signal | What it is | Source | What the director does with it |
| --- | --- | --- | --- |
| `loudness.relativeLu` | Momentary LUFS mean vs the take's integrated level | ffmpeg `ebur128` M-series, folded per line | +3 LU is a punch line → fast cuts; −3 LU is a confession → hold |
| `speechRate.wordsPerMinute`, `syllablesPerSec`, `pace` | Rate with a class: punchy / conversational / editorial / slow | word count ÷ duration; vowel-group syllables (Python) | punchy lines cut fast, slow lines hold — the cut style |
| `sentence.type`, `riseConfirms` | question / exclamation / statement / fragment, cross-checked against the pitch's terminal rise | punctuation regex + spectral-prosody `finalRise` | a question's answer arrives as the next visual |
| `prosody.pitch*` | Pitch mean/min/max/range (semitones), slope (semitones/s), terminal rise | spectral-prosody autocorrelation | rising delivery = building; falling = settling |
| `prosody.emphasisWords` | The words the energy peaks land on, with strength | spectral-prosody energy envelope → word spans | SFX peaks and caption highlights land ON them |
| `fillers` | um/uh (safe-remove) and discourse markers (context-call), timed | regex (English core) | remove for punch, keep for authenticity |
| `words[].confidence` | Measured gate: timing sanity, silence overlap, quiet audio — or WhisperX scores | Python heuristic / whisperx | below 0.5 is re-checked or dropped before captions |
| `words[].syllables` | Vowel-group estimate per word | Python | karaoke caption pacing |
| `forensic.formantsHz`, `f0Hz`, `jitter`, `shimmer`, `hnrDb` | The line's voice fingerprint | oximedia LPC + voice analyzer | checks TTS regeneration consistency |
| `speaker` | Which speaker holds the line | polyvoice diarization | a change is a scene change |
| `entities` | MONEY/PER/ORG/DATE/PII spans mapped to word times, each with its artefact: stat / lower-third / timeline / redact | anno offline NER + regex complement | `$10,450` becomes a stat card, a name a lower-third |

**Around the lines:**

| Signal | What it is | Source |
| --- | --- | --- |
| `pauses[]` classified | breath / beat / dramatic hold / scene turn / dead air, each with `keep` | ffmpeg `silencedetect` + classification (Python) |
| `motifs[]` | Repeated 3–6 word phrases across lines | n-gram overlap (Python) |
| `wholeTake.rhythmLayers`, `dominantPeriodSec`, `phrases` | The take's macro/micro rhythm and phrase boundaries | spectral-prosody graph Laplacian |
| `wholeTake.forensic` | Noise floor, SNR, noise type, spectral flatness/centroid, flux onsets, crest, dynamic range, mean F0, voicing rate, RMS envelope | oximedia |
| `speaker.turns/changes` | Who spoke when, and the change times | polyvoice AHC over oximedia voice-prints |
| `arc` | The emotion read's arc, passed through | emotion.py |
| `language` | Detected script + caller's hint, `fontHint`, `musicGenreHint` | Unicode script scan (Python) |
| `directorHints` | cutStyle (fast/mixed/hold/building) + cutRule, captionGate, bgm decision + reason, speakerChanges, questions, emphasisWords, protectedPauses, font and genre | the wiring itself |

Roughly **36 distinct signals**, every one with its source named in
`sources` — and every one optional: an ear that cannot run says so in
`notes` instead of guessing.

## The five libraries

| Library | Licence | MSRV | What it gives |
| --- | --- | --- | --- |
| [spectral-prosody](https://crates.io/crates/spectral-prosody) 0.1.1 | MIT | — | Pitch contours, energy envelopes, rhythm layers, Fiedler phrases |
| [oximedia-audio-analysis](https://crates.io/crates/oximedia-audio-analysis) 0.2.1 | Apache-2.0 | 1.87 | Formants F1–F4, noise floor/SNR, spectral flux, dynamics, voice quality |
| [polyvoice](https://crates.io/crates/polyvoice) 0.17.0 | MIT | 1.88 | EnergyVad, agglomerative cosine clustering (speaker ear) |
| [anno](https://crates.io/crates/anno) 0.14.0 | MIT OR Apache-2.0 | 1.91 | Offline pattern NER (names, numbers, dates, money, PII) |
| [WhisperX](https://github.com/m-bain/whisperX) (optional, pip) | BSD-4 | — | Aligned word times with real confidence scores |

polyvoice is pinned to 0.17 — the newest line whose MSRV (1.88) sits
under the toolchain this repository builds with (1.93.1, read from
`src/rust-toolchain.toml`); the 0.18+ and 1.0 lines declare 1.94. anno
and polyvoice are built with default features off: no ONNX tree, no
model downloads, the pattern/energy backends answer offline. The
speaker diarization is honest about being a **voice-print heuristic**
(oximedia F0/formants/centroid/jitter windows, z-scored across the take
with a coefficient-of-variation floor so constant features cannot
outvote real ones, clustered by polyvoice's AHC) — not a neural
x-vector model; its `method` field says exactly that.

## Operating it

- **Build the Rust ears**: `cd src && cargo build -p voice-signals`. The
  binary is found on `PATH`, beside the unpacked sense scripts, or where
  `FUNCUT_VOICE_SIGNALS_BIN` points. Without it the Python signals still
  answer and `notes` says which ear was absent.
- **WhisperX** (optional, heavy: torch): `pip install whisperx`. When
  importable, word times and confidences come from the alignment;
  `whisperx: "off"` in the spec disables it.
- **Silence tuning**: `thresholdDb` (35) and `minSilenceMs` (350) take
  the same values `analyze_silence` does; 120 ms catches every breath,
  500 ms only real gaps.
- **Speakers**: `maxSpeakers` (4) caps the diarization;
  `speakerThreshold` (0.60 cosine) tunes how eagerly windows merge.

## Honesty rules

- The per-line loudness is the momentary series (400 ms windows sampled
  every 100 ms) folded per line — a 200 ms line reads from the windows
  that cover it, and the reply carries the sample count.
- Word confidence without WhisperX is a **gate, not a probability**: it
  only ever lowers confidence for a measured reason (a span over
  silence, an implausible duration, quiet audio under the word).
- Syllable counts are vowel-group estimates (labelled as such), exact
  for English, flat-estimated for unvowelled scripts.
- Entity time mapping is best-effort: an entity no word claims is
  flagged `approximate: true` and timed to the line's own span.
- `voice_signals {action: "selftest"}` synthesises a two-voice take with
  every pause class built in and asserts the whole chain — run it after
  any change to the ears.
