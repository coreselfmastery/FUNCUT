// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The timing benchmarks: what the motion engine's hot paths cost, said
//! as numbers a person can check. Item 15 of the Geneva roadmap.
//!
//! These are `#[ignore]`d on purpose - the same contract the vision
//! crate's `time_a_frame_at_the_analysis_size` set: a timing assertion
//! would be flaky by nature, so nothing here fails a lane. Run them by
//! hand, in release, and read what they print:
//!
//! ```sh
//! cargo test -p concat-motion --release -- --ignored --nocapture bench
//! ```
//!
//! What is measured, and why each one earns its place:
//!
//! - **the timeline document** (`Geneva::timeline`) is the pure half of
//!   every bake - the JSON the sidecar reads, built for every comp on
//!   every render. It is the floor under the whole engine.
//! - **track sampling** (`Track::sample`) runs per animated property per
//!   frame: a six-second comp at 30 fps with thirty animated properties
//!   samples 5 400 times, and the cost had better be noise next to a
//!   frame draw.
//! - **the streaming hash** (`cache::stream_hash`) is the bake key's own
//!   cost over a big asset - the arithmetic that makes a 2 GB master
//!   clip cost one megabyte of RAM, and the number that says hashing is
//!   not the bottleneck it could be.
//! - **the comps sweep** (`cache::sweep_comps`) is the LRU the bounded
//!   cache promised: a directory walk over every entry's bytes, run
//!   once per bake, so its cost is part of the bake's own budget.
//! - **the noise field** (`Noise::fbm2`, `Noise::drift`) is what an
//!   organic layer samples per frame, and the particle cap (64 motes)
//!   is the worst case the library allows itself.
//!
//! The module is empty in a release build - the benchmarks live under
//! `cfg(test)`, where `#[ignore]` keeps them out of every lane until a
//! person asks for them by name.

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Instant;

    use crate::cache;
    use crate::engine::BakeRequest;
    use crate::geneva::Geneva;
    use crate::noise::Noise;
    use crate::presets;
    use crate::track::{Key, Track, Value};

    /// One megabyte, the unit the reports speak.
    const MIB: u64 = 1024 * 1024;

    /// How long the whole library takes to turn into timeline documents:
    /// eighteen presets, each the JSON a bake would hand the sidecar. The
    /// slowest one is named, because a library is only as fast as its
    /// heaviest comp.
    #[test]
    #[ignore]
    fn time_the_timeline_document_for_the_whole_library() {
        // `at` never launches anything - `timeline` is the pure half - and a
        // path that is not there names that plainly: this measures the
        // document, not the engine that would read it.
        let engine = Geneva::at(PathBuf::from("the-benchmarks-own-geneva"));
        let dir = tempfile::tempdir().expect("a throwaway directory");
        let request = BakeRequest {
            source_dir: dir.path().to_path_buf(),
            ..BakeRequest::default()
        };
        let mut slowest = (f64::NEG_INFINITY, "");
        let started = Instant::now();
        let mut documents = 0usize;
        for preset in presets::ALL {
            let Some(comp) = presets::build(preset.id) else {
                continue;
            };
            let each = Instant::now();
            let (document, warnings) = engine.timeline(&comp, &request);
            let each = each.elapsed().as_secs_f64() * 1000.0;
            if each > slowest.0 {
                slowest = (each, preset.id);
            }
            documents += 1;
            assert!(
                document.get("layers").is_some(),
                "{} answered no layers",
                preset.id
            );
            assert!(warnings.len() <= comp.nodes.len(), "{}", preset.id);
        }
        let total = started.elapsed().as_secs_f64() * 1000.0;
        println!(
            "timeline documents: {documents} presets in {total:.1} ms - slowest {} at {:.1} ms",
            slowest.1, slowest.0
        );
    }

    /// Track sampling at a comp's own rate: thirty animated properties over
    /// a six-second span at 30 fps - 5 400 samples - with six keys and a
    /// spring on the segment, because a linear-only track is the cheap case
    /// wearing the expensive one's name.
    #[test]
    #[ignore]
    fn time_track_sampling_across_a_six_second_span() {
        let track = Track::new(vec![
            Key {
                at: 0.0,
                value: Value::Point { x: 0.0, y: 0.0 },
                curve: Some(crate::curve::Curve::SPRING),
            },
            Key {
                at: 0.2,
                value: Value::Point { x: 32.0, y: 0.0 },
                curve: Some(crate::curve::Curve::EASE_OUT),
            },
            Key {
                at: 0.5,
                value: Value::Point { x: 30.0, y: 4.0 },
                curve: None,
            },
            Key {
                at: 0.75,
                value: Value::Point { x: 31.0, y: 2.0 },
                curve: Some(crate::curve::Curve::TYPEWRITER),
            },
            Key {
                at: 0.9,
                value: Value::Point { x: 28.0, y: 0.0 },
                curve: Some(crate::curve::Curve::ANTICIPATE),
            },
            Key {
                at: 1.0,
                value: Value::Point { x: 0.0, y: 0.0 },
                curve: None,
            },
        ]);
        let properties = 30;
        let frames = 6 * 30;
        let started = Instant::now();
        let mut answers = 0usize;
        for frame in 0..frames {
            // `sample` reads a fraction of the span, which is what the
            // renderer hands it: the frame's instant over the node's own
            // duration.
            let at = frame as f64 / frames as f64;
            for _ in 0..properties {
                if track.sample(at).is_some() {
                    answers += 1;
                }
            }
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "track sampling: {answers} samples in {:.1} ms - {:.0} ns a sample, \
             {} properties over a six-second span at 30 fps",
            elapsed * 1000.0,
            elapsed * 1e9 / answers.max(1) as f64,
            properties
        );
    }

    /// The streaming hash over a 128 MiB asset: the bake key's own cost.
    /// The file is generated, and says so - a real master clip has the same
    /// bytes to walk and no different arithmetic, but the number this
    /// prints was earned on a throwaway, and honesty about that is the
    /// difference between a benchmark and a boast.
    #[test]
    #[ignore]
    fn time_the_streaming_hash_over_a_128mib_asset() {
        let dir = tempfile::tempdir().expect("a throwaway directory");
        let path = dir.path().join("the-128-mib-asset.bin");
        // Deterministic bytes, written in blocks: a counter's little-endian
        // u64s, so the file is compressible only by luck and the hash earns
        // its walk.
        let block = [0u8; 8192];
        let blocks = (128 * MIB / block.len() as u64) as usize;
        {
            use std::io::Write;
            let mut file = std::fs::File::create(&path).expect("the asset");
            let mut counter: u64 = 0x9E37_79B9_7F4A_7C15;
            for _ in 0..blocks {
                let mut bytes = block;
                for pair in bytes.chunks_exact_mut(8) {
                    counter = counter.wrapping_add(0x9E37_79B9_7F4A_7C15);
                    pair.copy_from_slice(&counter.to_le_bytes());
                }
                file.write_all(&bytes).expect("the asset's bytes");
            }
        }
        let bytes = std::fs::metadata(&path).expect("the asset's size").len();
        let started = Instant::now();
        let hash = cache::stream_hash("bench", &path);
        let elapsed = started.elapsed().as_secs_f64();
        assert!(hash.is_some(), "the asset hashed");
        println!(
            "streaming hash: {} MiB in {:.0} ms - {:.0} MiB a second \
             (a generated file, not a real clip)",
            bytes / MIB,
            elapsed * 1000.0,
            bytes as f64 / MIB as f64 / elapsed
        );
    }

    /// The comps LRU sweep over a full cache: 48 bakes of 2 MiB under a
    /// budget of half - the walk that keeps the cache's promise, once a
    /// bake. Every entry carries a manifest (the sentinel a completed bake
    /// writes last) and two megabytes of frames, so the sweep pays the
    /// directory walk and the size count it pays in production.
    #[test]
    #[ignore]
    fn time_the_comps_sweep_over_a_full_cache() {
        let dir = tempfile::tempdir().expect("a throwaway directory");
        let root = dir.path().join("comps");
        std::fs::create_dir_all(&root).expect("the cache root");
        let frame = [7u8; MIB as usize];
        let entries = 48;
        for index in 0..entries {
            let entry = root.join(format!("bake-{index:04x}-1920x1080@30"));
            std::fs::create_dir_all(&entry).expect("the bake's directory");
            std::fs::write(entry.join("frame-000001.png"), frame).expect("a frame");
            std::fs::write(entry.join("frame-000002.png"), frame).expect("a frame");
            // The manifest is the sentinel: written last, read for the clock.
            std::fs::write(
                entry.join("manifest.json"),
                format!("{{\"hash\":\"{index:04x}\",\"seconds\":6.0}}"),
            )
            .expect("the manifest");
        }
        let budget = entries as u64 / 2 * 2 * MIB;
        let started = Instant::now();
        let report = cache::sweep_comps(&root, budget, None);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;
        // The sweep's own unit tests hold its arithmetic; this holds its
        // shape - roughly half evicted, and the cache under budget once
        // the dust settles, manifest bytes included.
        assert!(
            report.evicted.len() >= entries / 2 - 1,
            "the sweep evicted roughly half: {}",
            report.evicted.len()
        );
        assert!(report.kept <= budget, "the cache fits its budget");
        println!(
            "comps sweep: {} entries, {} MiB walked, {} evicted in {:.0} ms",
            entries,
            entries * 2,
            report.evicted.len(),
            elapsed
        );
    }

    /// The organic layers at their worst: the particle cap is 64 motes, and
    /// each mote asks the field for its place every frame - drift's two
    /// channels beside them, because a drift layer asks on its own. Six
    /// seconds at 30 fps of the busiest organic comp the library allows.
    #[test]
    #[ignore]
    fn time_noise_at_the_particle_cap() {
        let noise = Noise::from_seed(20261103);
        let motes = 64u32;
        let frames = 6 * 30;
        let started = Instant::now();
        for frame in 0..frames {
            let at = frame as f64 / 30.0;
            for mote in 0..motes {
                let x = noise.fbm2(f64::from(mote) * 0.37, at * 0.4, 3);
                let y = noise.fbm2(f64::from(mote) * 0.11 + 40.0, at * 0.31, 3);
                let _ = noise.noise2(x, y);
            }
            let (dx, dy) = noise.drift(at, 0);
            let _ = (dx, dy);
            let (dx, dy) = noise.drift(at, 1);
            let _ = (dx, dy);
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "organic layers: {} frames x {} motes plus drift in {:.0} ms - \
             {:.1} us a frame",
            frames,
            motes,
            elapsed * 1000.0,
            elapsed * 1e6 / frames as f64
        );
    }
}
