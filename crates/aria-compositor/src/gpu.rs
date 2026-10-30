//! WGPU device ownership.
//!
//! The compositor is headless by default ([`GpuContext::new_headless_blocking`])
//! — no window, no surface — which is what export and tests want. When the
//! desktop Slint UI comes online it creates the device once and shares it via
//! [`GpuContext::from_parts`] (Slint's `unstable-wgpu-29` hands back the same
//! `wgpu` 29 `Device`/`Queue`), so a preview frame can stay on the GPU from
//! decode through present with no readback.

use crate::error::CompositorError;

/// Long-lived WGPU resources shared by the compositor (and, later, the UI).
pub struct GpuContext {
    pub instance: wgpu::Instance,
    pub adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub(crate) pipelines: std::sync::Arc<
        std::sync::OnceLock<std::sync::Arc<crate::compositor::pipelines::SharedLayerPipelines>>,
    >,
}

impl std::fmt::Debug for GpuContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GpuContext").finish_non_exhaustive()
    }
}

impl GpuContext {
    /// Create a fresh headless GPU context before the process has one.
    async fn fresh_headless() -> Result<Self, CompositorError> {
        // On Windows, prefer D3D12 explicitly: wgpu's adapter sort is
        // backend-agnostic (Vulkan registers first and would win), and the
        // zero-copy video import opens the decoder's shared D3D11 textures
        // via D3D12's `OpenSharedHandle`. D3D12 is present on effectively
        // every Windows 10+ machine (WARP included); fall through to the
        // full backend set if it somehow isn't.
        #[cfg(target_os = "windows")]
        if let Ok(gpu) = Self::new_headless_with(wgpu::Backends::DX12).await {
            return Ok(gpu);
        }
        // Try primary backends (Vulkan / Metal / DX12) before falling back to
        // GL/EGL, so hosts with a primary adapter do not dlopen and initialize
        // a second EGL/DRI driver stack inside `wgpu::Instance::new`. A primary
        // adapter that is a CPU rasterizer (lavapipe) is dropped: the full set
        // ranks a GL adapter on real hardware above it.
        if let Ok(gpu) = Self::new_headless_with(wgpu::Backends::PRIMARY).await
            && primary_suffices(gpu.adapter.get_info().device_type)
        {
            return Ok(gpu);
        }
        Self::new_headless_with(wgpu::Backends::all()).await
    }

    async fn new_headless_with(backends: wgpu::Backends) -> Result<Self, CompositorError> {
        let instance = new_instance(backends);

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                ..Default::default()
            })
            .await
            .map_err(|e| CompositorError::NoAdapter(e.to_string()))?;

        // Ask for exactly what the adapter offers rather than wgpu's defaults:
        // the pipelines are raster-only, and default limits assume compute
        // support that GLES 3.0 adapters (Android emulator) don't have.
        // NV12 (when offered) feeds the Windows zero-copy import path, which
        // binds shared decoder textures as native NV12 plane views.
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("aria-compositor"),
                required_features: adapter.features() & wgpu::Features::TEXTURE_FORMAT_NV12,
                required_limits: adapter.limits(),
                ..Default::default()
            })
            .await
            .map_err(|e| CompositorError::RequestDevice(e.to_string()))?;

        Ok(Self {
            instance,
            adapter,
            device,
            queue,
            pipelines: std::sync::Arc::new(std::sync::OnceLock::new()),
        })
    }

    /// A handle on the process's headless device, brought up on first use.
    ///
    /// Every caller that wants a device of its own gets a handle on one device
    /// instead. A second device renders identically and costs a driver instance,
    /// an adapter, a pipeline set held in video memory, and the bring-up itself,
    /// which is the dearest thing this program does. It also costs every other
    /// thread in the process: the NVIDIA driver serializes device and pipeline
    /// creation on one internal mutex, so eight threads each bringing up a
    /// device spend their time in that mutex rather than on the work they were
    /// spawned for. A suite that builds an engine per test paid a device per
    /// test for a picture none of them looked at.
    ///
    /// The GL backend is the exception. Its context is bound to the thread that
    /// created it, and a handle used from another thread fails with `BadAccess`
    /// instead of falling back, so a GL adapter is handed back unshared exactly
    /// as before.
    ///
    /// Blocking, and the bring-up happens under the lock: a caller that arrives
    /// while another is starting the device waits for it rather than starting a
    /// second one, which is also what the driver loader needs (see
    /// `new_instance`).
    pub fn new_headless_blocking() -> Result<Self, CompositorError> {
        static SHARED: parking_lot::Mutex<Option<GpuContext>> = parking_lot::Mutex::new(None);
        let mut shared = SHARED.lock();
        if let Some(gpu) = shared.as_ref() {
            return Ok(gpu.share());
        }
        let gpu = pollster::block_on(Self::fresh_headless())?;
        if gpu.adapter.get_info().backend == wgpu::Backend::Gl {
            return Ok(gpu);
        }
        let handle = gpu.share();
        *shared = Some(gpu);
        Ok(handle)
    }

    /// Adopt an already-created WGPU setup — the path the Slint UI uses so the
    /// compositor and the renderer share one device.
    pub fn from_parts(
        instance: wgpu::Instance,
        adapter: wgpu::Adapter,
        device: wgpu::Device,
        queue: wgpu::Queue,
    ) -> Self {
        Self {
            instance,
            adapter,
            device,
            queue,
            pipelines: std::sync::Arc::new(std::sync::OnceLock::new()),
        }
    }

    /// Another handle on the same device, for a second consumer of one GPU.
    ///
    /// Not a device: every field is a reference-counted handle into the driver,
    /// so this is four refcount bumps and no allocation. The process used to
    /// create three separate devices at startup — the compositor plugin's, the
    /// render plugin's and the engine's — each with its own driver instance,
    /// adapter and pipeline set, of which exactly one was ever rendered with.
    /// Bringing up a device is the dearest thing a start does and it holds video
    /// memory for the life of the process.
    pub fn share(&self) -> Self {
        Self {
            instance: self.instance.clone(),
            adapter: self.adapter.clone(),
            device: self.device.clone(),
            queue: self.queue.clone(),
            pipelines: std::sync::Arc::clone(&self.pipelines),
        }
    }

    /// Whether the adapter is a CPU/software rasterizer (e.g. llvmpipe in CI).
    /// Exact-pixel golden comparisons aren't portable between a software
    /// rasterizer and real GPU drivers, so tests use this to relax tolerances.
    pub fn is_software(&self) -> bool {
        self.adapter.get_info().device_type == wgpu::DeviceType::Cpu
    }
}

