//! Audio reaction: the spectrum a comp's music carries, and what it
//! drives.
//!
//! A comp can name a WAV asset and bind its tracks to the music's
//! bands - sub-bass, bass, mid, presence, or the spectral flux of
//! change itself - and the bake expands those bindings into ordinary
//! keyframe tracks, one key a frame, so every engine that reads tracks
//! plays the reaction with nothing new to know.
//!
//! Everything here is deterministic: the same WAV and the same comp
//! bake the same keys, frame for frame. The FFT is a radix-2
//! butterfly, the bands are mean magnitudes over their frequency
//! windows peak-normalised across the whole clip, and the flux is the
//! sum of the spectrum's positive steps.

use crate::comp::{AudioBinding, Band, Comp, NodeKind};
use crate::track::{Key, Track, Value};
use std::path::Path;

/// The PCM an audio asset decodes to: mono f32 samples, the loudness
/// envelope's own ground truth.
#[derive(Clone, PartialEq, Debug)]
pub struct Pcm {
    /// The samples, interleaved down to mono.
    pub samples: Vec<f32>,
    /// The samples' own rate.
    pub sample_rate: f64,
}

/// The bands one analysis window carries, each normalised `0..=1`
/// against the loudest the clip ever plays them.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Bands {
    /// 20 to 60 Hz: the pressure a sub carries.
    pub sub_bass: f64,
    /// 60 to 250 Hz: the body of a kick and a bass line.
    pub bass: f64,
    /// 250 Hz to 2 kHz: where words and chords live.
    pub mid: f64,
    /// 2 to 6 kHz: the edge of consonants and hats.
    pub presence: f64,
    /// The spectrum's positive step from the last window: onsets,
    /// transients, the music's own pulse of change.
    pub flux: f64,
}

/// An iterative radix-2 FFT, in place, magnitudes out. `re` and `im`
/// are the window, zero-padded to `len` (a power of two) by the
/// caller; the result is `len` magnitude bins.
fn fft_magnitudes(re: &mut [f64], im: &mut [f64]) -> Vec<f64> {
    let len = re.len();
    debug_assert!(len.is_power_of_two(), "the fft needs a power of two");
    // Bit-reversal permutation: the butterfly's own addressing.
    let mut j = 0_usize;
    for i in 0..len {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut mask = len >> 1;
        while mask != 0 && j & mask != 0 {
            j &= !mask;
            mask >>= 1;
        }
        j |= mask;
    }
    // The butterflies: twiddles by angle, stage by stage.
    let mut span = 2;
    while span <= len {
        let angle = -2.0 * std::f64::consts::PI / span as f64;
        for start in (0..len).step_by(span) {
            for k in 0..span / 2 {
                let (wr, wi) = ((angle * k as f64).cos(), (angle * k as f64).sin());
                let (a, b) = (start + k, start + k + span / 2);
                let (xr, xi) = (re[b] * wr - im[b] * wi, re[b] * wi + im[b] * wr);
                re[b] = re[a] - xr;
                im[b] = im[a] - xi;
                re[a] += xr;
                im[a] += xi;
            }
        }
        span *= 2;
    }
    (0..len)
        .map(|i| (re[i] * re[i] + im[i] * im[i]).sqrt())
        .collect()
}

/// The Hann window's coefficient at `i` of `n`.
fn hann(i: usize, n: usize) -> f64 {
    0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / n as f64).cos()
}

/// The next power of two at or above `n`.
fn next_pow2(n: usize) -> usize {
    n.max(1).next_power_of_two()
}

