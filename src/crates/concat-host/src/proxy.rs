// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! Proxies: a smaller copy of a large file, written once into the
//! project's cache, that playback and the filmstrips read instead of the
//! original.
//!
//! A 4K file decodes at a few frames a second on a laptop without
//! hardware decode; its quarter-size copy decodes at dozens. So a file
//! as large as HD gets one, written on the scheduler's proxy lane - one
//! at a time, or two on a machine with workers to spare, each on two
//! threads, so the machine stays usable meanwhile - and
//! adopted into the reader pool as the file's stand-in. A request that
//! asks for the proxy reads it; the paused monitor and the export never
//! do, and see the original. The copy is named after the file's path,
//! size, modification time and a probe of its bytes, so a file replaced
//! on disk gets a fresh one - however exactly the replacement keeps the
//! original's length and timestamp - and the old copy is swept with the
//! rest of the cache.
//!
//! Which codec the copy is written in is Settings' to say, as
//! [`ProxyCodec`]: H.264, the small file, or MJPEG, whose every frame is
//! a keyframe of its own and which nothing has to seek within - the
//! scrubbiest there is, at several times the file.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use concat_core::frame::Signal;
use concat_media::decode::{DecodeOptions, Decoder, FrameSource};
use concat_media::{ColorRange, EncodeOptions, Encoder, FrameSink, Priority, RateMode, VideoCodec};

/// A file with at least this many pixels gets a proxy: full HD and
/// anything larger. Full HD included - the most common frame there is
/// is exactly the size a working copy pays for itself at: an agent's
/// preview, at 960 on the long side, reads the copy instead of the
/// original, and so does Half-quality playback.
const PIXELS_PROXIED_FROM: u64 = 1920 * 1080;
/// The proxy's long side is the original's divided by this, and never
/// shorter than [`SHORTEST_LONG_SIDE`].
const DIVISOR: u32 = 4;
const SHORTEST_LONG_SIDE: u32 = 960;

/// The codec a proxy is written in, chosen in Settings > General. An HDR
/// file's proxy keeps its signal whatever this says - HEVC in ten bits,
/// HLG or PQ as the file is - because MJPEG carries neither.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum ProxyCodec {
    /// H.264, veryfast, CRF 23: the small copy, quick enough to decode.
    /// The default - the one every proxy was before the setting existed,
    /// which is why its name stays the one those copies are filed under.
    #[default]
    H264,
    /// MJPEG at qscale 3: every frame a keyframe, nothing to seek to and
    /// no neighbours to reconstruct first, so a drag of the playhead
    /// reads whatever frame it lands on at once - about half again the
    /// scrub speed of H.264 on the same machine, in Blender's proxy
    /// benchmark, for several times the file. The choice for a disk with
    /// room and a timeline that must follow the pointer.
    Mjpeg,
}

impl ProxyCodec {
    /// Every codec, in the order the Settings menu lists them.
    pub const ALL: [ProxyCodec; 2] = [ProxyCodec::H264, ProxyCodec::Mjpeg];

    /// The name a setting stores.
    pub fn name(self) -> &'static str {
        match self {
            ProxyCodec::H264 => "h264",
            ProxyCodec::Mjpeg => "mjpeg",
        }
    }

    /// The codec a stored name means, or `None` for one nobody stores.
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim().to_ascii_lowercase().as_str() {
            "h264" | "h.264" => Some(ProxyCodec::H264),
            "mjpeg" | "m-jpeg" | "motion jpeg" => Some(ProxyCodec::Mjpeg),
            _ => None,
        }
    }

    /// The index into [`ProxyCodec::ALL`] a stored name is, so the sheet's
    /// dropdown can be seeded from the preference.
    pub fn index(self) -> usize {
        ProxyCodec::ALL
            .iter()
            .position(|codec| *codec == self)
            .unwrap_or(0)
    }
}

/// Whether a `width` by `height` file is worth a proxy.
pub fn wanted(width: u32, height: u32) -> bool {
    u64::from(width) * u64::from(height) >= PIXELS_PROXIED_FROM
}

/// The proxy's size for a `width` by `height` original: the long side a
/// quarter of the original's, not under [`SHORTEST_LONG_SIDE`] and never
/// over the original, the aspect kept, both sides even.
pub fn size_for(width: u32, height: u32) -> (u32, u32) {
    let (width, height) = (width.max(2), height.max(2));
    let long = width.max(height);
    let target = (long / DIVISOR).max(SHORTEST_LONG_SIDE).min(long);
    let scale = f64::from(target) / f64::from(long);
    let side = |px: u32| (((f64::from(px) * scale).round() as u32) & !1).max(2);
    (side(width), side(height))
}

