// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The `voice-signals` command line: one JSON spec in (stdin or `--spec`),
//! one JSON object out (stdout), the same contract the Python sense
//! scripts keep. `selftest` synthesises a take - two voices, a pause, a
//! line with money in it - runs the whole extraction over it and asserts
//! the reading, so a build with a broken ear says so instead of shipping.

use std::io::{Read, Write};
use std::path::PathBuf;

use voice_signals::{Error, LineSpec, Spec, WordSpec};

fn main() -> std::process::ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (cmd, spec_raw) = match parse_args(&args) {
        Ok(pair) => pair,
        Err(message) => {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"ok": false, "error": message})
            );
            return std::process::ExitCode::from(2);
        }
    };
    let result = match cmd.as_str() {
        "extract" => run_extract(&spec_raw),
        "selftest" => selftest(),
        _ => Err(Error::Spec(format!(
            "unknown command `{cmd}`: extract or selftest"
        ))),
    };
    match result {
        Ok(output) => {
            let text =
                serde_json::to_string_pretty(&output).unwrap_or_else(|_| "{\"ok\":false}".into());
            let _ = writeln!(std::io::stdout(), "{text}");
            if output_ok(&output) {
                std::process::ExitCode::SUCCESS
            } else {
                std::process::ExitCode::from(4)
            }
        }
        Err(error) => {
            let _ = writeln!(
                std::io::stdout(),
                "{}",
                serde_json::json!({"ok": false, "error": error.to_string()})
            );
            std::process::ExitCode::from(4)
        }
    }
}

/// Whether an output object carries `ok: true`.
fn output_ok(output: &serde_json::Value) -> bool {
    output.get("ok") == Some(&serde_json::Value::Bool(true))
}

/// Parse the command and the spec out of argv.
fn parse_args(args: &[String]) -> Result<(String, String), String> {
    let mut cmd: Option<&str> = None;
    let mut spec: Option<String> = None;
    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--spec" => {
                i += 1;
                spec = Some(args.get(i).cloned().ok_or("--spec needs a value (or -)")?);
            }
            "--help" | "-h" => {
                return Err("usage: voice-signals <extract|selftest> --spec -".into());
            }
            other => {
                if cmd.is_none() {
                    cmd = Some(other);
                } else {
                    return Err(format!("unexpected argument `{other}`"));
                }
            }
        }
        i += 1;
    }
    let cmd = cmd
        .ok_or("usage: voice-signals <extract|selftest> --spec -")?
        .to_owned();
    let spec = match spec {
        Some(raw) if raw == "-" => {
            let mut buffer = String::new();
            std::io::stdin()
                .read_to_string(&mut buffer)
                .map_err(|e| format!("could not read the spec from stdin: {e}"))?;
            buffer
        }
        Some(raw) => raw,
        None => "{}".into(),
    };
    Ok((cmd, spec))
}

/// Run one extraction and re-wrap it as the CLI's JSON.
fn run_extract(spec_raw: &str) -> Result<serde_json::Value, Error> {
    let spec: Spec = serde_json::from_str(spec_raw).map_err(|e| {
        Error::Spec(format!(
            "the spec is not valid JSON for this extractor: {e}"
        ))
    })?;
    let output = voice_signals::extract(&spec)?;
    serde_json::to_value(&output)
        .map_err(|e| Error::Analysis(format!("serialising the output: {e}")))
}

