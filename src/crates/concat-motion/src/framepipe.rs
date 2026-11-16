// SPDX-License-Identifier: AGPL-3.0-or-later
// SPDX-FileCopyrightText: 2026 Jareer Abdullah and Concat contributors

//! The shared-memory frame pipe: frames as bytes between two ends that
//! agree on a protocol, without a PNG round trip between them.
//!
//! The layout is one page-aligned header followed by `capacity` frame
//! slots, RGBA as the engines bake:
//!
//! ```text
//! offset 0:  magic "CF10", width, height, stride, capacity,
//!            sequence (atomic: bumped per frame published)
//! offset 64: slot 0 .. slot capacity-1, each stride*height bytes
//! ```
//!
//! A writer renders into slot `sequence % capacity`, then publishes by
//! storing `sequence + 1` with release; a reader loads the sequence with
//! acquire and reads the slot it names. Two ends that do not agree simply
//! never see each other: the segment is named per bake (`shm_open`, so any
//! process on the machine that knows the name can attach), and the same
//! bytes live in a `memfd` when both ends are one process and the fd can
//! be handed over directly - the editor's preview and its engines, one
//! day. The crate carries no unsafe of its own; this module is the one
//! exception the Cargo.toml comment promises, and it is the whole of it.

use std::ffi::CString;
use std::sync::atomic::{AtomicU32, Ordering};

use rustix::fs::Mode;
use rustix::mm::{MapFlags, ProtFlags};

/// The protocol's magic: "CF10", the concat frame pipe, layout one.
const MAGIC: u32 = u32::from_ne_bytes(*b"CF10");

/// The header every segment starts with. Page-aligned by the mapping; the
/// frames begin at offset 64 whatever the header's own size, so a slot's
/// place never depends on the struct's padding.
const FRAMES_AT: usize = 64;

/// One shared-memory frame pipe.
pub struct FramePipe {
    name: Option<String>,
    // Held, never read: the mapping lives as long as the fd does, and the
    // fd closes when the pipe drops.
    #[allow(dead_code)]
    fd: rustix::fd::OwnedFd,
    ptr: *mut u8,
    len: usize,
}

// Safety: the mapping is shared by design - that is what it is for - and
// the pointer is only handed out as slices bounded by the header the two
// ends already agreed on. Send + Sync: rustix fd is Send+Sync; the atomics
// are atomic; the slices are guarded by the protocol.
unsafe impl Send for FramePipe {}
unsafe impl Sync for FramePipe {}

impl Drop for FramePipe {
    fn drop(&mut self) {
        // Safety: the mapping was created by `mmap` in `create`/`open` with
        // exactly this length, and nothing else has unmapped it: the pipe
        // owns it, and this is the only unmap.
        unsafe {
            let _ = rustix::mm::munmap(self.ptr as *mut _, self.len);
        }
    }
}

impl FramePipe {
    /// Create a named segment a second process can attach to by name: a
    /// POSIX shm object, sized for `capacity` frames of `width` x `height`
    /// RGBA, owned by the caller until `unlink` retires the name. Creating
    /// over an existing name is refused, as a stale segment from a crashed
    /// run must not be read as this one's frames.
    pub fn create(name: &str, width: u32, height: u32, capacity: u32) -> Result<FramePipe, String> {
        if !name.starts_with('/') || name.len() < 2 {
            return Err(
                "a shm name is a path: `/concat-motion-…`, leading slash, at least one character"
                    .into(),
            );
        }
        if width == 0 || height == 0 || capacity == 0 {
            return Err("a frame pipe carries at least one frame of at least one pixel".into());
        }
        let stride = width.saturating_mul(4) as usize;
        let frame = stride.saturating_mul(height as usize);
        let len = FRAMES_AT + frame.saturating_mul(capacity as usize);
        let c_name = CString::new(name).map_err(|_| "a shm name carries no NUL")?;
        let fd = rustix::shm::open(
            &c_name,
            rustix::shm::OFlags::CREATE | rustix::shm::OFlags::EXCL | rustix::shm::OFlags::RDWR,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|source| format!("shm_open {name:?}: {source}"))?;
        rustix::fs::ftruncate(&fd, len as u64)
            .map_err(|source| format!("ftruncate {name:?}: {source}"))?;
        let ptr = map(&fd, len)?;
        // Safety: the mapping is exactly `len` bytes and writable; the
        // header sits inside it, page-aligned by the mapping.
        unsafe {
            let header = &mut *header_ptr(ptr);
            header.magic = MAGIC;
            header.width = width;
            header.height = height;
            header.stride = width.saturating_mul(4);
            header.capacity = capacity;
            header.sequence.store(0, Ordering::Release);
        }
        Ok(FramePipe {
            name: Some(name.to_string()),
            fd,
            ptr,
            len,
        })
    }