/// Whether a frame `width` by `height` is small enough that every proxy
/// can serve it whole: a proxy's long side is at least
/// [`SHORTEST_LONG_SIDE`] - a file smaller than that never gets one - so
/// a frame no longer than this reads each clip's working copy without
/// upscaling it. The same picture, from the smaller file: what a caller
/// paging through an edit wants to read fast, and what the API's small
/// preview frames are served from where the media has its copy.
pub fn serves_frame(width: u32, height: u32) -> bool {
    width.max(height) <= SHORTEST_LONG_SIDE
}

/// How much of a file's head, middle and tail is read to name its proxy:
/// three small reads that differ between two same-sized files almost
/// everywhere, and not a pass over tens of gigabytes that a lookup
/// cannot afford - it happens for every clip when a project opens, and
/// for every clip still waiting for its copy while the write runs.
const PROBE: u64 = 64 * 1024;

/// Folds a probe of the file's bytes into `hasher`: the first, the
/// middle and the last [`PROBE`] bytes, each with the offset it came
/// from - a file no longer than the probe is read whole, once, its head,
/// middle and tail being the same bytes. What this adds to the path, the
/// size and the modification time is the one thing none of them can say:
/// what the file's bytes are. A replacement that keeps the original's
/// length and timestamp - a backup tool restoring what it saved, an
/// editor writing in place - is the case those cannot tell from an
/// unchanged file, and the bytes can (audit 2026-11-04, #2). A file that
/// cannot be read past its metadata contributes what it can: a key made
/// without its bytes is a key that only asks for the copy again.
fn probe_into(file: &mut std::fs::File, len: u64, hasher: &mut impl Hasher) {
    use std::io::{Read, Seek, SeekFrom};
    let probe = PROBE.min(len);
    let starts: Vec<u64> = if probe == len {
        vec![0]
    } else {
        vec![0, len / 2 - probe / 2, len - probe]
    };
    let mut bytes = vec![0u8; probe as usize];
    for at in starts {
        if file.seek(SeekFrom::Start(at)).is_err() || file.read_exact(&mut bytes).is_err() {
            return;
        }
        at.hash(hasher);
        bytes.hash(hasher);
    }
}

/// Where the proxy of `media` lives under `project`: named by the file's
/// path, size, modification time and a probe of its bytes, and by the
/// range it is read as, since the copy is written from the corrected
/// picture and a correction changed is a different copy - and by whether
/// the file is HDR, whose proxy keeps its signal, so the tone-mapped
/// copies earlier builds wrote of one are written again. A proxy in the
/// codec other than H.264 - the one every proxy was before the codec was
/// a choice - is named for that too, so a switch of codec in Settings
/// writes the choice's copy and not the old one renamed. None for a file
/// that cannot be stat'ed, which cannot be read either.
pub fn path_for(
    project: &Path,
    media: &str,
    range: Option<ColorRange>,
    hdr: bool,
    codec: ProxyCodec,
) -> Option<PathBuf> {
    let meta = std::fs::metadata(media).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    media.hash(&mut hasher);
    meta.len().hash(&mut hasher);
    // The bytes tell a replacement from the file it replaced where the
    // length and the timestamp cannot: see `probe_into`, whose probe is
    // folded in wherever the file can be opened.
    if let Ok(mut file) = std::fs::File::open(media) {
        probe_into(&mut file, meta.len(), &mut hasher);
    }
    if let Some(range) = range {
        range.name().hash(&mut hasher);
    }
    if hdr {
        "hdr".hash(&mut hasher);
    }
    if codec != ProxyCodec::H264 {
        codec.name().hash(&mut hasher);
    }
    if let Ok(modified) = meta.modified()
        && let Ok(since) = modified.duration_since(std::time::UNIX_EPOCH)
    {
        // Keep sub-second precision: replacing a same-size source twice
        // within one second must not reuse the first source's proxy.
        since.as_nanos().hash(&mut hasher);
    }
    Some(
        project
            .join("cache")
            .join("proxy")
            .join(format!("{:016x}.mp4", hasher.finish())),
    )
}

/// The proxy of `media` under `project`, read as `range`, when it has
/// been written - an HDR one or an SDR one, in either codec, whichever
/// there is: a filmstrip reads any of them.
pub fn existing(project: &Path, media: &str, range: Option<ColorRange>) -> Option<PathBuf> {
    [true, false]
        .into_iter()
        .flat_map(|hdr| ProxyCodec::ALL.map(move |codec| (hdr, codec)))
        .filter_map(|(hdr, codec)| path_for(project, media, range, hdr, codec))
        .find(|path| path.is_file())
}

