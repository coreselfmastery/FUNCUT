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
//! size and modification time, so a file replaced on disk gets a fresh
//! one and the old copy is swept with the rest of the cache.
//!
//! Writing one is a turn two processes take without meeting: the
//! partial is this writer's own - named after its process - and the
//! target is claimed with an operating-system lock for the length of
//! the write, so a second window on the same machine leaves the copy to
//! the first rather than transcode it twice into the same name. A
//! writer that dies lets go of its claim with its handle, and the
//! partial it leaves is swept with the next sweep: an unlocked partial
//! is an orphan, whichever process wrote it.
//!
//! Which codec the copy is written in is Settings' to say, as
//! [`ProxyCodec`]: H.264, the small file, or MJPEG, whose every frame is
//! a keyframe of its own and which nothing has to seek within - the
//! scrubbiest there is, at several times the file.

use std::collections::HashSet;
use std::fs::{File, OpenOptions, TryLockError};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

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

/// Where the proxy of `media` lives under `project`: named by the file's
/// path, size and modification time, and by the range it is read as,
/// since the copy is written from the corrected picture and a correction
/// changed is a different copy - and by whether the file is HDR, whose
/// proxy keeps its signal, so the tone-mapped copies earlier builds wrote
/// of one are written again. A proxy in the codec other than H.264 - the
/// one every proxy was before the codec was a choice - is named for that
/// too, so a switch of codec in Settings writes the choice's copy and
/// not the old one renamed. None for a file that cannot be stat'ed,
/// which cannot be read either.
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

/// How long a partial is left alone on a folder whose locks do not work:
/// where a claim cannot be taken, age is the only thing that tells an
/// orphan from a slow write, and an hour is the longest a copy of a
/// quarter-size transcode has ever taken on the machines this is for.
const PART_LIFETIME: Duration = Duration::from_secs(60 * 60);

/// Sees to it that `media` - `width` by `height`, read as `range` - has
/// a proxy when it is worth one: an existing copy is adopted into the
/// reader pool at once; otherwise one is queued to be written on the
/// proxy lane and adopted when done. Asking again while it is being
/// written is free, here or in another process: a copy another process
/// is writing is left to it, and the next ask finds it done. Returns
/// whether the file has, or will have, a proxy.
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
    // Ours is not writing it; whether another process is. The turn is
    // taken before the job is queued and held for the length of the
    // write, so two processes never transcode one file twice into one
    // name - and a process that dies mid-write lets its turn go with
    // its handle, leaving the next ask to write the copy again.
    let turn = take_turn(&target);
    if matches!(turn, Turn::Theirs) {
        writing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&target);
        return true;
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
        // The turn goes with `turn`'s file when this closure ends.
        drop(turn);
    });
    true
}

/// The proxies being written now, by target, so one is never queued
/// twice and a sweep leaves it for its writer to finish.
fn writing() -> &'static Mutex<HashSet<PathBuf>> {
    static WRITING: OnceLock<Mutex<HashSet<PathBuf>>> = OnceLock::new();
    WRITING.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Removes the proxies under `project` that `keep` does not name: the
/// copies of media no longer in the project, or read as a range they no
/// longer are. Nothing else sweeps this folder, and a proxy is a
/// quarter-size copy of every file over HD the project ever held
/// (audit 2026-09-23, #15). One being written is left for its writer,
/// here or in another process: a partial is an orphan exactly when
/// nothing holds its target's turn, and age is the judge only where a
/// claim cannot be taken at all. Returns how many files went.
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
        // A proxy in mid-write sits under a `.part.mp4` name beside its
        // target - the muxer needs the extension to know the container.
        // Its writer holds the target's turn, in this process or
        // another; a partial nothing holds a turn for is an orphan a
        // dead writer left, and goes with the rest of the cache.
        let part = path
            .file_stem()
            .is_some_and(|stem| stem.to_string_lossy().ends_with(".part"));
        if part {
            if partial_is_orphan(&path) && std::fs::remove_file(&path).is_ok() {
                removed += 1;
            }
            continue;
        }
        if !part
            && path.extension().is_some_and(|extension| extension == "mp4")
            && !keep.contains(&path)
            && !writing.contains(&path)
            && std::fs::remove_file(&path).is_ok()
        {
            removed += 1;
        }
    }
    removed
}

/// Whether a `.part.mp4` beside a target is an orphan: nothing holds the
/// turn on the target it belongs to, in this process or any other. On a
/// folder whose locks do not work, age decides instead - a partial older
/// than [`PART_LIFETIME`] is nobody's work in progress.
fn partial_is_orphan(partial: &Path) -> bool {
    // The target's name is the partial's first dot-separated word: the
    // hash stem the target is filed under, with this writer's process
    // and counter after it and `.part` before the extension.
    let stem = partial
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('.').next())
        .unwrap_or("");
    let target = partial.with_file_name(format!("{stem}.mp4"));
    if target_turn_is_held(&target) {
        return false;
    }
    if locks_work() {
        return true;
    }
    // No claim could be probed: age is what is left.
    std::fs::metadata(partial)
        .and_then(|meta| meta.modified())
        .is_ok_and(|written| written.elapsed().unwrap_or_default() > PART_LIFETIME)
}