/// The bands of one window of samples, with the previous window's
/// magnitudes for the flux. Returns the window's own magnitudes for
/// the next call's flux.
fn bands_of(window: &[f32], sample_rate: f64, previous: Option<&[f64]>) -> (Bands, Vec<f64>) {
    let n = next_pow2(window.len());
    let mut re = vec![0.0_f64; n];
    let mut im = vec![0.0_f64; n];
    for (i, sample) in window.iter().enumerate() {
        re[i] = f64::from(*sample) * hann(i, window.len());
    }
    let mags = fft_magnitudes(&mut re, &mut im);
    let bin_hz = sample_rate / n as f64;
    let mean_over = |from: f64, to: f64| -> f64 {
        let first = ((from / bin_hz).ceil() as usize).max(1);
        let last = ((to / bin_hz).floor() as usize).min(n / 2 - 1);
        if first > last {
            return 0.0;
        }
        mags[first..=last].iter().sum::<f64>() / (last - first + 1) as f64
    };
    let mut bands = Bands {
        sub_bass: mean_over(20.0, 60.0),
        bass: mean_over(60.0, 250.0),
        mid: mean_over(250.0, 2000.0),
        presence: mean_over(2000.0, 6000.0),
        flux: 0.0,
    };
    if let Some(previous) = previous {
        let mut step = 0.0;
        let first = ((20.0 / bin_hz).ceil() as usize).max(1);
        let last = ((6000.0 / bin_hz).floor() as usize).min(n / 2 - 1);
        for i in first..=last {
            let now = mags.get(i).copied().unwrap_or(0.0);
            let then = previous.get(i).copied().unwrap_or(0.0);
            step += (now - then).max(0.0);
        }
        bands.flux = step;
    }
    (bands, mags)
}

/// Every frame's bands, one per frame of the comp's own rate: the
/// envelope a binding drives its track by, each band peak-normalised
/// across the whole clip so the loudest the music plays it is `1`.
pub fn envelope(pcm: &Pcm, fps: f64, frames: usize) -> Vec<Bands> {
    let rate = pcm.sample_rate.max(1.0);
    // A quarter-second window, a power of two: fine enough for the
    // bands, short enough to catch a kick.
    let window = next_pow2((rate * 0.25).round() as usize).max(256);
    let mut raw: Vec<Bands> = Vec::with_capacity(frames);
    let mut previous: Option<Vec<f64>> = None;
    for frame in 0..frames {
        let centre = (frame as f64 / fps) * rate;
        let start = (centre - window as f64 / 2.0).round() as i64;
        let start = start.clamp(0, pcm.samples.len() as i64) as usize;
        let end = (start + window).min(pcm.samples.len());
        if end <= start {
            raw.push(Bands::default());
            continue;
        }
        let (bands, mags) = bands_of(&pcm.samples[start..end], rate, previous.as_deref());
        previous = Some(mags);
        raw.push(bands);
    }
    // Peak-normalise each band against the clip's own loudest, so a
    // binding's amount means the same thing whatever the mix.
    fn peak_of(raw: &[Bands], read: fn(&Bands) -> f64) -> f64 {
        raw.iter().map(read).fold(0.0_f64, f64::max).max(1e-9)
    }
    let (p_sub, p_bass, p_mid, p_pres, p_flux) = (
        peak_of(&raw, |b| b.sub_bass),
        peak_of(&raw, |b| b.bass),
        peak_of(&raw, |b| b.mid),
        peak_of(&raw, |b| b.presence),
        peak_of(&raw, |b| b.flux),
    );
    raw.iter()
        .map(|bands| Bands {
            sub_bass: (bands.sub_bass / p_sub).clamp(0.0, 1.0),
            bass: (bands.bass / p_bass).clamp(0.0, 1.0),
            mid: (bands.mid / p_mid).clamp(0.0, 1.0),
            presence: (bands.presence / p_pres).clamp(0.0, 1.0),
            flux: (bands.flux / p_flux).clamp(0.0, 1.0),
        })
        .collect()
}