/// Sees to it that `media` - `width` by `height`, read as `range` - has
/// a proxy when it is worth one: an existing copy is adopted into the
/// reader pool at once; otherwise one is queued to be written on the
/// proxy lane and adopted when done. Asking again while it is being
/// written is free. Returns whether the file has, or will have, a proxy.
pub fn ensure(
    project: &Path,
    media: &str,
    width: u32,
    height: u32,
    range: Option<ColorRange>,
    hdr: bool,
    codec: ProxyCodec,
) -> bool {
    if !wanted(width, height) {
        return false;
    }
    let Some(target) = path_for(project, media, range, hdr, codec) else {
        return false;
    };
    let pool = crate::scheduler().pool();
    if target.is_file() {
        if pool.proxy_of(Path::new(media)).as_deref() != Some(target.as_path()) {
            pool.adopt_proxy(Path::new(media), target);
        }
        return true;
    }
    let writing = writing();
    {
        let mut writing = writing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if !writing.insert(target.clone()) {
            return true;
        }
    }
    let source = media.to_owned();
    let size = size_for(width, height);
    crate::scheduler().submit(Priority::Proxy, move || {
        match write(&source, &target, size, range, codec) {
            Ok(()) => {
                crate::scheduler()
                    .pool()
                    .adopt_proxy(Path::new(&source), target.clone());
                log::info!("{source}: proxy written to {}", target.display());
            }
            Err(error) => log::warn!("{source}: no proxy: {error}"),
        }
        writing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&target);
    });
    true
}

/// The proxies being written now, by target, so one is never queued
/// twice and a sweep leaves it for its writer to finish. Process-local:
/// what crosses processes is the lock [`write`] takes beside its partial.
fn writing() -> &'static Mutex<HashSet<PathBuf>> {
    static WRITING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    WRITING.get_or_init(|| Mutex::new(HashSet::new()))
}

/// The lock file beside `partial` that names the process writing it.
fn lock_beside(partial: &Path) -> PathBuf {
    partial.with_extension("lock")
}

/// Takes the lock file at `path`, or `None` when another process holds it
/// or it cannot be made. The operating system's lock, not the file's: it
/// lives exactly as long as the handle, so a process that crashes or is
/// killed lets go of it and the file it leaves is taken over by the next
/// writer without anybody deciding whether its pid still runs.
fn take_lock_file(path: &Path) -> Option<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .ok()?;
    match file.try_lock() {
        Ok(()) => Some(file),
        Err(_) => None,
    }
}

/// The lock that makes one proxy one write on one machine. The lock file
/// is a permanent sentinel: unlinking it while a handle still owns its
/// lock lets another process create a new file at the same path and take
/// a second lock on a different inode. Keeping the sentinel means every
/// process always locks the same file, even after a completed write.
struct WritingLock {
    /// The OS releases this lock when the handle is dropped.
    _held: std::fs::File,
}

/// Takes the writing lock for `partial`'s proxy, or `None` when another
/// process is writing it.
fn take_writing_lock(partial: &Path) -> Option<WritingLock> {
    let path = lock_beside(partial);
    take_lock_file(&path).map(|file| WritingLock { _held: file })
}

/// Removes a crash-left partial only while holding the same stable lock a
/// writer must take before opening it. Creating a missing sentinel first
/// closes the race with a writer that is just about to start.
fn remove_abandoned_partial(partial: &Path) -> bool {
    let Some(_lock) = take_lock_file(&lock_beside(partial)) else {
        return false;
    };
    std::fs::remove_file(partial).is_ok()
}

