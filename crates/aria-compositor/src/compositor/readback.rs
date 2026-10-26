use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, sync_channel};

use aria_core::RgbaImage;

use crate::error::CompositorError;

use super::{FrameSink, ImageSink};

const READBACK_SLOTS: usize = 2;

struct ReadbackSlot {
    buffer: wgpu::Buffer,
    available: Arc<AtomicBool>,
}

/// Two reusable buffers let frame N map while frame N+1 renders.
pub(super) struct ReadbackPool {
    slots: [ReadbackSlot; READBACK_SLOTS],
    next: usize,
}

impl ReadbackPool {
    pub(super) fn new(device: &wgpu::Device, size: u64) -> Self {
        let slots = std::array::from_fn(|index| ReadbackSlot {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(if index == 0 {
                    "aria.readback.0"
                } else {
                    "aria.readback.1"
                }),
                size,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            available: Arc::new(AtomicBool::new(true)),
        });
        Self { slots, next: 0 }
    }

    pub(super) fn acquire(&mut self) -> Result<ReadbackLease, CompositorError> {
        for offset in 0..READBACK_SLOTS {
            let index = (self.next + offset) % READBACK_SLOTS;
            let slot = &self.slots[index];
            if slot
                .available
                .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.next = (index + 1) % READBACK_SLOTS;
                return Ok(ReadbackLease {
                    buffer: slot.buffer.clone(),
                    available: Arc::clone(&slot.available),
                });
            }
        }
        Err(CompositorError::ReadbackBusy)
    }
}

pub(super) struct ReadbackLease {
    pub(super) buffer: wgpu::Buffer,
    available: Arc<AtomicBool>,
}

/// A submitted frame whose GPU-to-CPU copy can overlap a later frame.
pub struct PendingReadback {
    lease: Option<ReadbackLease>,
    device: wgpu::Device,
    receiver: Receiver<Result<(), wgpu::BufferAsyncError>>,
    padded: u32,
    width: u32,
    height: u32,
}

impl std::fmt::Debug for PendingReadback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingReadback")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("padded", &self.padded)
            .finish_non_exhaustive()
    }
}

impl PendingReadback {
    pub(super) fn begin(
        device: &wgpu::Device,
        lease: ReadbackLease,
        padded: u32,
        width: u32,
        height: u32,
    ) -> Self {
        let slice = lease.buffer.slice(..);
        let (sender, receiver) = sync_channel(1);
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = sender.send(result);
        });
        Self {
            device: device.clone(),
            lease: Some(lease),
            receiver,
            padded,
            width,
            height,
        }
    }

    /// Wait for this frame, strip row padding, and write into caller storage.
    pub fn read_into(mut self, sink: &mut dyn FrameSink) -> Result<(), CompositorError> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|_| CompositorError::MapFailed)?;
        self.receiver
            .recv()
            .map_err(|_| CompositorError::MapFailed)?
            .map_err(|_| CompositorError::MapFailed)?;

        let lease = self
            .lease
            .take()
            .expect("pending readback retains its slot");
        let unpadded = (self.width * 4) as usize;
        {
            let mapped = lease.buffer.slice(..).get_mapped_range();
            let pixels = sink.pixels(self.width, self.height);
            debug_assert_eq!(pixels.len(), unpadded * self.height as usize);
            for row in 0..self.height as usize {
                let src = row * self.padded as usize;
                let dst = row * unpadded;
                pixels[dst..dst + unpadded].copy_from_slice(&mapped[src..src + unpadded]);
            }
        }
        lease.buffer.unmap();
        lease.available.store(true, Ordering::Release);
        Ok(())
    }

    /// Wait for this frame and return tightly packed RGBA pixels.
    pub fn into_image(self) -> Result<RgbaImage, CompositorError> {
        let mut sink = ImageSink::default();
        self.read_into(&mut sink)?;
        Ok(sink
            .into_image()
            .expect("a successful pending readback fills the image sink"))
    }
}

impl Drop for PendingReadback {
    fn drop(&mut self) {
        if let Some(lease) = self.lease.take() {
            let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
            lease.buffer.unmap();
            lease.available.store(true, Ordering::Release);
        }
    }
}

/// Maps `slice` for reading and blocks until the map completes.
///
/// The device is shared between threads, and wgpu runs a map callback in the
/// poll that collected it, after that poll releases the device. A poll on this
/// thread can therefore return while a poll on another thread still holds the
/// callback for this buffer, so the wait ends on the callback and not on the
/// poll. On an error the map may still be pending, so the caller drops the
/// buffer instead of mapping it again.
pub(crate) fn map_read(
    device: &wgpu::Device,
    slice: wgpu::BufferSlice<'_>,
) -> Result<(), CompositorError> {
    let (sender, receiver) = sync_channel(1);
    slice.map_async(wgpu::MapMode::Read, move |result| {
        let _ = sender.send(result);
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .map_err(|_| CompositorError::MapFailed)?;
    receiver
        .recv()
        .map_err(|_| CompositorError::MapFailed)?
        .map_err(|_| CompositorError::MapFailed)
}
