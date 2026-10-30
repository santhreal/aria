//! Zero-copy import of Linux DMA-BUF video frames into `wgpu` textures.
//!
//! GStreamer hands the decoder a DMA-BUF-backed NV12 sample. Rather than map
//! the planes to system memory and `write_texture` them, we import the fd
//! through `VK_KHR_external_memory_fd` + `VK_EXT_external_memory_dma_buf`,
//! adopt the `VkImage` as a `wgpu` NV12 texture, and expose its planes as
//! `R8Unorm` / `Rg8Unorm` views for the compositor's existing NV12 YUV path.
//!
//! ## Honest limit
//!
//! wgpu-hal 29 creates images with `VK_IMAGE_TILING_OPTIMAL` and no
//! `VkImageDrmFormatModifierListCreateInfoEXT`. Tiled hardware modifiers
//! (NVIDIA block-linear, Intel CCS, AMD DCC) therefore fail import. Only
//! `DRM_FORMAT_MOD_LINEAR` NV12 is accepted; anything else is
//! [`CompositorError::UnsupportedFormat`] so the renderer falls back to CPU
//! decode. That is a real fallback, not a silent copy.
//!
//! ## Version pinning
//!
//! `ash` 0.38 is the same line wgpu-hal 29 links, so `vk::Image` /
//! `vk::DeviceMemory` unify across the import seam.
//! The module calls Vulkan external-memory FFI and `wgpu-hal` raw-texture import and is `unsafe` throughout.

#![allow(unsafe_code)]

use parking_lot::Mutex;
use std::fs::File;
use std::hash::Hash;
use std::os::fd::{BorrowedFd, FromRawFd, IntoRawFd, OwnedFd};
use std::os::unix::fs::MetadataExt;

use ash::vk;
use ash::vk::Handle;
use rustc_hash::FxHashMap;

use aria_core::{DmaBufSurface, GpuSurface};

use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::planes::PlaneSet;

const MAX_CACHED_TEXTURES: usize = 32;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct SurfaceKey {
    devices: [u64; 4],
    inodes: [u64; 4],
    coded: (u32, u32),
    fourcc: u32,
    modifier: u64,
    plane_count: u32,
    strides: [u32; 4],
    offsets: [u32; 4],
}

impl SurfaceKey {
    fn new(payload: &DmaBufSurface, coded: (u32, u32)) -> Result<Self, CompositorError> {
        let mut devices = [0; 4];
        let mut inodes = [0; 4];
        for index in 0..payload.plane_count as usize {
            let fd = payload.fds[index];
            if fd < 0 {
                return Err(CompositorError::MalformedFrame(format!(
                    "DMABuf plane {index} fd is invalid"
                )));
            }
            // SAFETY: the frame's keep-alive retains every descriptor through
            // this import call. Clone it so metadata lookup cannot close it.
            let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
            let duplicate = borrowed.try_clone_to_owned().map_err(|error| {
                CompositorError::MalformedFrame(format!(
                    "could not duplicate DMABuf plane {index} fd {fd}: {error}"
                ))
            })?;
            let metadata = File::from(duplicate).metadata().map_err(|error| {
                CompositorError::MalformedFrame(format!(
                    "could not identify DMABuf plane {index} fd {fd}: {error}"
                ))
            })?;
            devices[index] = metadata.dev();
            inodes[index] = metadata.ino();
        }
        Ok(Self {
            devices,
            inodes,
            coded,
            fourcc: payload.fourcc,
            modifier: payload.modifier,
            plane_count: payload.plane_count,
            strides: payload.strides,
            offsets: payload.offsets,
        })
    }
}

struct BoundedLru<K, V> {
    entries: FxHashMap<K, (V, u64)>,
    clock: u64,
}

impl<K, V> Default for BoundedLru<K, V> {
    fn default() -> Self {
        Self {
            entries: FxHashMap::default(),
            clock: 0,
        }
    }
}

impl<K: Copy + Eq + Hash, V> BoundedLru<K, V> {
    fn get_or_try_insert_with<E>(
        &mut self,
        key: K,
        create: impl FnOnce() -> Result<V, E>,
    ) -> Result<&V, E> {
        self.clock = self.clock.wrapping_add(1);
        if !self.entries.contains_key(&key) {
            if self.entries.len() >= MAX_CACHED_TEXTURES {
                let oldest = self
                    .entries
                    .iter()
                    .min_by_key(|(_, (_, last_used))| *last_used)
                    .map(|(key, _)| *key);
                if let Some(oldest) = oldest {
                    self.entries.remove(&oldest);
                }
            }
            self.entries.insert(key, (create()?, self.clock));
        }
        let (value, last_used) = self
            .entries
            .get_mut(&key)
            .expect("the requested cache entry was inserted");
        *last_used = self.clock;
        Ok(value)
    }
}