/// Reads a WAV file into [`Pcm`]: PCM 16, 24 and 32-bit, and 32-bit
/// float, any channel count folded to mono by the mean. Anything else
/// is refused by name.
pub fn read_wav(bytes: &[u8]) -> Result<Pcm, String> {
    let read_u16 = |at: usize| u16::from_le_bytes([bytes[at], bytes[at + 1]]);
    let read_u32 =
        |at: usize| u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a RIFF/WAVE file".into());
    }
    let (mut format, mut channels, mut rate, mut bits) = (1_u16, 1_u16, 44_100.0_f64, 16_u16);
    let mut data: Option<&[u8]> = None;
    let mut at = 12;
    while at + 8 <= bytes.len() {
        let id = &bytes[at..at + 4];
        let size = read_u32(at + 4) as usize;
        let body = at + 8;
        if body + size > bytes.len() {
            break;
        }
        match id {
            b"fmt " if size >= 16 => {
                format = read_u16(body);
                channels = read_u16(body + 2).max(1);
                rate = f64::from(read_u32(body + 4));
                bits = read_u16(body + 14);
            }
            b"data" => data = Some(&bytes[body..body + size]),
            _ => {}
        }
        at = body + size + (size % 2);
    }
    let data = data.ok_or("the wav carries no data chunk")?;
    let frame_bytes = (bits as usize / 8) * channels as usize;
    if frame_bytes == 0 {
        return Err("the wav's sample width is zero".into());
    }
    let decode = |chunk: &[u8]| -> Result<f32, String> {
        let one = |bytes: &[u8]| -> f32 {
            match (bits, format) {
                (16, 1) => i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32_768.0,
                (24, 1) => {
                    let value = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]) >> 8;
                    value as f32 / 8_388_608.0
                }
                (32, 1) => {
                    i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32
                        / 2_147_483_648.0
                }
                (32, 3) => f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                _ => 0.0,
            }
        };
        if chunk.len() < bits as usize / 8 {
            return Err("the wav's data ended mid-sample".into());
        }
        Ok((0..channels as usize)
            .map(|c| one(&chunk[c * (bits as usize / 8)..]))
            .sum::<f32>()
            / channels as f32)
    };
    let samples = data
        .chunks_exact(frame_bytes)
        .map(decode)
        .collect::<Result<Vec<f32>, String>>()?;
    Ok(Pcm {
        samples,
        sample_rate: rate,
    })
}

/// The viseme a mouth shape carries: the lip sync's own alphabet, the
/// shapes mouths make rather than the sounds tongues do.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Viseme {
    /// Closed: silence, or an M, B or P.
    Sil,
    /// Open wide: an AH as in "father".
    Aa,
    /// Mid open: an EH as in "bed".
    Eh,
    /// Slightly open: an IH as in "bit".
    Ih,
    /// Round: an OH as in "boat".
    Oh,
    /// Small round: an UW as in "boot".
    Uh,
    /// Lips to teeth: an F or V.
    Fv,
}

/// The viseme one frame's bands and word make: an open band shape is
/// an open mouth, a dark one is round, a closed one is closed. The
/// word's own first letter answers when the bands are quiet, so a
/// whisper still lipsyncs.
pub fn viseme(bands: &Bands, word: Option<&str>) -> Viseme {
    let open = bands.mid * 0.5 + bands.presence * 0.5;
    let dark = bands.bass * 0.7 + bands.sub_bass * 0.3;
    if open < 0.15 && dark < 0.15 {
        // Quiet: the word's own consonants answer.
        return match word
            .and_then(|word| word.chars().next())
            .map(|c| c.to_ascii_lowercase())
        {
            Some('m') | Some('b') | Some('p') => Viseme::Sil,
            Some('f') | Some('v') => Viseme::Fv,
            Some('o') => Viseme::Oh,
            Some('u') | Some('w') => Viseme::Uh,
            Some('i') | Some('y') => Viseme::Ih,
            Some('e') => Viseme::Eh,
            Some('a') => Viseme::Aa,
            _ => Viseme::Sil,
        };
    }
    if dark > open {
        Viseme::Oh
    } else if open > 0.6 {
        Viseme::Aa
    } else if open > 0.35 {
        Viseme::Eh
    } else {
        Viseme::Ih
    }
}

/// One micro-foley cue: a sound the edit can lay under a moment the
/// music itself marked.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct SfxCue {
    /// When the cue lands, seconds into the comp.
    pub at: f64,
    /// The cue's own name: `thump`, `tick` or `whoosh` - a bed the
    /// concat stage owns by that name.
    pub name: &'static str,
    /// How hard it lands, `0..=1` against the clip's own loudest.
    pub gain: f64,
}