    /// Attach to a named segment another end created: same bytes, same
    /// header, no initialization. A segment that is not speaking the
    /// protocol is refused rather than guessed at.
    pub fn open(name: &str) -> Result<FramePipe, String> {
        if !name.starts_with('/') || name.len() < 2 {
            return Err(
                "a shm name is a path: `/concat-motion-…`, leading slash, at least one character"
                    .into(),
            );
        }
        let c_name = CString::new(name).map_err(|_| "a shm name carries no NUL")?;
        let fd = rustix::shm::open(&c_name, rustix::shm::OFlags::RDWR, Mode::RUSR | Mode::WUSR)
            .map_err(|source| format!("shm_open {name:?}: {source}"))?;
        let stat = rustix::fs::fstat(&fd).map_err(|source| format!("fstat {name:?}: {source}"))?;
        let len = usize::try_from(stat.st_size).unwrap_or(0);
        if len < FRAMES_AT {
            return Err(format!(
                "shm segment {name:?} is {len} bytes, too small for the header"
            ));
        }
        let ptr = map(&fd, len)?;
        // Safety: the magic is the two ends' one handshake. A reader that
        // finds any other bytes in the header's first field is looking at a
        // segment that is not speaking this protocol.
        unsafe {
            if (*header_ptr(ptr)).magic != MAGIC {
                return Err(format!(
                    "shm segment {name:?} does not carry the frame pipe's magic"
                ));
            }
        }
        Ok(FramePipe {
            name: Some(name.to_string()),
            fd,
            ptr,
            len,
        })
    }

    /// A `memfd` segment, unnamed: for the two ends that are one process,
    /// where the fd is handed over directly and no name is needed.
    pub fn memfd(width: u32, height: u32, capacity: u32) -> Result<FramePipe, String> {
        let stride = width.saturating_mul(4) as usize;
        let frame = stride.saturating_mul(height as usize);
        let len = FRAMES_AT + frame.saturating_mul(capacity as usize);
        let fd =
            rustix::fs::memfd_create(c"concat-motion-frame-pipe", rustix::fs::MemfdFlags::CLOEXEC)
                .map_err(|source| format!("memfd_create: {source}"))?;
        rustix::fs::ftruncate(&fd, len as u64)
            .map_err(|source| format!("memfd ftruncate: {source}"))?;
        let ptr = map(&fd, len)?;
        let pipe = FramePipe {
            name: None,
            fd,
            ptr,
            len,
        };
        unsafe {
            let header = &mut *header_ptr(pipe.ptr);
            header.magic = MAGIC;
            header.width = width;
            header.height = height;
            header.stride = width.saturating_mul(4);
            header.capacity = capacity;
            header.sequence.store(0, Ordering::Release);
        }
        Ok(pipe)
    }

    /// The segment's name, when it has one a second end can attach by.
    pub fn name(&self) -> &str {
        self.name.as_deref().unwrap_or("")
    }

    /// Frames the pipe holds at once.
    pub fn capacity(&self) -> u32 {
        unsafe { (*header_ptr(self.ptr)).capacity }
    }

    /// The width every slot carries.
    pub fn width(&self) -> u32 {
        unsafe { (*header_ptr(self.ptr)).width }
    }

    /// The height every slot carries.
    pub fn height(&self) -> u32 {
        unsafe { (*header_ptr(self.ptr)).height }
    }