struct CachedTexture {
    texture: wgpu::Texture,
    y: wgpu::TextureView,
    uv: wgpu::TextureView,
}

impl CachedTexture {
    fn planes(&self) -> PlaneSet {
        PlaneSet::Biplanar {
            y: (self.texture.clone(), self.y.clone()),
            uv: (self.texture.clone(), self.uv.clone()),
        }
    }
}

/// Imports LINEAR DMA-BUF NV12 surfaces into `wgpu` textures for one device.
///
/// Built once per [`GpuContext`]. `None` when the adapter is not Vulkan, lacks
/// `TEXTURE_FORMAT_NV12`, or did not enable `VK_KHR_external_memory_fd`.
pub(crate) struct VulkanSurfaceImporter {
    textures: Mutex<BoundedLru<SurfaceKey, CachedTexture>>,
    /// Pins the wgpu instance→adapter→device chain until cached Vulkan images
    /// and imported memory have been released. Keep this field last.
    #[allow(dead_code)]
    device: wgpu::Device,
}

// SAFETY: Vulkan devices are free-threaded for the import calls we make;
// the importer lives in the Compositor, which moves with the renderer.
unsafe impl Send for VulkanSurfaceImporter {}
unsafe impl Sync for VulkanSurfaceImporter {}

impl VulkanSurfaceImporter {
    pub(crate) fn new(gpu: &GpuContext) -> Option<Self> {
        if gpu.adapter.get_info().backend != wgpu::Backend::Vulkan {
            return None;
        }
        if !gpu
            .device
            .features()
            .contains(wgpu::Features::TEXTURE_FORMAT_NV12)
        {
            return None;
        }
        // SAFETY: read-only backend probe; the guard drops before return.
        let enabled =
            unsafe { gpu.device.as_hal::<wgpu::hal::api::Vulkan>() }.and_then(|hal_device| {
                let name = ash::khr::external_memory_fd::NAME;
                hal_device
                    .enabled_device_extensions()
                    .contains(&name)
                    .then_some(())
            });
        enabled?;
        Some(Self {
            textures: Mutex::new(BoundedLru::default()),
            device: gpu.device.clone(),
        })
    }

    /// Import a LINEAR NV12 DMA-BUF as `(luma, chroma)` wgpu views.
    ///
    /// `coded` is the full coded buffer size in pixels.
    pub(crate) fn import_nv12(
        &self,
        gpu: &GpuContext,
        surface: &GpuSurface,
        coded: (u32, u32),
    ) -> Result<PlaneSet, CompositorError> {
        let payload = surface.handle() as *const DmaBufSurface;
        if payload.is_null() {
            return Err(CompositorError::MalformedFrame(
                "DMABuf surface payload is null".into(),
            ));
        }
        // SAFETY: for `GpuSurfaceKind::DmaBuf` the handle points at the
        // `DmaBufSurface` owned by the frame's keep-alive, which outlives
        // this borrow of the frame.
        let payload = unsafe { *payload };
        if !payload.is_linear_nv12() {
            return Err(CompositorError::UnsupportedFormat(format!(
                "DMABuf import needs LINEAR NV12; got fourcc={:#x} modifier={:#x} planes={}",
                payload.fourcc, payload.modifier, payload.plane_count
            )));
        }
        if payload.fds[0] < 0 {
            return Err(CompositorError::MalformedFrame(
                "DMABuf plane 0 fd is invalid".into(),
            ));
        }

        let (cw, ch) = coded;
        if cw == 0 || ch == 0 {
            return Err(CompositorError::MalformedFrame(
                "DMABuf coded size is zero".into(),
            ));
        }

        let key = SurfaceKey::new(&payload, coded)?;
        let mut textures = self.textures.lock();
        let texture = textures
            .get_or_try_insert_with(key, || self.create_cached_texture(gpu, &payload, cw, ch))?;
        Ok(texture.planes())
    }