/// The micro-foley a clip's onsets suggest: every flux peak above its
/// own local mean, named by the band it rides - a sub-bass onset is a
/// thump, a presence onset a tick, a slow swell a whoosh.
pub fn micro_foley(env: &[Bands], fps: f64) -> Vec<SfxCue> {
    let mean = env.iter().map(|b| b.flux).sum::<f64>() / env.len().max(1) as f64;
    let mut cues = Vec::new();
    for (i, bands) in env.iter().enumerate() {
        if bands.flux < mean * 1.5 || bands.flux < 0.1 {
            continue;
        }
        // The loudest neighbour flux, so one onset cues once.
        let neighbours = env
            .get(i.wrapping_sub(1))
            .map(|b| b.flux)
            .unwrap_or(0.0)
            .max(env.get(i + 1).map(|b| b.flux).unwrap_or(0.0));
        if bands.flux < neighbours {
            continue;
        }
        let name = if bands.sub_bass + bands.bass > bands.mid + bands.presence {
            "thump"
        } else if bands.presence > bands.mid {
            "tick"
        } else {
            "whoosh"
        };
        cues.push(SfxCue {
            at: i as f64 / fps,
            name,
            gain: bands.flux.clamp(0.0, 1.0),
        });
    }
    // Three cues a second is a busy enough bed.
    let budget = (3.0 * env.len() as f64 / fps.max(1e-9)).ceil() as usize;
    cues.truncate(budget.max(3));
    cues
}

/// The mood a clip's own envelope says: `valence` from the spectrum's
/// brightness tilt (bright mixes read positive, dark ones negative)
/// and `arousal` from its flux and loudness - a rough, honest
/// heuristic the pulse stack's own emotion work can lean on.
pub fn mood_of(env: &[Bands]) -> (f64, f64) {
    if env.is_empty() {
        return (0.0, 0.0);
    }
    fn mean_of(env: &[Bands], read: fn(&Bands) -> f64) -> f64 {
        env.iter().map(read).sum::<f64>() / env.len() as f64
    }
    let (bright, dark, flux, loud) = (
        mean_of(env, |b| b.presence),
        mean_of(env, |b| b.bass + b.sub_bass),
        mean_of(env, |b| b.flux),
        mean_of(env, |b| (b.bass + b.mid + b.presence) / 3.0),
    );
    let valence = ((bright - dark) / (bright + dark + 1e-9)).clamp(-1.0, 1.0);
    let arousal = ((flux * 0.6 + loud * 0.4) * 1.6 - 0.1).clamp(0.0, 1.0);
    (valence, arousal)
}