/// Removes the proxies under `project` that `keep` does not name: the
/// copies of media no longer in the project, or read as a range they no
/// longer are. Nothing else sweeps this folder, and a proxy is a
/// quarter-size copy of every file over HD the project ever held
/// (audit 2026-09-23, #15). One being written is left for its writer.
/// Returns how many files went.
pub fn sweep(project: &Path, keep: &HashSet<PathBuf>) -> usize {
    let dir = project.join("cache").join("proxy");
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let writing = writing()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut removed = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.ends_with(".part.mp4") {
            // A proxy in mid-write sits under a `.part.mp4` name - the
            // muxer needs the extension to know the container. Its writer
            // holds the lock beside it, in this process or another; a lock
            // that answers - or that was never there, from a build that
            // wrote no lock - belongs to nobody, and the partial is a
            // crash's leftovers and goes (audit 2026-11-04, #9). A live
            // writer's lock does not answer, and its partial is left for
            // it to finish and rename.
            if remove_abandoned_partial(&path) {
                removed += 1;
            }
            continue;
        }
        if name.ends_with(".part.lock") {
            // Keep the lock sentinel even after its partial is gone. Removing
            // an unlocked lock file races a writer opening the same path:
            // the two handles could then lock different inodes.
            continue;
        }
        if path.extension().is_some_and(|extension| extension == "mp4")
            && !keep.contains(&path)
            && !writing.contains(&path)
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

/// Writes the proxy of `source`, read as `range`, at `size` to `target`
/// in `codec` (an HDR source's copy keeps its signal and is HEVC
/// whatever the codec says, since MJPEG carries neither): two threads,
/// in software, into a temporary name that is moved into place only when
/// whole, so a copy that is there is a copy that is complete. The copy
/// is tagged with the levels it holds and converted to match - video
/// range for H.264, JPEG's own full range for MJPEG, which the encoder
/// will take no other way - so it is read as it is and needs no
/// correction of its own. An HDR source's copy keeps its signal - HEVC
/// in ten bits, HLG or PQ as the source is, on the hardware encoder
/// where there is one - so an HDR timeline plays it as the original
/// draws when paused.
pub fn write(
    source: &str,
    target: &Path,
    (width, height): (u32, u32),
    range: Option<ColorRange>,
    codec: ProxyCodec,
) -> Result<(), String> {
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    // Written beside the target under a name the muxer still knows the
    // format of, and moved into place only when whole.
    let partial = target.with_extension("part.mp4");
    // The cross-process line before anything is read or written: one
    // proxy, one writer, on the whole machine. Another process holding it
    // is not an error there - its write will finish what this one was
    // queued for - and the next [`ensure`] after that adopts the copy it
    // lands.
    let _one_writer = take_writing_lock(&partial)
        .ok_or_else(|| "another Concat process is writing this proxy".to_owned())?;
    let info = concat_media::probe(source).map_err(|error| error.to_string())?;
    let (rate, signal) = info
        .video
        .as_ref()
        .map(|video| (video.frame_rate, video.signal))
        .ok_or_else(|| "no video stream".to_owned())?;
    let hdr = matches!(signal, Signal::Hlg | Signal::Pq);
    let options = DecodeOptions::default()
        .scaled_to(width, height)
        .in_software()
        .threaded(2)
        .in_range(range)
        .deep(hdr);
    let mut decoder = Decoder::open(source, &options).map_err(|error| error.to_string())?;
    // The codec Settings chose for SDR copies. MJPEG's quality is its
    // qscale - three is the production-tested profile, near enough to
    // the original at a quarter the size - and its levels are JPEG's
    // own, full range, which the encoder insists on and the copy reads
    // back as (`ColorRange::implied` knows an MJPEG file is full range,
    // so it needs no correction of its own).
    let mjpeg = codec == ProxyCodec::Mjpeg;
    let encoding = EncodeOptions {
        codec: if hdr {
            VideoCodec::Hevc
        } else if mjpeg {
            VideoCodec::Mjpeg
        } else {
            VideoCodec::H264
        },
        preset: "veryfast".to_owned(),
        crf: if mjpeg { 3 } else { 23 },
        // A proxy is a working copy: the CRF says how good, and no
        // bitrate target ever applies to it.
        rate_mode: RateMode::Vbr,
        bitrate_kbps: 0,
        ten_bit: false,
        color_range: if mjpeg && !hdr {
            ColorRange::Full
        } else {
            ColorRange::Limited
        },
        // HEVC in ten bits costs the CPU far more than H.264 does; the
        // media engine does it for nothing where there is one.
        hardware: hdr,
        threads: 2,
    };
    let mut encoder = if hdr {
        Encoder::create_hdr(&partial, width, height, rate, &encoding, signal)
    } else {
        Encoder::create(&partial, width, height, rate, &encoding)
    }
    .map_err(|error| error.to_string())?;
    let result = (|| {
        while let Some(frame) = decoder.next_frame().map_err(|error| error.to_string())? {
            encoder
                .write_frame(&frame)
                .map_err(|error| error.to_string())?;
        }
        encoder.finish().map_err(|error| error.to_string())
    })();
    // Windows will not rename or delete a file something still holds open,
    // and a finished encoder keeps its output open until it is dropped.
    drop(encoder);
    drop(decoder);
    match result {
        Ok(()) => std::fs::rename(&partial, target)
            .map_err(|error| format!("{}: {error}", target.display())),
        Err(error) => {
            let _ = std::fs::remove_file(&partial);
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An HDR file's proxy keeps its signal: HEVC, HLG as the file is,
    /// its grey coming back as the same signal - under a name of its own,
    /// so the tone-mapped copy an earlier build wrote is written again.
    #[test]
    fn an_hdr_file_gets_an_hdr_proxy() {
        use concat_core::frame::{Depth, Frame};
        use concat_core::time::FrameRate;
        if !VideoCodec::Hevc.available() {
            eprintln!("no HEVC encoder in the linked FFmpeg; skipped");
            return;
        }
        let dir = std::env::temp_dir().join(format!("concat-proxy-hdr-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("iphone.mp4");
        let value = (0.6f64 * 65_535.0).round() as u16;
        {
            let mut encoder = Encoder::create_hdr(
                &media,
                640,
                360,
                FrameRate::THIRTY,
                &EncodeOptions {
                    codec: VideoCodec::Hevc,
                    preset: "ultrafast".to_owned(),
                    hardware: false,
                    ..EncodeOptions::default()
                },
                Signal::Hlg,
            )
            .expect("encodes");
            let pixel: Vec<u8> = [value, value, value, u16::MAX]
                .iter()
                .flat_map(|channel| channel.to_le_bytes())
                .collect();
            let frame = Frame::from_rgba64(640, 360, pixel.repeat(640 * 360), Signal::Hlg)
                .expect("a deep frame");
            for _ in 0..6 {
                encoder.write_frame(&frame).expect("writes");
            }
            encoder.finish().expect("finishes");
        }
        let source = media.to_string_lossy().into_owned();
        let project = dir.join("project");
        let target = path_for(&project, &source, None, true, ProxyCodec::H264).expect("a path");
        assert_ne!(
            Some(&target),
            path_for(&project, &source, None, false, ProxyCodec::H264).as_ref()
        );
        write(&source, &target, (320, 180), None, ProxyCodec::H264).expect("writes the proxy");
        let info = concat_media::probe(&target).expect("probes");
        let video = info.video.expect("has pictures");
        assert_eq!((video.codec.as_str(), video.signal), ("hevc", Signal::Hlg));
        let mut decoder =
            Decoder::open(&target, &DecodeOptions::default().deep(true)).expect("opens");
        let frame = decoder.next_frame().expect("decodes").expect("a frame");
        assert_eq!(frame.depth(), Depth::Sixteen);
        let at = ((90 * 320 + 160) * 8) as usize;
        let got = u16::from_le_bytes([frame.pixels()[at], frame.pixels()[at + 1]]);
        assert!(
            got.abs_diff(value) < 1_000,
            "the HLG grey {value} came back {got}"
        );
        assert_eq!(existing(&project, &source, None), Some(target));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn full_hd_files_get_a_proxy_too() {
        // Full HD itself: the most common frame there is reads its
        // quarter-size copy at Half and Quarter, and an agent's 960-side
        // preview comes from it. Below HD, the original is already light.
        assert!(wanted(1920, 1080));
        assert!(wanted(1080, 1920));
        assert!(!wanted(1280, 720));
        assert!(wanted(3840, 2160));
        assert!(wanted(2560, 1440));
        assert!(!wanted(0, 0));
    }

    #[test]
    fn a_proxy_is_a_quarter_of_the_long_side_but_never_tiny_or_larger() {
        assert_eq!(size_for(3840, 2160), (960, 540));
        assert_eq!(size_for(7680, 4320), (1920, 1080));
        // A tall file keeps its way up.
        assert_eq!(size_for(2160, 3840), (540, 960));
        // A file too small for a quarter gets the floor, never upscaled.
        assert_eq!(size_for(2560, 1440), (960, 540));
        assert_eq!(size_for(640, 480), (640, 480));
        // Odd sizes come out even, and nothing is ever under two.
        let (w, h) = size_for(3841, 2161);
        assert!(w % 2 == 0 && h % 2 == 0);
        assert_eq!(size_for(1, 1), (2, 2));
    }

    /// A frame no longer than a proxy's floor is served whole by every
    /// copy there is; a frame at the timeline's own size is not, and
    /// stays on the originals.
    #[test]
    fn a_frame_no_longer_than_a_proxy_is_served_whole() {
        assert!(serves_frame(960, 540));
        assert!(serves_frame(540, 960), "a tall frame keeps the service");
        assert!(serves_frame(480, 270));
        assert!(!serves_frame(1920, 1080));
        assert!(!serves_frame(1080, 1920));
    }

    /// A proxy written from a file as large as HD is a quarter of it, is
    /// adopted by the pool, and serves a moving request while the paused
    /// one still reads the original.
    #[test]
    fn a_written_proxy_is_read_for_a_moving_picture_only() {
        use concat_core::frame::Frame;
        use concat_core::time::FrameRate;
        let dir = std::env::temp_dir().join(format!("concat-proxy-write-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("big.mp4");
        {
            let mut encoder = Encoder::create(
                &media,
                2560,
                1440,
                FrameRate::THIRTY,
                &EncodeOptions {
                    preset: "ultrafast".to_owned(),
                    hardware: false,
                    ..EncodeOptions::default()
                },
            )
            .expect("encodes");
            for index in 0..6u8 {
                let mut frame = Frame::black(2560, 1440);
                frame.fill([index * 40, 20, 200, 255]);
                encoder.write_frame(&frame).expect("writes");
            }
            encoder.finish().expect("finishes");
        }
        let project = dir.join("project");
        let source = media.to_string_lossy().into_owned();
        assert!(
            ensure(&project, &source, 2560, 1440, None, false, ProxyCodec::H264),
            "worth a proxy"
        );
        crate::scheduler().drain();
        let proxy = existing(&project, &source, None).expect("the proxy was written");
        let info = concat_media::probe(&proxy).expect("probes");
        let video = info.video.expect("has pictures");
        assert_eq!((video.width, video.height), (960, 540));
        let pool = crate::scheduler().pool();
        assert_eq!(
            pool.proxy_of(Path::new(&source)).as_deref(),
            Some(proxy.as_path())
        );
        // Second time round: nothing to write, still adopted.
        assert!(ensure(
            &project,
            &source,
            2560,
            1440,
            None,
            false,
            ProxyCodec::H264
        ));
        let time = FrameRate::THIRTY.time_of_frame(2);
        let moving = pool
            .frame(&concat_media::FrameRequest::new(&source, time, 480, 270).from_proxy(true))
            .expect("reads the proxy");
        assert_eq!((moving.width(), moving.height()), (480, 270));
        let paused = pool
            .frame(&concat_media::FrameRequest::new(&source, time, 480, 270))
            .expect("reads the original");
        assert_eq!((paused.width(), paused.height()), (480, 270));
        // A file not worth a proxy is left alone.
        assert!(!ensure(
            &project,
            &source,
            1280,
            720,
            None,
            false,
            ProxyCodec::H264
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A proxy in MJPEG is a quarter of the file, every frame a keyframe,
    /// and full range as JPEG is by definition: the colours come back as
    /// they went in, not lifted by a range the copy forgot it held. Its
    /// name is the H.264 one's plus the codec, so the two never stand in
    /// for each other.
    #[test]
    fn an_mjpeg_proxy_round_trips_the_picture_and_has_its_own_name() {
        use concat_core::frame::Frame;
        use concat_core::time::FrameRate;
        let dir = std::env::temp_dir().join(format!("concat-proxy-mjpeg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("big.mp4");
        {
            let mut encoder = Encoder::create(
                &media,
                2560,
                1440,
                FrameRate::THIRTY,
                &EncodeOptions {
                    preset: "ultrafast".to_owned(),
                    hardware: false,
                    ..EncodeOptions::default()
                },
            )
            .expect("encodes");
            for index in 0..6u8 {
                let mut frame = Frame::black(2560, 1440);
                frame.fill([index * 40, 20, 200, 255]);
                encoder.write_frame(&frame).expect("writes");
            }
            encoder.finish().expect("finishes");
        }
        let project = dir.join("project");
        let source = media.to_string_lossy().into_owned();
        let h264 = path_for(&project, &source, None, false, ProxyCodec::H264).expect("a path");
        let mjpeg = path_for(&project, &source, None, false, ProxyCodec::Mjpeg).expect("a path");
        assert_ne!(h264, mjpeg, "a codec switch is another copy");
        assert!(
            ensure(
                &project,
                &source,
                2560,
                1440,
                None,
                false,
                ProxyCodec::Mjpeg
            ),
            "worth a proxy"
        );
        crate::scheduler().drain();
        assert!(mjpeg.is_file(), "the MJPEG copy was written");
        let info = concat_media::probe(&mjpeg).expect("probes");
        let video = info.video.expect("has pictures");
        assert_eq!(
            (video.width, video.height),
            (960, 540),
            "a quarter of the long side, as the H.264 one is"
        );
        let pool = crate::scheduler().pool();
        assert_eq!(
            pool.proxy_of(Path::new(&source)).as_deref(),
            Some(mjpeg.as_path())
        );
        // The last frame is (200, 20, 200) in the original's terms; the
        // proxy's copy of it comes back near enough - a washed-out
        // number is a range the copy held and the read-back denied.
        let time = FrameRate::THIRTY.time_of_frame(5);
        let frame = pool
            .frame(&concat_media::FrameRequest::new(&source, time, 480, 270).from_proxy(true))
            .expect("reads the proxy");
        let at = 4 * (200 * 480 + 240);
        let pixel = |shift: usize| frame.pixels()[at + shift];
        for (channel, wanted) in [(0, 200u8), (1, 20), (2, 200)] {
            let got = pixel(channel);
            assert!(
                got.abs_diff(wanted) < 24,
                "channel {channel} came back {got}, wanted near {wanted}"
            );
        }
        // A filmstrip's reader finds the MJPEG copy as it would the
        // H.264 one.
        assert_eq!(existing(&project, &source, None), Some(mjpeg));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_proxy_path_follows_the_file_and_changes_when_the_file_does() {
        let dir = std::env::temp_dir().join(format!("concat-proxy-path-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("clip.mp4");
        std::fs::write(&media, b"one").expect("writes");
        let project = dir.join("project");
        let first = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("a path");
        assert!(first.starts_with(project.join("cache").join("proxy")));
        assert!(existing(&project, &media.to_string_lossy(), None).is_none());
        // Read as another range, the file is another proxy too: the copy
        // holds the corrected picture.
        let full = path_for(
            &project,
            &media.to_string_lossy(),
            Some(ColorRange::Full),
            false,
            ProxyCodec::H264,
        )
        .expect("a path");
        assert_ne!(first, full, "a file read as full range is another proxy");
        std::fs::write(&media, b"one more byte").expect("writes");
        let second = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("a path");
        assert_ne!(first, second, "a changed file is another proxy");
        assert!(
            path_for(
                &project,
                &dir.join("missing.mp4").to_string_lossy(),
                None,
                false,
                ProxyCodec::H264,
            )
            .is_none()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_size_replacement_within_one_second_gets_a_new_proxy_key() {
        use std::time::{Duration, UNIX_EPOCH};

        let dir = std::env::temp_dir().join(format!(
            "concat-proxy-subsecond-path-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("clip.mp4");
        let project = dir.join("project");
        std::fs::write(&media, b"first payload").expect("writes first source");

        let first_time = UNIX_EPOCH + Duration::new(1_700_000_000, 100_000_000);
        let second_time = UNIX_EPOCH + Duration::new(1_700_000_000, 900_000_000);
        let set_modified = |time| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&media)
                .expect("opens source for timestamp update")
                .set_times(std::fs::FileTimes::new().set_modified(time))
                .expect("sets modification time");
        };

        set_modified(first_time);
        let first_metadata = std::fs::metadata(&media).expect("first metadata");
        let first_modified = first_metadata.modified().expect("first mtime");
        let first = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("first proxy path");

        // The replacement has different bytes but exactly the same length.
        std::fs::write(&media, b"other payload").expect("writes replacement source");
        set_modified(second_time);
        let second_metadata = std::fs::metadata(&media).expect("second metadata");
        let second_modified = second_metadata.modified().expect("second mtime");

        assert_eq!(first_metadata.len(), second_metadata.len());
        let first_since = first_modified
            .duration_since(UNIX_EPOCH)
            .expect("first time is after the epoch");
        let second_since = second_modified
            .duration_since(UNIX_EPOCH)
            .expect("second time is after the epoch");
        if first_since.as_secs() != second_since.as_secs()
            || first_since.subsec_nanos() == second_since.subsec_nanos()
        {
            eprintln!("filesystem does not preserve sub-second modification times; skipped");
            let _ = std::fs::remove_dir_all(&dir);
            return;
        }

        let second = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("replacement proxy path");
        assert_ne!(
            first, second,
            "same-size replacements within one second must not share a proxy"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A replacement that keeps the original's length and its
    /// modification time exactly - a backup tool restoring what it saved,
    /// an editor writing in place - is the case no fact about the file's
    /// name can tell from an unchanged file: the bytes can (audit
    /// 2026-11-04, #2).
    #[test]
    fn same_size_same_time_replacement_gets_a_new_proxy_key() {
        use std::time::{Duration, UNIX_EPOCH};

        let dir =
            std::env::temp_dir().join(format!("concat-proxy-content-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("clip.mp4");
        let project = dir.join("project");
        std::fs::write(&media, b"first payload").expect("writes first source");

        // One timestamp, set again on the replacement: whatever precision
        // the filesystem keeps, both sources carry the same one.
        let time = UNIX_EPOCH + Duration::new(1_700_000_000, 500_000_000);
        let set_modified = |time| {
            std::fs::OpenOptions::new()
                .write(true)
                .open(&media)
                .expect("opens source for timestamp update")
                .set_times(std::fs::FileTimes::new().set_modified(time))
                .expect("sets modification time");
        };

        set_modified(time);
        let first = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("first proxy path");
        let first_mtime = std::fs::metadata(&media)
            .expect("first metadata")
            .modified()
            .expect("first mtime");

        // The replacement has different bytes, exactly the same length,
        // and exactly the timestamp the first source had.
        std::fs::write(&media, b"other payload").expect("writes replacement source");
        set_modified(time);

        let second_metadata = std::fs::metadata(&media).expect("replacement metadata");
        assert_eq!(
            second_metadata.len(),
            13,
            "the replacement keeps the length"
        );
        assert_eq!(
            second_metadata.modified().expect("replacement mtime"),
            first_mtime,
            "and the modification time, exactly"
        );

        let second = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("replacement proxy path");
        assert_ne!(
            first, second,
            "a replacement the file's facts cannot tell apart is told apart by its bytes"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The probe of an unchanged file is the same probe every time: a key
    /// that moved between two looks at one file would rewrite its copy
    /// over and over, which is the failure the key exists to prevent -
    /// the wrong copy, not no copy - in the other direction.
    #[test]
    fn an_unchanged_file_keeps_one_key() {
        let dir =
            std::env::temp_dir().join(format!("concat-proxy-stable-path-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        let media = dir.join("clip.mp4");
        let project = dir.join("project");
        // A file over one probe long, so the head, the middle and the
        // tail are three different reads of it, and a file under it, read
        // whole: both must sit still.
        std::fs::write(&media, vec![b'x'; 256 * 1024]).expect("writes a long source");
        let long = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("a path");
        let again = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("the same path");
        assert_eq!(long, again, "an unchanged file keeps its key");

        std::fs::write(&media, b"short but whole").expect("writes a short source");
        let short = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("a path");
        let once_more = path_for(
            &project,
            &media.to_string_lossy(),
            None,
            false,
            ProxyCodec::H264,
        )
        .expect("the same path");
        assert_eq!(short, once_more, "a short unchanged file keeps its key too");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sweep_keeps_what_is_named_and_what_is_being_written() {
        let scratch =
            std::env::temp_dir().join(format!("concat-proxy-sweep-{}", std::process::id()));
        let dir = scratch.join("cache").join("proxy");
        std::fs::create_dir_all(&dir).expect("a proxy folder");
        let kept = dir.join("1111111111111111.mp4");
        let stale = dir.join("2222222222222222.mp4");
        let busy = dir.join("3333333333333333.mp4");
        // The real partial's name: `write` puts it beside the target,
        // extension kept for the muxer, `.part` before it.
        let partial = dir.join("4444444444444444.part.mp4");
        // A partial whose writer is live: it holds the lock beside it, in
        // this process as any other, and the sweep leaves both alone.
        let written = dir.join("5555555555555555.part.mp4");
        for file in [&kept, &stale, &busy, &partial, &written] {
            std::fs::write(file, b"x").expect("writes");
        }
        let live = take_lock_file(&lock_beside(&written)).expect("the writer's lock");
        writing()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(busy.clone());
        let keep: HashSet<PathBuf> = [kept.clone()].into_iter().collect();
        // The stale copy and the dead partial go; the kept copy, the busy
        // one and the live writer's partial stay.
        assert_eq!(sweep(&scratch, &keep), 2);
        assert!(kept.is_file() && busy.is_file() && written.is_file());
        assert!(!stale.is_file());
        assert!(
            !partial.is_file(),
            "a partial no writer holds is a crash's leftovers"
        );
        // The writer lets go - as a finished write does - and what it left
        // behind is the next sweep's to take.
        drop(live);
        assert_eq!(sweep(&scratch, &keep), 1);
        assert!(!written.is_file());
        assert!(
            lock_beside(&written).is_file(),
            "the stable lock sentinel remains after cleanup"
        );
        writing()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&busy);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    #[test]
    fn abandoned_partial_cleanup_and_writers_share_one_stable_lock() {
        let scratch =
            std::env::temp_dir().join(format!("concat-proxy-cleanup-lock-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let dir = scratch.join("cache").join("proxy");
        std::fs::create_dir_all(&dir).expect("a proxy folder");
        let target = dir.join("7777777777777777.mp4");
        let partial = target.with_extension("part.mp4");
        std::fs::write(&partial, b"interrupted proxy").expect("writes partial");

        let live_writer = take_writing_lock(&partial).expect("writer takes the sentinel");
        assert!(
            !remove_abandoned_partial(&partial),
            "cleanup cannot remove a partial while its writer owns the lock"
        );
        assert!(partial.is_file(), "the live writer's partial stays");
        drop(live_writer);

        assert!(
            remove_abandoned_partial(&partial),
            "cleanup takes the now-free sentinel before removing the partial"
        );
        assert!(!partial.exists(), "the abandoned partial is removed");
        assert!(
            lock_beside(&partial).is_file(),
            "cleanup leaves the sentinel in place for the next writer"
        );
        let next_writer = take_writing_lock(&partial)
            .expect("the next writer acquires the same sentinel after cleanup");
        drop(next_writer);

        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// One proxy, one writer, on the whole machine: a write refuses while
    /// another holds the stable lock beside the partial, then a later writer
    /// acquires that same sentinel after the first lets go.
    #[test]
    fn a_second_writer_of_one_proxy_is_refused_until_the_first_is_done() {
        let scratch =
            std::env::temp_dir().join(format!("concat-proxy-second-writer-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let dir = scratch.join("cache").join("proxy");
        std::fs::create_dir_all(&dir).expect("a proxy folder");
        let target = dir.join("6666666666666666.mp4");
        let partial = target.with_extension("part.mp4");
        let _first = take_writing_lock(&partial).expect("the first writer's lock");
        let refused = write(
            "whatever the source is, the lock is asked first",
            &target,
            (320, 180),
            None,
            ProxyCodec::H264,
        )
        .expect_err("one proxy, one writer");
        assert_eq!(
            refused, "another Concat process is writing this proxy",
            "the refusal names what happened, not what the source did"
        );
        assert!(
            !partial.is_file(),
            "the refused writer left no partial under the holder's"
        );
        drop(_first);
        assert!(
            lock_beside(&partial).is_file(),
            "the stable sentinel is not unlinked when the writer finishes"
        );
        let _second = take_writing_lock(&partial)
            .expect("the next writer locks the same persistent sentinel");
        drop(_second);
        assert!(lock_beside(&partial).is_file());
        let _ = std::fs::remove_dir_all(&scratch);
    }
}
