//! The process's GPU device, and the renderers built on it.
//!
//! Bringing up a device is the dearest thing this program does: 176 ms of a
//! 222 ms start on the machine in `docs/performance.md`, plus a driver instance,
//! an adapter and a pipeline set held in video memory until the process exits.
//! It happens once, in the render plugin, and everything that renders afterwards
//! shares that device.
//!
//! Sharing the *device* is not sharing a renderer. An export, a proxy render, a
//! PNG sequence and an assistant screenshot each want their own decoders, their
//! own proxy substitutions and their own cancellation, and they run on job
//! threads while the preview renders on the UI thread — one renderer between
//! them would serialise exactly the work that was moved off the UI thread. Each
//! gets a renderer of its own; none of them gets a device of its own.
//!
//! `test_support::tests::one_module_brings_up_the_gpu` is what keeps a fifth
//! caller from quietly adding a fifth device.

use aria_compositor::GpuContext;
use aria_plugin::PluginContext;
use aria_render::Renderer;

/// Another handle on the device the render plugin brought up, or `None` on a
/// context where it did not mount.
///
/// Cheap enough to call per job: four reference-counted handles, no allocation
/// and no driver work. A context with no device is the normal case in a test
/// that mounts no plugins, and the caller falls back to bringing one up.
pub(crate) fn shared_device(ctx: &PluginContext) -> Option<GpuContext> {
    ctx.try_get::<GpuContext>().map(|gpu| gpu.share())
}

/// A renderer for background work, on `gpu` when there is one.
///
/// `what` names the work in the refusal, because a host with no GPU at all
/// refuses here and the operator needs to know which button they pressed:
/// "no GPU for the proxy" rather than a bare device error.
///
/// Take the device with [`shared_device`] on the calling thread and pass it in;
/// the renderer itself is built where it is used, which for a job is inside the
/// closure the job runs.
pub(crate) fn renderer_on(gpu: Option<GpuContext>, what: &str) -> Result<Renderer, String> {
    match gpu {
        Some(gpu) => Ok(Renderer::with_gpu(gpu)),
        None => Renderer::new_headless().map_err(|err| format!("no GPU for {what}: {err}")),
    }
}