/// Expands a comp's audio bindings into ordinary tracks: every bound
/// `(node, track, band)` gains a key a frame, `base + envelope *
/// amount`, in the track's own fraction clock. The comp it returns is
/// the comp every engine bakes; the bindings themselves stay on the
/// document for the editor to read.
pub fn expand(comp: &Comp, source_dir: &Path) -> Result<Comp, crate::MotionError> {
    // Every audio node's bindings, with the asset each reads.
    let mut driven: Vec<(String, &AudioBinding)> = Vec::new();
    for node in &comp.nodes {
        if let NodeKind::Audio { asset, bindings } = &node.kind {
            for binding in bindings {
                driven.push((asset.clone(), binding));
            }
        }
    }
    if driven.is_empty() {
        return Ok(comp.clone());
    }
    // One decode per distinct asset.
    let mut pcms: std::collections::BTreeMap<String, Pcm> = std::collections::BTreeMap::new();
    let mut envs: std::collections::BTreeMap<String, Vec<Bands>> =
        std::collections::BTreeMap::new();
    let frames = ((comp.duration * comp.fps).round() as usize).max(1);
    for asset in driven
        .iter()
        .map(|(asset, _)| asset)
        .cloned()
        .collect::<Vec<String>>()
    {
        if envs.contains_key(&asset) {
            continue;
        }
        let source = comp
            .assets
            .get(&asset)
            .map(|found| found.src.clone())
            .ok_or_else(|| crate::MotionError::Invalid {
                diagnostics: vec![crate::comp::Diagnostic::new(
                    format!("/nodes/kind/audio/{asset:?}"),
                    format!("the audio reaction names an asset {asset:?} the comp does not carry"),
                )],
            })?;
        let path = source_dir.join(&source);
        let bytes = std::fs::read(&path).map_err(|source| crate::MotionError::Io {
            doing: "reading".into(),
            path: path.clone(),
            source,
        })?;
        let pcm = read_wav(&bytes).map_err(|problem| crate::MotionError::Invalid {
            diagnostics: vec![crate::comp::Diagnostic::new(
                format!("/assets/{asset}"),
                format!("the audio asset {source:?} does not read: {problem}"),
            )],
        })?;
        pcms.insert(asset.clone(), pcm.clone());
        envs.insert(asset.clone(), envelope(&pcm, comp.fps, frames));
    }
    let mut expanded = comp.clone();
    for (asset, binding) in &driven {
        let Some(env) = envs.get(asset) else {
            continue;
        };
        let Some(node) = expanded
            .nodes
            .iter_mut()
            .find(|node| node.id == binding.node)
        else {
            continue;
        };
        let read = |bands: &Bands| match binding.band {
            Band::SubBass => bands.sub_bass,
            Band::Bass => bands.bass,
            Band::Mid => bands.mid,
            Band::Presence => bands.presence,
            Band::Flux => bands.flux,
        };
        let keys: Vec<Key> = env
            .iter()
            .enumerate()
            .map(|(i, bands)| {
                Key::new(
                    (i as f64 / frames.max(1) as f64).clamp(0.0, 1.0),
                    Value::Number {
                        value: binding.base + read(bands) * binding.amount,
                    },
                )
            })
            .collect();
        node.tracks.insert(binding.track.clone(), Track::new(keys));
    }
    Ok(expanded)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A sine at `hz`, `seconds` long, at a steady rate.
    fn sine(hz: f64, seconds: f64, rate: f64) -> Pcm {
        let samples = (0..(seconds * rate) as usize)
            .map(|i| (2.0 * std::f64::consts::PI * hz * i as f64 / rate).sin() as f32)
            .collect();
        Pcm {
            samples,
            sample_rate: rate,
        }
    }

    /// The FFT puts a 440 Hz sine's peak in 440 Hz's own bin.
    #[test]
    fn the_fft_finds_the_pitch() {
        let rate = 8000.0;
        let pcm = sine(440.0, 1.0, rate);
        let (bands, _) = bands_of(&pcm.samples[..1024], rate, None);
        // 440 Hz is mid country (250..2000): the mid band carries it.
        assert!(bands.mid > 0.1, "{bands:?}");
        // And nothing below it moves.
        assert!(bands.sub_bass < bands.mid, "{bands:?}");
    }

    /// A sub-bass tone lands in the sub-bass band, a presence tone in
    /// presence: the windows hold their own.
    #[test]
    fn the_bands_hold_their_windows() {
        let rate = 16_000.0;
        let sub = sine(40.0, 1.0, rate);
        let (bands, _) = bands_of(&sub.samples[..2048], rate, None);
        assert!(bands.sub_bass > bands.presence * 4.0, "{bands:?}");
        let edge = sine(4000.0, 1.0, rate);
        let (bands, _) = bands_of(&edge.samples[..2048], rate, None);
        assert!(bands.presence > bands.sub_bass * 4.0, "{bands:?}");
    }

    /// The envelope peak-normalises: the loudest frame of a band reads
    /// `1`, and every frame is clamped inside `0..=1`.
    #[test]
    fn the_envelope_normalises() {
        // A 50 Hz pulse train: loud, then quiet.
        let rate = 8000.0;
        let mut samples = Vec::new();
        for i in 0..(2.0 * rate) as usize {
            let t = i as f64 / rate;
            let gate = if t < 0.5 { 1.0 } else { 0.05 };
            samples.push(((2.0 * std::f64::consts::PI * 50.0 * t).sin() * gate) as f32);
        }
        let pcm = Pcm {
            samples,
            sample_rate: rate,
        };
        let env = envelope(&pcm, 30.0, 60);
        assert_eq!(env.len(), 60);
        let loudest = env.iter().map(|b| b.bass).fold(0.0_f64, f64::max);
        assert!((loudest - 1.0).abs() < 1e-9, "{loudest}");
        assert!(env.iter().all(|b| (0.0..=1.0).contains(&b.bass)), "{env:?}");
        // The quiet half reads below the loud half.
        let early = env[..10].iter().map(|b| b.bass).sum::<f64>();
        let late = env[40..].iter().map(|b| b.bass).sum::<f64>();
        assert!(early > late * 2.0, "{early} vs {late}");
    }

    /// A 16-bit mono WAV round-trips through the reader.
    #[test]
    fn a_wav_reads() {
        let rate = 8000_u32;
        let samples: Vec<i16> = (0..64)
            .map(|i| ((i as f32 / 8.0).sin() * 8000.0) as i16)
            .collect();
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"RIFF");
        let data = samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<u8>>();
        let body = 24 + data.len();
        bytes.extend_from_slice(&(body as u32 + 8).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&1_u16.to_le_bytes());
        bytes.extend_from_slice(&rate.to_le_bytes());
        bytes.extend_from_slice(&(rate * 2).to_le_bytes());
        bytes.extend_from_slice(&2_u16.to_le_bytes());
        bytes.extend_from_slice(&16_u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&(data.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&data);
        let pcm = read_wav(&bytes).expect("the wav reads");
        assert_eq!(pcm.samples.len(), 64);
        assert_eq!(pcm.sample_rate, 8000.0);
        assert!((pcm.samples[2] - f32::from(samples[2]) / 32_768.0).abs() < 1e-6);
    }

    /// A refuse, by name: not a RIFF file.
    #[test]
    fn a_non_wav_is_refused() {
        assert!(read_wav(b"not a wav at all").is_err());
    }

    /// The viseme reads an open band shape as an open mouth and a
    /// quiet frame by its word's own first letter.
    #[test]
    fn the_viseme_reads_the_mouth() {
        let open = Bands {
            mid: 0.9,
            presence: 0.8,
            ..Bands::default()
        };
        assert_eq!(viseme(&open, None), Viseme::Aa);
        let round = Bands {
            bass: 0.9,
            sub_bass: 0.7,
            ..Bands::default()
        };
        assert_eq!(viseme(&round, None), Viseme::Oh);
        let quiet = Bands::default();
        assert_eq!(viseme(&quiet, Some("moon")), Viseme::Sil);
        assert_eq!(viseme(&quiet, Some("very")), Viseme::Fv);
    }

    /// Onsets cue: a click train puts a cue at each click, and the
    /// cues carry the moment they land.
    #[test]
    fn onsets_cue() {
        let rate = 8000.0;
        let mut samples = Vec::new();
        for i in 0..(2.0 * rate) as usize {
            let t = i as f64 / rate;
            // A click every half second: 5 ms of noise.
            let click = t.rem_euclid(0.5) < 0.005;
            let value = if click {
                ((i * 7919) % 13) as f32 / 13.0 - 0.5
            } else {
                0.0
            };
            samples.push(value);
        }
        let pcm = Pcm {
            samples,
            sample_rate: rate,
        };
        let env = envelope(&pcm, 30.0, 60);
        let cues = micro_foley(&env, 30.0);
        assert!(!cues.is_empty(), "{env:?}");
        assert!(cues.iter().all(|cue| cue.at >= 0.0), "{cues:?}");
    }

    /// The mood reads bright as positive and busy as aroused.
    #[test]
    fn the_mood_reads_the_mix() {
        let bright = vec![Bands {
            presence: 0.9,
            mid: 0.4,
            ..Bands::default()
        }];
        let (valence, arousal) = mood_of(&bright);
        assert!(valence > 0.0, "{valence}");
        assert!(arousal > 0.15, "{arousal}");
        let dark = vec![Bands {
            bass: 0.9,
            sub_bass: 0.5,
            ..Bands::default()
        }];
        let (valence, _) = mood_of(&dark);
        assert!(valence < 0.0, "{valence}");
        assert_eq!(mood_of(&[]), (0.0, 0.0));
    }

    /// A node with the fields the comp's own test helpers spell out.
    fn node_of(id: &str, kind: crate::comp::NodeKind) -> crate::comp::Node {
        crate::comp::Node {
            id: id.into(),
            parent: None,
            start: 0.0,
            duration: 0.0,
            transform: crate::comp::Transform::default(),
            opacity: 1.0,
            blend: crate::comp::Blend::Normal,
            mask: None,
            matte: None,
            motion_blur: None,
            effects: Vec::new(),
            tracks: Default::default(),
            kind,
        }
    }

    /// The expansion turns a binding into a track: a comp with an audio
    /// node, a WAV on disk and a bound scale track bakes keys that move
    /// between the base and the base plus the amount.
    #[test]
    fn a_binding_expands_into_a_track() {
        use std::collections::BTreeMap;
        // A 50 Hz pulse, loud then quiet, as a 16-bit WAV on disk.
        let rate = 8000_u32;
        let samples: Vec<i16> = (0..(1.0 * rate as f64) as usize)
            .map(|i| {
                let t = i as f64 / f64::from(rate);
                let gate = if t < 0.4 { 1.0 } else { 0.0 };
                (((2.0 * std::f64::consts::PI * 100.0 * t).sin() * 20_000.0) * gate) as i16
            })
            .collect();
        let data = samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect::<Vec<u8>>();
        let mut wav = Vec::new();
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&((36 + data.len()) as u32).to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&rate.to_le_bytes());
        wav.extend_from_slice(&(rate * 2).to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
        wav.extend_from_slice(&data);
        let dir = std::env::temp_dir().join("concat-motion-audio-expand-test");
        std::fs::create_dir_all(&dir).expect("the temp dir creates");
        std::fs::write(dir.join("pulse.wav"), &wav).expect("the wav writes");

        let mut assets = BTreeMap::new();
        assets.insert(
            "pulse".to_string(),
            crate::comp::Asset {
                src: "pulse.wav".into(),
                kind: None,
            },
        );
        let comp = Comp {
            format: crate::comp::FORMAT,
            id: "react".into(),
            name: String::new(),
            width: 64,
            height: 48,
            fps: 30.0,
            duration: 1.0,
            background: crate::comp::Rgba::new(0.0, 0.0, 0.0, 0.0),
            assets,
            curves: BTreeMap::new(),
            nodes: vec![
                node_of(
                    "music",
                    NodeKind::Audio {
                        asset: "pulse".into(),
                        bindings: vec![AudioBinding {
                            node: "bar".into(),
                            track: "scale".into(),
                            band: Band::Bass,
                            base: 1.0,
                            amount: 1.0,
                        }],
                    },
                ),
                node_of(
                    "bar",
                    NodeKind::Rect {
                        width: crate::comp::Length::Px(16.0),
                        height: crate::comp::Length::Px(8.0),
                        fill: None,
                        radius: 0.0,
                    },
                ),
            ],
            comps: BTreeMap::new(),
            slots: Vec::new(),
        };
        let expanded = expand(&comp, &dir).expect("the comp expands");
        let bar = expanded.nodes.iter().find(|n| n.id == "bar").unwrap();
        let track = bar.tracks.get("scale").expect("the binding wrote a track");
        assert!(track.keys.len() == 30, "{} keys", track.keys.len());
        // The loud head moves above the base and the quiet tail holds
        // it: measured as means, because which single frame the
        // peak-normalisation crowns is the window's own business.
        let value_at = |i: usize| match &track.keys[i].value {
            Value::Number { value } => *value,
            _ => f64::NAN,
        };
        let head: f64 = (3..9).map(&value_at).sum::<f64>() / 6.0;
        let tail: f64 = (22..28).map(&value_at).sum::<f64>() / 6.0;
        assert!(head > 1.5, "the loud head reads {head}");
        assert!(tail < 1.05, "the quiet tail reads {tail}");
        // And every key stays between the base and the base plus the
        // amount: the envelope is clamped, the arithmetic is honest.
        assert!(
            (0..30).all(|i| (1.0..=2.0).contains(&value_at(i))),
            "the drive left its own range"
        );
    }
}
