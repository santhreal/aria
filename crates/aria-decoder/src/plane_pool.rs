//! Reusable CPU plane buffers.
//!
//! A 4K NV12 frame holds 12 MiB of planes. Freeing those buffers on every
//! frame and allocating them again unmaps and faults the same pages per
//! frame, so CPU decoders take plane buffers from a process-wide pool and the
//! holders of decoded frames return them with [`recycle_frame_planes`].
//!
//! The pool keeps buffers of [`MIN_BYTES`] to [`MAX_BYTES`], at most
//! [`MAX_BUFFERS`] of them and at most [`MAX_BYTES`] in total, and evicts the
//! oldest first. A take matches a returned buffer of the same length, most
//! recently returned first, so one stream's buffers serve its next frames.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};

use aria_core::{FrameData, VideoFrame};

/// Smallest buffer the pool keeps (64 KiB). Smaller buffers come from the
/// allocator's arenas, which reuse their pages without unmapping them.
const MIN_BYTES: usize = 64 << 10;

/// Most buffers the pool keeps.
const MAX_BUFFERS: usize = 64;

/// Most bytes the pool keeps (128 MiB, ten 4K NV12 frames).
const MAX_BYTES: usize = 128 << 20;

static POOL: Mutex<PlanePool> = Mutex::new(PlanePool::new());

/// Returned buffers, oldest first, and the sum of their capacities.
struct PlanePool {
    buffers: VecDeque<Vec<u8>>,
    bytes: usize,
}

impl PlanePool {
    const fn new() -> Self {
        Self {
            buffers: VecDeque::new(),
            bytes: 0,
        }
    }

    /// Whether the pool keeps a buffer of `capacity` bytes.
    const fn keeps(capacity: usize) -> bool {
        capacity >= MIN_BYTES && capacity <= MAX_BYTES
    }

    /// The most recently returned buffer of length `len`, if any.
    fn take(&mut self, len: usize) -> Option<Vec<u8>> {
        let index = self.buffers.iter().rposition(|buf| buf.len() == len)?;
        let buf = self.buffers.remove(index)?;
        self.bytes -= buf.capacity();
        Some(buf)
    }

    /// Keep `buf`, whose capacity [`Self::keeps`], evicting the oldest
    /// buffers past the count or byte bound. Returns the evicted buffers so
    /// the caller frees them after releasing the lock.
    fn put(&mut self, buf: Vec<u8>) -> Vec<Vec<u8>> {
        if self.buffers.capacity() < MAX_BUFFERS {
            self.buffers.reserve_exact(MAX_BUFFERS - self.buffers.len());
        }
        let mut evicted = Vec::new();
        while self.buffers.len() >= MAX_BUFFERS || self.bytes + buf.capacity() > MAX_BYTES {
            let Some(oldest) = self.buffers.pop_front() else {
                break;
            };
            self.bytes -= oldest.capacity();
            evicted.push(oldest);
        }
        self.bytes += buf.capacity();
        self.buffers.push_back(buf);
        evicted
    }
}

fn pool() -> MutexGuard<'static, PlanePool> {
    POOL.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A plane buffer of length `len`: a returned buffer of that length when the
/// pool holds one, else a new zeroed buffer. The contents of a returned
/// buffer are unspecified, so the caller writes every byte before reading.
pub fn take_plane_vec(len: usize) -> Vec<u8> {
    let pooled = if PlanePool::keeps(len) {
        pool().take(len)
    } else {
        None
    };
    pooled.unwrap_or_else(|| vec![0u8; len])
}

/// Copy `src` into a plane buffer from the pool.
pub fn copy_plane_bytes(src: &[u8]) -> Vec<u8> {
    let mut out = take_plane_vec(src.len());
    out.copy_from_slice(src);
    out
}

/// Return `buf` to the pool, keeping its length, or free it when the pool
/// does not keep buffers of its size.
pub(crate) fn recycle_plane_vec(buf: Vec<u8>) {
    if !PlanePool::keeps(buf.capacity()) {
        return;
    }
    let evicted = pool().put(buf);
    drop(evicted);
}

/// Return the CPU plane buffers of `frame` to the pool, leaving its planes
/// empty. A GPU frame is unchanged.
pub fn recycle_frame_planes(frame: &mut VideoFrame) {
    if let FrameData::Cpu(cpu) = &mut frame.data {
        for plane in &mut cpu.planes {
            recycle_plane_vec(std::mem::take(&mut plane.data));
        }
    }
}

/// Pool bounds and matching, on a local pool so no other test's buffers
/// interleave with these. Not covered: lock contention between decode threads.
#[cfg(test)]
mod tests {
    use super::*;

    fn filled(len: usize, fill: u8) -> Vec<u8> {
        vec![fill; len]
    }