    /// How many frames have been published, whatever the capacity: the
    /// writer's own count of itself.
    pub fn sequence(&self) -> u32 {
        unsafe { (*header_ptr(self.ptr)).sequence.load(Ordering::Acquire) }
    }

    /// The slot `index` names, for writing: the bytes a renderer renders
    /// into. `None` when the index is past the capacity.
    pub fn slot_mut(&mut self, index: u32) -> Option<&mut [u8]> {
        // Safety: `slot_ptr` bounds the pointer against the header the two
        // ends agreed on, and the mapping is writable.
        unsafe {
            self.slot_ptr(index)
                .map(|(ptr, len)| std::slice::from_raw_parts_mut(ptr, len))
        }
    }

    /// The slot `index` names, for reading.
    pub fn slot(&self, index: u32) -> Option<&[u8]> {
        // Safety: as `slot_mut`, for reading.
        unsafe {
            self.slot_ptr(index)
                .map(|(ptr, len)| std::slice::from_raw_parts(ptr, len))
        }
    }

    /// Publish slot `index`: the acquire/release turnstile. A frame is
    /// bytes until this, and a reader after it is reading a whole one.
    pub fn publish(&mut self, index: u32) {
        unsafe {
            let header = &*header_ptr(self.ptr);
            let sequence = header.sequence.load(Ordering::Acquire);
            debug_assert_eq!(
                sequence % header.capacity,
                index % header.capacity,
                "publish names the slot the sequence points at"
            );
            header
                .sequence
                .store(sequence.wrapping_add(1), Ordering::Release);
        }
    }

    /// The latest published frame: its slot index and its bytes, `None`
    /// before anything was published.
    pub fn latest(&self) -> Option<(u32, &[u8])> {
        let sequence = self.sequence();
        if sequence == 0 {
            return None;
        }
        let index = (sequence - 1) % self.capacity().max(1);
        self.slot(index).map(|bytes| (index, bytes))
    }

    /// The slot's bounds, checked against the header.
    ///
    /// # Safety
    ///
    /// The mapping's length covers every slot the header names; the caller
    /// only receives a pointer inside it.
    unsafe fn slot_ptr(&self, index: u32) -> Option<(*mut u8, usize)> {
        // Safety: the mapping's length covers every slot the header names;
        // the header is inside it; the pointer returned stays inside it.
        unsafe {
            let header = &*header_ptr(self.ptr);
            if index >= header.capacity || header.capacity == 0 {
                return None;
            }
            let stride = header.stride as usize;
            let frame = stride.saturating_mul(header.height as usize);
            let at = FRAMES_AT + frame.saturating_mul(index as usize);
            let end = at.saturating_add(frame);
            if end > self.len {
                return None;
            }
            Some((self.ptr.add(at), frame))
        }
    }
}

/// Retire a segment's name: after this, attaching by name fails, and the
/// last end to drop its mapping frees the bytes.
pub fn unlink(name: &str) -> Result<(), String> {
    let c_name = CString::new(name).map_err(|_| "a shm name carries no NUL")?;
    rustix::shm::unlink(&c_name).map_err(|source| format!("shm_unlink {name:?}: {source}"))
}

/// The header pointer a mapping starts with.
///
/// # Safety
///
/// `ptr` must be a mapping of at least `FRAMES_AT` bytes.
unsafe fn header_ptr(ptr: *mut u8) -> *mut Header {
    ptr as *mut Header
}

fn map(fd: &rustix::fd::OwnedFd, len: usize) -> Result<*mut u8, String> {
    // Safety: the fd is an open shm or memfd whose length covers `len`; the
    // mapping is shared and read-write, the two things the protocol is.
    unsafe {
        let ptr = rustix::mm::mmap(
            std::ptr::null_mut(),
            len,
            ProtFlags::READ | ProtFlags::WRITE,
            MapFlags::SHARED,
            fd,
            0,
        )
        .map_err(|source| format!("mmap: {source}"))?;
        if ptr.is_null() {
            return Err("mmap returned null".into());
        }
        Ok(ptr as *mut u8)
    }
}