/// What asking for a proxy target's turn found.
enum Turn {
    /// Ours to write: the handle holds the turn, and letting it go - by
    /// dropping the value - is what gives the turn back.
    Ours { _held: File },
    /// Another process is writing this copy now.
    Theirs,
    /// The runtime folder would not take the file: nobody is turned
    /// away, and nobody is held to one at a time.
    Unmanaged,
}

/// Asks for the turn to write `target`, across processes: an exclusive
/// lock on a file in the session's runtime folder, named after the
/// target as the filesystem spells it. The lock is the operating
/// system's, not the file's - it lives exactly as long as the handle -
/// so a writer that crashes or is killed gives the turn back, and the
/// next ask writes the copy again.
fn take_turn(target: &Path) -> Turn {
    let Some(path) = turn_path(target) else {
        return Turn::Unmanaged;
    };
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    else {
        return Turn::Unmanaged;
    };
    match file.try_lock() {
        Ok(()) => Turn::Ours { _held: file },
        Err(TryLockError::WouldBlock) => Turn::Theirs,
        Err(TryLockError::Error(_)) => Turn::Unmanaged,
    }
}

/// Whether some process holds the turn on `target` right now: a writer
/// mid-transcode, in this process or another.
fn target_turn_is_held(target: &Path) -> bool {
    let Some(path) = turn_path(target) else {
        return false;
    };
    let Ok(file) = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
    else {
        return false;
    };
    matches!(file.try_lock(), Err(TryLockError::WouldBlock))
}

/// Whether claims can be probed at all: the runtime folder takes the
/// files. Where it does not, a partial's fate is decided by its age.
fn locks_work() -> bool {
    static WORK: OnceLock<bool> = OnceLock::new();
    *WORK.get_or_init(|| {
        let probe = std::env::temp_dir().join(format!("concat-proxy-probe-{}", std::process::id()));
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&probe)
            .and_then(|file| {
                file.try_lock().map_err(|error| match error {
                    TryLockError::Error(error) => error,
                    TryLockError::WouldBlock => std::io::Error::other("the probe lock is held"),
                })
            })
            .is_ok()
    })
}

/// The claim file for `target`, in the session's runtime folder - or the
/// system's temporary one where there is no session - named after the
/// target as the filesystem spells it.
fn turn_path(target: &Path) -> Option<PathBuf> {
    let folder = std::env::var_os("XDG_RUNTIME_DIR").map_or_else(std::env::temp_dir, PathBuf::from);
    let name = target.file_name()?;
    let spelled = target
        .parent()
        .and_then(|parent| std::fs::canonicalize(parent).ok())
        .map(|parent| parent.join(name))
        .unwrap_or_else(|| target.to_path_buf());
    Some(folder.join(format!(
        "concat-proxy-{:016x}.lock",
        fnv1a(&spelled.to_string_lossy())
    )))
}

/// The partial a proxy is written under: beside its target, named after
/// it with this process and a counter of this call between, and `.part`
/// before the extension the muxer needs - so two writers, in one process
/// or two, never share the file one is still encoding into.
fn partial_for(target: &Path) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let stem = target
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or("proxy");
    let mut taken = 0;
    loop {
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        let candidate =
            target.with_file_name(format!("{stem}.{}.{}.part.mp4", std::process::id(), nonce));
        // The encoder makes the file itself, so this is a courtesy and
        // not a lock: a name already taken is tried again under the next
        // counter, and after sixteen in a row - which cannot happen - the
        // last is returned and is still this writer's own.
        if !candidate.exists() || taken >= 16 {
            return candidate;
        }
        taken += 1;
    }
}

/// FNV-1a, sixty-four bits, over the bytes of `text`: a short, stable
/// name for a path, so a claim file made by one build is found by the
/// next.
fn fnv1a(text: &str) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in text.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