    #[test]
    fn the_pool_keeps_only_buffers_within_its_size_bounds() {
        assert!(!PlanePool::keeps(MIN_BYTES - 1));
        assert!(PlanePool::keeps(MIN_BYTES));
        assert!(PlanePool::keeps(MAX_BYTES));
        assert!(!PlanePool::keeps(MAX_BYTES + 1));
    }

    #[test]
    fn a_take_returns_the_latest_buffer_of_the_same_length() {
        let mut pool = PlanePool::new();
        assert!(
            pool.take(MIN_BYTES).is_none(),
            "an empty pool holds nothing"
        );
        let older = filled(MIN_BYTES, 1);
        let newer = filled(MIN_BYTES, 2);
        let newer_ptr = newer.as_ptr();
        assert!(pool.put(older).is_empty());
        assert!(pool.put(newer).is_empty());
        assert!(pool.put(filled(MIN_BYTES * 2, 3)).is_empty());
        assert!(
            pool.take(MIN_BYTES + 1).is_none(),
            "a buffer of another length must not serve a take"
        );
        let taken = pool.take(MIN_BYTES).expect("pooled buffer");
        assert_eq!(
            taken.as_ptr(),
            newer_ptr,
            "the latest buffer of the length comes first, not a larger one"
        );
        assert_eq!(taken.len(), MIN_BYTES);
        assert!(taken.iter().all(|&byte| byte == 2));
        assert_eq!(pool.bytes, MIN_BYTES * 3, "two buffers remain counted");
        assert!(pool.take(MIN_BYTES).is_some_and(|buf| buf[0] == 1));
        assert!(pool.take(MIN_BYTES).is_none());
        assert!(pool.take(MIN_BYTES * 2).is_some_and(|buf| buf[0] == 3));
        assert_eq!(pool.bytes, 0);
    }

    #[test]
    fn the_count_bound_evicts_the_oldest_buffers() {
        let mut pool = PlanePool::new();
        let mut evicted = Vec::new();
        for index in 0..MAX_BUFFERS + 3 {
            evicted.extend(pool.put(filled(MIN_BYTES, index as u8)));
        }
        assert_eq!(pool.buffers.len(), MAX_BUFFERS);
        assert_eq!(pool.bytes, MAX_BUFFERS * MIN_BYTES);
        let evicted: Vec<u8> = evicted.iter().map(|buf| buf[0]).collect();
        assert_eq!(evicted, [0, 1, 2], "the oldest buffers leave first");
        assert_eq!(pool.buffers.front().map(|buf| buf[0]), Some(3));
    }

    #[test]
    fn the_byte_bound_evicts_the_oldest_buffers() {
        let mut pool = PlanePool::new();
        let len = MAX_BYTES / 4 + 1;
        let mut evicted = Vec::new();
        for index in 0..6u8 {
            // Reserved, never written: the bound counts capacity, so these
            // stand in for 32 MiB planes without faulting their pages.
            let mut buf = Vec::with_capacity(len);
            buf.push(index);
            evicted.extend(pool.put(buf).iter().map(|buf| buf[0]));
            assert!(pool.bytes <= MAX_BYTES, "{} bytes pooled", pool.bytes);
            let held: usize = pool.buffers.iter().map(Vec::capacity).sum();
            assert_eq!(pool.bytes, held, "the byte count tracks the held buffers");
        }
        assert_eq!(pool.buffers.len(), 3, "four buffers exceed the byte bound");
        assert_eq!(evicted, [0, 1, 2], "the oldest buffers leave first");
    }

    #[test]
    fn recycled_frame_planes_serve_the_next_take() {
        const LEN: usize = MIN_BYTES * 3 + 11;
        let mut frame = VideoFrame::new(
            aria_core::RationalTime::new(0, aria_core::Rational::FPS_30),
            aria_core::PixelFormat::Nv12,
            aria_core::ColorSpace::BT709,
            (2, 2),
            aria_core::Rect::from_size(2, 2),
            aria_core::Rotation::None,
            FrameData::Cpu(aria_core::CpuImage::new(vec![aria_core::Plane::new(
                filled(LEN, 0x5a),
                2,
                2,
            )])),
        );
        recycle_frame_planes(&mut frame);
        let FrameData::Cpu(cpu) = &frame.data else {
            unreachable!("built as a CPU frame");
        };
        assert!(cpu.planes[0].data.is_empty(), "the plane moved to the pool");
        let taken = take_plane_vec(LEN);
        assert!(
            taken.iter().all(|&byte| byte == 0x5a),
            "a fresh zeroed buffer means the plane was freed, not pooled"
        );
        let copied = copy_plane_bytes(&filled(LEN, 0x3c));
        assert!(copied.len() == LEN && copied.iter().all(|&byte| byte == 0x3c));
    }
}