    fn create_cached_texture(
        &self,
        gpu: &GpuContext,
        payload: &DmaBufSurface,
        width: u32,
        height: u32,
    ) -> Result<CachedTexture, CompositorError> {
        let imported = self.import_image(gpu, payload, width, height)?;
        let descriptor = wgpu::TextureDescriptor {
            label: Some("aria.zerocopy.nv12"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::NV12,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        };
        // SAFETY: `imported` was created on this device and matches `descriptor`.
        let texture = unsafe {
            gpu.device
                .create_texture_from_hal::<wgpu::hal::api::Vulkan>(imported, &descriptor)
        };
        let y = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("aria.zerocopy.nv12.y"),
            format: Some(wgpu::TextureFormat::R8Unorm),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::Plane0,
            ..Default::default()
        });
        let uv = texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("aria.zerocopy.nv12.uv"),
            format: Some(wgpu::TextureFormat::Rg8Unorm),
            dimension: Some(wgpu::TextureViewDimension::D2),
            aspect: wgpu::TextureAspect::Plane1,
            ..Default::default()
        });
        Ok(CachedTexture { texture, y, uv })
    }

    fn import_image(
        &self,
        gpu: &GpuContext,
        payload: &DmaBufSurface,
        width: u32,
        height: u32,
    ) -> Result<wgpu::hal::vulkan::Texture, CompositorError> {
        // SAFETY: `as_hal` hands back the backing Vulkan device; all ash
        // objects created here are adopted by wgpu-hal before the guard drops.
        unsafe {
            gpu.device
                .as_hal::<wgpu::hal::api::Vulkan>()
                .ok_or_else(|| {
                    CompositorError::UnsupportedFormat("device is not the Vulkan backend".into())
                })
                .and_then(|hal| self.import_image_on(&hal, payload, width, height))
        }
    }

    unsafe fn import_image_on(
        &self,
        hal: &wgpu::hal::vulkan::Device,
        payload: &DmaBufSurface,
        width: u32,
        height: u32,
    ) -> Result<wgpu::hal::vulkan::Texture, CompositorError> {
        let raw = hal.raw_device();
        let instance = hal.shared_instance().raw_instance();

        let mut external = vk::ExternalMemoryImageCreateInfo::default()
            .handle_types(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT);
        let create = vk::ImageCreateInfo::default()
            .flags(vk::ImageCreateFlags::MUTABLE_FORMAT | vk::ImageCreateFlags::EXTENDED_USAGE)
            .image_type(vk::ImageType::TYPE_2D)
            .format(vk::Format::G8_B8R8_2PLANE_420_UNORM)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::LINEAR)
            .usage(vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED)
            .push_next(&mut external);

        let image = unsafe { raw.create_image(&create, None) }.map_err(|e| {
            CompositorError::UnsupportedFormat(format!(
                "Vulkan rejected LINEAR NV12 DMABuf image create ({e}). Use a LINEAR surface or decode on CPU."
            ))
        })?;

        let req = unsafe { raw.get_image_memory_requirements(image) };
        let mem_type = find_memory_type(
            instance,
            hal.raw_physical_device(),
            req.memory_type_bits,
            vk::MemoryPropertyFlags::empty(),
        )
        .ok_or_else(|| {
            destroy_image(raw, image);
            CompositorError::UnsupportedFormat("no Vulkan memory type accepts this DMABuf".into())
        })?;

        // Import owns a duplicate fd: ash/Vulkan takes it; the decoder keeps
        // the original for the frame lifetime.
        let import_fd = dup_fd(payload.fds[0]).map_err(|e| {
            destroy_image(raw, image);
            CompositorError::UnsupportedFormat(format!(
                "could not dup DMABuf fd: {e}. Check ulimit -n."
            ))
        })?;

        let dedicated = vk::MemoryDedicatedAllocateInfo::default().image(image);
        let mut import = vk::ImportMemoryFdInfoKHR::default()
            .handle_type(vk::ExternalMemoryHandleTypeFlags::DMA_BUF_EXT)
            .fd(import_fd);
        // ash 0.38 has no `push_next` on ImportMemoryFdInfoKHR (same as
        // wgpu-hal's Win32 import).
        import.p_next = <*const _>::cast(&dedicated);
        let alloc = vk::MemoryAllocateInfo::default()
            .allocation_size(req.size.max(1))
            .memory_type_index(mem_type)
            .push_next(&mut import);

        let memory = match unsafe { raw.allocate_memory(&alloc, None) } {
            Ok(memory) => memory,
            Err(e) => {
                // Vulkan consumed the fd on both success and most failures;
                // do not close it again.
                destroy_image(raw, image);
                return Err(CompositorError::UnsupportedFormat(format!(
                    "Vulkan could not import DMABuf memory ({e}). Decode on CPU instead."
                )));
            }
        };

        if let Err(e) = unsafe { raw.bind_image_memory(image, memory, 0) } {
            unsafe { raw.free_memory(memory, None) };
            destroy_image(raw, image);
            return Err(CompositorError::UnsupportedFormat(format!(
                "Vulkan could not bind imported DMABuf ({e}). Decode on CPU instead."
            )));
        }

        let desc = wgpu::hal::TextureDescriptor {
            label: Some("aria.zerocopy.nv12"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::NV12,
            usage: wgpu::TextureUses::RESOURCE,
            memory_flags: wgpu::hal::MemoryFlags::empty(),
            view_formats: vec![wgpu::TextureFormat::R8Unorm, wgpu::TextureFormat::Rg8Unorm],
        };
        Ok(unsafe {
            hal.texture_from_raw(
                image,
                &desc,
                None,
                wgpu::hal::vulkan::TextureMemory::Dedicated(memory),
            )
        })
    }
}

