//! The GPU device a [`Renderer`](super::Renderer) draws on, and the compositor
//! pipelines built against it.
//!
//! A deferred renderer brings the device up on its first frame. Bringing up a
//! device is the dearest thing a renderer does: half a second of driver work on
//! a desktop GPU, most of it under a lock the driver holds for the whole
//! process. A session that edits, saves and closes without drawing a frame
//! pays none of it.

use aria_compositor::{Compositor, CompositorError, GpuContext, PassStats};

use crate::error::RenderError;

/// The device and its compositor, once brought up.
pub(super) struct Bound {
    gpu: GpuContext,
    compositor: Compositor,
}

impl Bound {
    fn on(gpu: GpuContext) -> Self {
        let compositor = Compositor::new(&gpu);
        Self { gpu, compositor }
    }
}

/// `None` until the first frame of a deferred renderer; then the device, or
/// the error bringing it up failed with. A failed bring-up is kept, so every
/// later frame fails with it at once instead of asking the driver again.
pub(super) struct Device(Option<Result<Bound, CompositorError>>);

impl Device {
    /// A device the first frame brings up.
    pub(super) const fn deferred() -> Self {
        Self(None)
    }

    /// The device `gpu`, with its pipelines built now.
    pub(super) fn on(gpu: GpuContext) -> Self {
        Self(Some(Ok(Bound::on(gpu))))
    }

    /// The device and its compositor, bringing up the process's headless
    /// device first when there is none yet.
    pub(super) fn ready(&mut self) -> Result<(&GpuContext, &mut Compositor), RenderError> {
        let device = self
            .0
            .get_or_insert_with(|| GpuContext::new_headless_blocking().map(Bound::on));
        match device {
            Ok(Bound { gpu, compositor }) => Ok((&*gpu, compositor)),
            Err(err) => Err(RenderError::Compositor(again(err))),
        }
    }

    /// The device, or `None` while it is not brought up.
    pub(super) fn gpu(&self) -> Option<&GpuContext> {
        match &self.0 {
            Some(Ok(bound)) => Some(&bound.gpu),
            _ => None,
        }
    }

    /// The compositor, or `None` while the device is not brought up.
    pub(super) fn compositor(&self) -> Option<&Compositor> {
        match &self.0 {
            Some(Ok(bound)) => Some(&bound.compositor),
            _ => None,
        }
    }

    /// Draw on `gpu` from now on, rebuilding the pipelines on it.
    pub(super) fn set(&mut self, gpu: GpuContext) {
        match &mut self.0 {
            Some(Ok(bound)) => {
                bound.compositor.reset_gpu(&gpu);
                bound.gpu = gpu;
            }
            _ => *self = Self::on(gpu),
        }
    }

    /// GPU-to-CPU readbacks submitted so far; none before the device is up.
    pub(super) const fn readback_count(&self) -> u64 {
        match &self.0 {
            Some(Ok(bound)) => bound.compositor.readback_count(),
            _ => 0,
        }
    }

    /// What the effect passes of the last composite cost; nothing before the
    /// device is up.
    pub(super) fn pass_stats(&self) -> PassStats {
        self.compositor()
            .map(Compositor::pass_stats)
            .unwrap_or_default()
    }
}

/// The bring-up error again, for the next frame that asks. Bringing up a
/// device fails with no adapter or with a refused device and nothing else.
fn again(err: &CompositorError) -> CompositorError {
    match err {
        CompositorError::RequestDevice(why) => CompositorError::RequestDevice(why.clone()),
        CompositorError::NoAdapter(why) => CompositorError::NoAdapter(why.clone()),
        other => CompositorError::NoAdapter(other.to_string()),
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