/// Build a `wgpu::Instance`, one thread at a time.
///
/// Creating one loads the platform's whole driver stack — on Linux the Vulkan
/// loader `dlopen`s every installed ICD — and doing that from several threads at
/// once does not return an error, it crashes inside the loader: three headless
/// contexts built in parallel land in `vkCreateInstance` together and one of
/// them jumps through a null pointer.
///
/// The lock covers construction only, and the instance itself stays per context.
/// Sharing one instead is what the handle's `Clone` invites and it is wrong
/// here: the GL backend binds its EGL context to the thread that created it, so
/// a second thread using a shared instance fails with `BadAccess` instead of
/// falling back.
fn new_instance(backends: wgpu::Backends) -> wgpu::Instance {
    static CREATING: parking_lot::Mutex<()> = parking_lot::Mutex::new(());
    let _one_at_a_time = CREATING.lock();
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

/// Whether a primary-backend adapter of `kind` is used without bringing up the
/// GL backend. `wgpu` sorts the adapters of every backend it was given by
/// device type, CPU rasterizers last, so with the GL backend present a GL
/// adapter on hardware is chosen over a Vulkan CPU rasterizer (lavapipe). A
/// CPU adapter is therefore not used from the primary backends alone. Any
/// hardware adapter is used: the full set could only replace it with a GL
/// adapter of a higher-ranked type, a discrete GPU with no Vulkan driver beside
/// an integrated one that has one.
fn primary_suffices(kind: wgpu::DeviceType) -> bool {
    match kind {
        wgpu::DeviceType::DiscreteGpu
        | wgpu::DeviceType::IntegratedGpu
        | wgpu::DeviceType::VirtualGpu
        | wgpu::DeviceType::Other => true,
        wgpu::DeviceType::Cpu => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **One headless device per process, whoever asks for it.**
    ///
    /// A device is not compared by identity because `wgpu` exposes none. It is
    /// compared by what only one device can do: a queue rejects a buffer that
    /// belongs to another device, so writing the first handle's buffer through
    /// the second handle's queue returns exactly when both name one device.
    ///
    /// Skipped where the sibling compositor tests skip — a host with no adapter
    /// cannot answer this either way.
    #[test]
    fn a_second_headless_context_is_a_handle_on_the_first() {
        let Ok(first) = GpuContext::new_headless_blocking() else {
            eprintln!("skipping shared-device test: no GPU adapter");
            return;
        };
        if first.adapter.get_info().backend == wgpu::Backend::Gl {
            eprintln!("skipping shared-device test: a GL context is not shareable");
            return;
        }
        let second = GpuContext::new_headless_blocking().expect("the device is already up");
        assert_eq!(
            first.adapter.get_info(),
            second.adapter.get_info(),
            "the second handle named a different adapter"
        );
        let buffer = first.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("aria.gpu.one-device-per-process"),
            size: 16,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        second.queue.write_buffer(&buffer, 0, &[0u8; 16]);
        second.queue.submit(std::iter::empty());
    }

    /// **A CPU rasterizer from the primary backends is not used alone.**
    ///
    /// Asking the primary backends first skips the GL backend's driver bring-up.
    /// It also skips `wgpu`'s cross-backend ranking, which places a GL adapter
    /// on hardware above a Vulkan CPU rasterizer; a host with lavapipe beside a
    /// GPU that has only a GL driver rendered on the CPU when the primary
    /// answer was used whatever its type. Every device type is listed, and
    /// `primary_suffices` matches exhaustively, so a new type fails to compile
    /// until it is placed.
    ///
    /// Not caught: which adapter `wgpu` returns on a real mixed host.
    #[test]
    fn only_a_hardware_primary_adapter_skips_the_gl_backend() {
        for (kind, used) in [
            (wgpu::DeviceType::DiscreteGpu, true),
            (wgpu::DeviceType::IntegratedGpu, true),
            (wgpu::DeviceType::VirtualGpu, true),
            (wgpu::DeviceType::Other, true),
            (wgpu::DeviceType::Cpu, false),
        ] {
            assert_eq!(
                primary_suffices(kind),
                used,
                "primary adapter of type {kind:?}"
            );
        }
    }
}