/// Writes the proxy of `source`, read as `range`, at `size` to `target`
/// in `codec` (an HDR source's copy keeps its signal and is HEVC
/// whatever the codec says, since MJPEG carries neither): two threads,
/// in software, into a temporary name of this writer's own that is moved
/// into place only when whole, so a copy that is there is a copy that is
/// complete - and a copy another process is writing beside is never
/// written through, whatever the folder's locks make of the two. The
/// copy is tagged with the levels it holds and converted to match - video
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
    let info = concat_media::probe(source).map_err(|error| error.to_string())?;
    let (rate, signal) = info
        .video
        .as_ref()
        .map(|video| (video.frame_rate, video.signal))
        .ok_or_else(|| "no video stream".to_owned())?;
    let hdr = matches!(signal, Signal::Hlg | Signal::Pq);
    if let Some(folder) = target.parent() {
        std::fs::create_dir_all(folder)
            .map_err(|error| format!("{}: {error}", folder.display()))?;
    }
    // Written beside the target under a name the muxer still knows the
    // format of, this writer's own, and moved into place only when
    // whole.
    let partial = partial_for(target);
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

    #[test]
    fn a_sweep_keeps_what_is_named_and_what_is_being_written() {
        if !locks_work() {
            eprintln!("this folder's locks do not work; skipped");
            return;
        }
        let scratch =
            std::env::temp_dir().join(format!("concat-proxy-sweep-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&scratch);
        let dir = scratch.join("cache").join("proxy");
        std::fs::create_dir_all(&dir).expect("a proxy folder");
        let kept = dir.join("1111111111111111.mp4");
        let stale = dir.join("2222222222222222.mp4");
        let busy = dir.join("3333333333333333.mp4");
        // A live writer's partial: its writer holds the target's turn,
        // here or in another process, and the sweep leaves it alone.
        let live = dir.join("4444444444444444.mp4");
        let live_partial = dir.join(format!(
            "4444444444444444.{}.1.part.mp4",
            std::process::id()
        ));
        let held = take_turn(&live);
        assert!(
            matches!(held, Turn::Ours { .. }),
            "the turn is ours to hold"
        );
        // An orphan: a writer that died mid-write, its turn gone with its
        // handle, its partial left behind for this sweep to take.
        let orphan = dir.join(format!(
            "5555555555555555.{}.1.part.mp4",
            std::process::id()
        ));
        for file in [&kept, &stale, &busy, &live_partial, &orphan] {
            std::fs::write(file, b"x").expect("writes");
        }
        writing()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(busy.clone());
        let keep: HashSet<PathBuf> = [kept.clone()].into_iter().collect();
        assert_eq!(
            sweep(&scratch, &keep),
            2,
            "the stale copy and the orphan go"
        );
        assert!(kept.is_file() && busy.is_file() && live_partial.is_file());
        assert!(!stale.is_file() && !orphan.is_file());
        writing()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(&busy);
        drop(held);
        let _ = std::fs::remove_dir_all(&scratch);
    }

    /// A partial's name is its writer's own: the target's stem, this
    /// process, a counter of the call, and `.part` before the extension -
    /// so two writers never share the file one is still encoding into.
    #[test]
    fn a_partials_name_is_its_writers_own() {
        let target = Path::new("/cache/proxy/0123456789abcdef.mp4");
        let first = partial_for(target);
        let second = partial_for(target);
        assert_ne!(first, second, "two writes never share a partial");
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("0123456789abcdef."), "{name}");
        assert!(
            name.contains(&format!(".{}.", std::process::id())),
            "{name} names its writer"
        );
        assert!(name.ends_with(".part.mp4"), "{name}");
        assert_eq!(
            name.split('.').count(),
            5,
            "{name} is stem, writer, counter, part, mp4"
        );
    }

    /// A copy another process is writing is left to it: the ask says the
    /// file will have its proxy, nothing is written here, and the next
    /// ask - the turn free again - writes it.
    #[test]
    fn a_copy_another_process_is_writing_is_left_to_it() {
        use concat_core::frame::Frame;
        use concat_core::time::FrameRate;
        if !locks_work() {
            eprintln!("this folder's locks do not work; skipped");
            return;
        }
        let dir = std::env::temp_dir().join(format!("concat-proxy-turn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
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
            let frame = Frame::black(2560, 1440);
            for _ in 0..3 {
                encoder.write_frame(&frame).expect("writes");
            }
            encoder.finish().expect("finishes");
        }
        let project = dir.join("project");
        let source = media.to_string_lossy().into_owned();
        let target = path_for(&project, &source, None, false, ProxyCodec::H264).expect("a path");
        // Another writer holds the turn - as a second window would.
        let held = take_turn(&target);
        assert!(matches!(held, Turn::Ours { .. }));
        assert!(
            ensure(&project, &source, 2560, 1440, None, false, ProxyCodec::H264),
            "worth a proxy"
        );
        crate::scheduler().drain();
        assert!(!target.is_file(), "the copy was left to its writer");
        assert!(
            !writing()
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .contains(&target),
            "nothing of ours is queued or held for it"
        );
        // The writer finished and let go: the next ask writes the copy.
        drop(held);
        assert!(ensure(
            &project,
            &source,
            2560,
            1440,
            None,
            false,
            ProxyCodec::H264
        ));
        crate::scheduler().drain();
        assert!(
            target.is_file(),
            "the copy was written once the turn was free"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