/// The pipe's header, as the two ends lay it out.
#[repr(C)]
struct Header {
    magic: u32,
    width: u32,
    height: u32,
    stride: u32,
    capacity: u32,
    sequence: AtomicU32,
    reserved: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name no other test in this run shares: a process-wide counter, so
    /// parallel tests never race one segment.
    fn test_name(tag: &str) -> String {
        static NAMED: AtomicU32 = AtomicU32::new(0);
        format!(
            "/concat-motion-test-{}-{}-{}",
            std::process::id(),
            tag,
            NAMED.fetch_add(1, Ordering::Relaxed)
        )
    }

    /// Two ends, one segment: the second attaches by name and reads the
    /// frame the first wrote, whole, through the turnstile.
    #[test]
    fn two_ends_share_one_segment_by_name() {
        let name = test_name("shared");
        let mut writer = FramePipe::create(&name, 4, 2, 2).expect("created");
        assert_eq!((writer.width(), writer.height()), (4, 2));
        assert_eq!(writer.capacity(), 2);
        // Nothing published yet: the latest is nothing.
        assert!(writer.latest().is_none());
        // Frame one: 4x2 RGBA, a solid frame with a corner byte.
        let slot = writer.slot_mut(0).expect("slot 0");
        slot.copy_from_slice(&[
            1, 2, 3, 4, 5, 6, 7, 8, //
            9, 10, 11, 12, 13, 14, 15, 16, //
            17, 18, 19, 20, 21, 22, 23, 24, //
            25, 26, 27, 28, 29, 30, 31, 32,
        ]);
        // The reader attaches before the publish and sees nothing.
        let reader = FramePipe::open(&name).expect("attached");
        assert!(reader.latest().is_none());
        writer.publish(0);
        // And after it, the whole frame.
        let (index, bytes) = reader.latest().expect("the first frame");
        assert_eq!(index, 0);
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[31], 32);
        assert_eq!(reader.sequence(), 1);
        // The second frame overwrites slot 1; the ring wraps at capacity.
        let slot = writer.slot_mut(1).expect("slot 1");
        slot.fill(0xEE);
        writer.publish(1);
        let (index, _) = reader.latest().expect("the second frame");
        assert_eq!(index, 1);
        assert_eq!(reader.sequence(), 2);
        // The writer's own slot 0 is still there: capacity holds both.
        assert_eq!(writer.slot(0).map(|bytes| bytes[31]), Some(32));
        unlink(&name).expect("unlinked");
        // A reader after the unlink cannot attach: the name is retired.
        assert!(FramePipe::open(&name).is_err());
    }

    /// A memfd needs no name and speaks the same protocol: the fd is the
    /// seam, for the two ends that are one process.
    #[test]
    fn a_memfd_speaks_the_same_protocol() {
        let mut pipe = FramePipe::memfd(2, 2, 2).expect("memfd");
        assert!(pipe.name().is_empty());
        pipe.slot_mut(0).expect("slot 0").fill(7);
        pipe.publish(0);
        let (index, bytes) = pipe.latest().expect("a frame");
        assert_eq!(index, 0);
        assert!(bytes.iter().all(|byte| *byte == 7));
    }

    /// A segment that is not speaking this protocol is refused, and so is
    /// a name a second create would shadow.
    #[test]
    fn the_wrong_bytes_and_the_taken_name_are_refused() {
        let name = test_name("refused");
        // A run that failed before its unlink must not poison this one.
        let _ = unlink(&name);
        // A plain shm segment with no header: not the protocol.
        let c_name = CString::new(name.as_str()).unwrap();
        let fd = rustix::shm::open(
            &c_name,
            rustix::shm::OFlags::CREATE | rustix::shm::OFlags::RDWR,
            Mode::RUSR | Mode::WUSR,
        )
        .expect("created");
        rustix::fs::ftruncate(&fd, 4096).expect("sized");
        drop(fd);
        assert!(FramePipe::open(&name).is_err());
        // Creating over the taken name is refused, not shadowed.
        assert!(FramePipe::create(&name, 4, 4, 2).is_err());
        unlink(&name).expect("unlinked");
    }
}