/// Synthesise a take and assert the whole extraction over it.
fn selftest() -> Result<serde_json::Value, Error> {
    let dir = std::env::temp_dir().join(format!("voice-signals-selftest-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| Error::Scratch(e.to_string()))?;
    let wav = dir.join("take.wav");
    write_selftest_wav(&wav)?;

    let spec = Spec {
        wav: Some(wav.to_string_lossy().into_owned()),
        lines: vec![
            LineSpec {
                start: Some(0.10),
                end: Some(3.90),
                text: "Lynn Conway worked at IBM and earned $10,450.".into(),
                words: vec![
                    WordSpec {
                        start: 0.10,
                        end: 0.40,
                        text: "Lynn".into(),
                    },
                    WordSpec {
                        start: 0.40,
                        end: 0.80,
                        text: "Conway".into(),
                    },
                    WordSpec {
                        start: 0.80,
                        end: 1.20,
                        text: "worked".into(),
                    },
                    WordSpec {
                        start: 1.20,
                        end: 1.40,
                        text: "at".into(),
                    },
                    WordSpec {
                        start: 1.40,
                        end: 1.90,
                        text: "IBM".into(),
                    },
                    WordSpec {
                        start: 1.90,
                        end: 2.30,
                        text: "and".into(),
                    },
                    WordSpec {
                        start: 2.30,
                        end: 3.00,
                        text: "earned".into(),
                    },
                    WordSpec {
                        start: 3.00,
                        end: 3.90,
                        text: "$10,450.".into(),
                    },
                ],
            },
            LineSpec {
                start: Some(5.10),
                end: Some(8.90),
                text: "The project shipped in 2026 to great acclaim.".into(),
                words: vec![
                    WordSpec {
                        start: 5.10,
                        end: 5.40,
                        text: "The".into(),
                    },
                    WordSpec {
                        start: 5.40,
                        end: 6.10,
                        text: "project".into(),
                    },
                    WordSpec {
                        start: 6.10,
                        end: 6.70,
                        text: "shipped".into(),
                    },
                    WordSpec {
                        start: 6.70,
                        end: 6.90,
                        text: "in".into(),
                    },
                    WordSpec {
                        start: 6.90,
                        end: 7.60,
                        text: "2026".into(),
                    },
                    WordSpec {
                        start: 7.60,
                        end: 8.90,
                        text: "to great acclaim.".into(),
                    },
                ],
            },
        ],
        ..Spec::default()
    };
    let output = voice_signals::extract(&spec)?;
    let mut failures: Vec<String> = Vec::new();

    // The audio facts.
    let Some(audio) = &output.audio else {
        failures.push("no audio facts".into());
        return Ok(selftest_report(failures));
    };
    if (audio.duration_sec - 9.0).abs() > 0.5 {
        failures.push(format!("duration {} is not ~9 s", audio.duration_sec));
    }

    // The prosody ear: line 0 (110 Hz) must read near its own pitch.
    let first = output.prosody.lines.first().and_then(|line| line.as_ref());
    match first.and_then(|line| line.pitch_mean_hz) {
        Some(hz) if (85.0..=135.0).contains(&hz) => {}
        other => failures.push(format!("line 0 pitch {other:?} is not the 110 Hz tone")),
    }
    // The second line (240 Hz) reads higher than the first.
    let second = output.prosody.lines.get(1).and_then(|line| line.as_ref());
    match (
        first.and_then(|l| l.pitch_mean_hz),
        second.and_then(|l| l.pitch_mean_hz),
    ) {
        (Some(low), Some(high)) if high > low + 60.0 => {}
        pair => failures.push(format!("the two tones did not read apart: {pair:?}")),
    }

    // The forensic ear: the noise floor of a synthetic take is far down.
    if let Some(whole) = &output.forensic.whole {
        if whole.noise_floor_dbfs > -30.0 {
            failures.push(format!(
                "noise floor {} is too high",
                whole.noise_floor_dbfs
            ));
        }
    } else {
        failures.push("no whole-take forensics".into());
    }

    // The speaker ear: two tones must be two speakers.
    if !output.speaker.available {
        failures.push(format!(
            "diarization unavailable: {:?}",
            output.speaker.reason
        ));
    }
    if output.speaker.speakers.len() < 2 {
        failures.push(format!(
            "the two voices read as {} speaker(s)",
            output.speaker.speakers.len()
        ));
    }
    if output.speaker.changes.is_empty() {
        failures.push("no speaker change between the two voices".into());
    }

    // The text ear: the money and the person, timed.
    let money = output.entities.iter().find(|e| e.entity_type == "MONEY");
    match money {
        Some(entity) => {
            if entity.card != "stat" {
                failures.push(format!("money card is {}", entity.card));
            }
            if entity.start.is_none() {
                failures.push("the money entity has no time".into());
            }
        }
        None => failures.push("no MONEY entity in the selftest line".into()),
    }
    if !output.entities.iter().any(|e| e.entity_type == "PER") {
        failures.push("no PER entity in the selftest line".into());
    }

    let _ = std::fs::remove_dir_all(&dir);
    Ok(selftest_report(failures))
}

/// The selftest's report object.
fn selftest_report(failures: Vec<String>) -> serde_json::Value {
    if failures.is_empty() {
        serde_json::json!({"ok": true, "selftest": "all ears answer"})
    } else {
        serde_json::json!({"ok": false, "selftest": "failures", "failures": failures})
    }
}

/// Write the selftest take: 4 s at 110 Hz, 1 s of silence, 4 s at 240 Hz,
/// with a slow amplitude pulse so the energy contour has peaks.
fn write_selftest_wav(path: &PathBuf) -> Result<(), Error> {
    let rate = 16_000_u32;
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec)
        .map_err(|e| Error::Scratch(format!("{}: {e}", path.display())))?;
    let mut push = |freq: f64, secs: f64| -> Result<(), Error> {
        let n = (secs * f64::from(rate)) as usize;
        for i in 0..n {
            let t = i as f64 / f64::from(rate);
            // A 2 Hz amplitude pulse: peaks every 0.5 s for the energy
            // contour to find, never dipping to silence.
            let pulse = 0.35 + 0.35 * (std::f64::consts::TAU * 2.0 * t).sin();
            let sample = (freq * std::f64::consts::TAU * t).sin() * pulse;
            writer
                .write_sample((sample * 32000.0) as i16)
                .map_err(|e| Error::Scratch(format!("write: {e}")))?;
        }
        Ok(())
    };
    push(110.0, 4.0)?;
    push(0.0, 1.0)?;
    push(240.0, 4.0)?;
    writer
        .finalize()
        .map_err(|e| Error::Scratch(format!("finalise: {e}")))?;
    Ok(())
}