fn destroy_image(raw: &ash::Device, image: vk::Image) {
    if !image.is_null() {
        unsafe { raw.destroy_image(image, None) };
    }
}

fn find_memory_type(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    type_bits: u32,
    required: vk::MemoryPropertyFlags,
) -> Option<u32> {
    let props = unsafe { instance.get_physical_device_memory_properties(physical) };
    for (i, ty) in props
        .memory_types
        .iter()
        .enumerate()
        .take(props.memory_type_count as usize)
    {
        if type_bits & (1 << i) != 0 && ty.property_flags.contains(required) {
            return Some(i as u32);
        }
    }
    None
}

fn dup_fd(fd: i32) -> std::io::Result<i32> {
    // SAFETY: `fd` is a live DMA-BUF owned by the decoder keep-alive.
    let owned = unsafe { OwnedFd::from_raw_fd(fd) };
    let duped = owned.try_clone();
    // Relinquish the original so Drop does not close the decoder's fd.
    let _ = owned.into_raw_fd();
    Ok(duped?.into_raw_fd())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linear_nv12_predicate_matches_core() {
        let mut s = DmaBufSurface {
            fourcc: DmaBufSurface::NV12,
            modifier: DmaBufSurface::LINEAR,
            plane_count: 1,
            fds: [3, 0, 0, 0],
            strides: [1920, 0, 0, 0],
            offsets: [0, 0, 0, 0],
        };
        assert!(s.is_linear_nv12());
        s.modifier = 1;
        assert!(!s.is_linear_nv12());
        s.modifier = DmaBufSurface::LINEAR;
        s.fourcc = 0;
        assert!(!s.is_linear_nv12());
    }

    /// WHY: a decoder pool reuses the same DMA-BUF slot for many frames. The
    /// Vulkan image and memory import must run once per underlying slot, while
    /// the bounded cache evicts only the least-recently-used inactive slot.
    #[test]
    fn imported_surface_cache_creates_once_and_evicts_one() {
        let mut cache = BoundedLru::default();
        let mut creates = 0;
        for key in 0..MAX_CACHED_TEXTURES as u64 {
            let value = cache
                .get_or_try_insert_with(key, || {
                    creates += 1;
                    Ok::<_, ()>(key)
                })
                .expect("insert");
            assert_eq!(*value, key);
        }
        assert_eq!(creates, MAX_CACHED_TEXTURES);

        let reused = cache
            .get_or_try_insert_with(0, || {
                creates += 1;
                Ok::<_, ()>(u64::MAX)
            })
            .expect("reuse");
        assert_eq!(*reused, 0);
        assert_eq!(creates, MAX_CACHED_TEXTURES);

        cache
            .get_or_try_insert_with(MAX_CACHED_TEXTURES as u64, || {
                creates += 1;
                Ok::<_, ()>(MAX_CACHED_TEXTURES as u64)
            })
            .expect("bounded insert");
        assert_eq!(cache.entries.len(), MAX_CACHED_TEXTURES);
        assert!(cache.entries.contains_key(&0), "recent slot was evicted");
        assert!(!cache.entries.contains_key(&1), "oldest slot was retained");
        assert_eq!(creates, MAX_CACHED_TEXTURES + 1);
    }

    #[test]
    fn surface_identity_uses_the_underlying_fd_not_descriptor_number() {
        use std::os::fd::AsRawFd;
        use std::os::unix::net::UnixStream;

        let (surface, _peer) = UnixStream::pair().expect("socket pair");
        let duplicate = surface.try_clone().expect("duplicate descriptor");
        let payload = |fd| DmaBufSurface {
            fourcc: DmaBufSurface::NV12,
            modifier: DmaBufSurface::LINEAR,
            plane_count: 1,
            fds: [fd, 0, 0, 0],
            strides: [1920, 0, 0, 0],
            offsets: [0, 0, 0, 0],
        };
        let original =
            SurfaceKey::new(&payload(surface.as_raw_fd()), (1920, 1080)).expect("original key");
        let duplicated =
            SurfaceKey::new(&payload(duplicate.as_raw_fd()), (1920, 1080)).expect("duplicate key");
        assert_eq!(duplicated, original);
        assert_ne!(
            SurfaceKey::new(&payload(duplicate.as_raw_fd()), (1280, 720)).expect("resized key"),
            original
        );
    }
}
