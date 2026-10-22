use aria_core::{CpuImage, MatrixCoefficients, PixelFormat, Plane, RgbaImage, VideoFrame};
use rustc_hash::FxHashMap;
use sha2::{Digest, Sha256};

use crate::error::CompositorError;
use crate::gpu::GpuContext;
use crate::grade::ColorGrade;
use crate::layer::{CompositeLayer, CompositorConfig, LayerPlacement};
use crate::planes::{PlaneSet, PlaneTexture};

const MAX_RETAINED_PLANE_TEXTURES: usize = 16;
const MAX_RETAINED_RGBA_TEXTURES: usize = 32;
/// Staging chunk for plane uploads. A frame's planes are closed for copying as
/// soon as they are written, so each uploaded frame occupies its own chunk
/// until the GPU has read it: a 1080p NV12 frame (3.3 MB padded) fits one, and
/// a 4K luma plane (8.3 MB) gets a chunk of its own size, which is retained.
const PLANE_STAGING_CHUNK: u64 = 4 << 20;
/// Offset alignment of each plane in its chunk: Direct3D 12's texture data
/// placement alignment, below which wgpu copies through an intermediate buffer.
const PLANE_STAGING_ALIGNMENT: wgpu::BufferSize = wgpu::BufferSize::new(512).unwrap();

/// GPU texture allocations and retained entries for one upload cache.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct TexturePoolStats {
    /// Textures allocated during the last frame.
    pub textures_created: u32,
    /// Textures retained after the last frame.
    pub retained_textures: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct PlaneKey {
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
}

struct PlaneSlot {
    key: PlaneKey,
    plane: PlaneTexture,
    last_frame: u64,
}

#[derive(Default)]
pub(super) struct PlaneTexturePool {
    slots: Vec<PlaneSlot>,
    frame: u64,
    textures_created: u32,
    /// The planes each CPU frame of the current render uploaded, keyed by the
    /// frame's address. A render borrows every frame it draws until it
    /// returns, so within one render an address states one frame, and a frame
    /// drawn more than once (motion-blur samples, a reference and its layer)
    /// uploads once.
    uploaded: Vec<(usize, PlaneSet)>,
    /// Staging memory the planes are written into, retained across frames.
    staging: Option<wgpu::util::StagingBelt>,
}

impl PlaneTexturePool {
    pub(super) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.textures_created = 0;
        self.uploaded.clear();
    }

    /// Return staging space to the pool once the GPU has read it. Call after
    /// every submission of an encoder that [`Self::frame_planes`] recorded
    /// into.
    pub(super) fn recall(&mut self) {
        if let Some(staging) = self.staging.as_mut() {
            staging.recall();
        }
    }

    /// The plane textures of the CPU frame `frame`, uploading its planes into
    /// retained textures on its first draw of the render. The copies are
    /// recorded into `encoder` ahead of any pass that samples them, and their
    /// staging is closed before this returns, so a submission of `encoder` at
    /// any later point carries them.
    pub(super) fn frame_planes(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        frame: &VideoFrame,
        image: &CpuImage,
    ) -> Result<PlaneSet, CompositorError> {
        let at = std::ptr::from_ref(frame).addr();
        if let Some((_, planes)) = self.uploaded.iter().find(|(key, _)| *key == at) {
            return Ok(planes.clone());
        }
        let planes = upload_cpu_planes(gpu, encoder, self, frame, image);
        if let Some(staging) = self.staging.as_mut() {
            staging.finish();
        }
        let planes = planes?;
        self.uploaded.push((at, planes.clone()));
        Ok(planes)
    }

    pub(super) fn stats(&self) -> TexturePoolStats {
        TexturePoolStats {
            textures_created: self.textures_created,
            retained_textures: self.slots.len(),
        }
    }

    /// Stage `data` (rows of `row_bytes` at `stride`) and record its copy into
    /// a retained texture of `key`'s shape into `encoder`.
    fn upload(
        &mut self,
        gpu: &GpuContext,
        encoder: &mut wgpu::CommandEncoder,
        data: &[u8],
        stride: usize,
        row_bytes: u32,
        key: PlaneKey,
    ) -> Result<PlaneTexture, CompositorError> {
        // Rows are staged at the copy pitch, so a plane whose stride already
        // meets it stages in one block.
        let pitch = row_bytes.checked_next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
        let size =
            pitch.and_then(|pitch| wgpu::BufferSize::new(u64::from(pitch) * u64::from(key.height)));
        let (Some(pitch), Some(size)) = (pitch, size) else {
            return Err(CompositorError::MalformedFrame(format!(
                "plane of {row_bytes} bytes across {} rows has no copy layout",
                key.height
            )));
        };
        let slot_index = self
            .slots
            .iter()
            .position(|slot| slot.key == key && slot.last_frame != self.frame)
            .unwrap_or_else(|| {
                let plane = create_plane_texture(&gpu.device, key);
                self.textures_created = self.textures_created.saturating_add(1);
                let replacement = (self.slots.len() >= MAX_RETAINED_PLANE_TEXTURES)
                    .then(|| {
                        self.slots
                            .iter()
                            .enumerate()
                            .filter(|(_, slot)| slot.last_frame != self.frame)
                            .min_by_key(|(_, slot)| slot.last_frame)
                            .map(|(index, _)| index)
                    })
                    .flatten();
                if let Some(index) = replacement {
                    self.slots[index] = PlaneSlot {
                        key,
                        plane,
                        last_frame: self.frame,
                    };
                    index
                } else {
                    self.slots.push(PlaneSlot {
                        key,
                        plane,
                        last_frame: self.frame,
                    });
                    self.slots.len() - 1
                }
            });

        let slot = &mut self.slots[slot_index];
        slot.last_frame = self.frame;
        let staging = self.staging.get_or_insert_with(|| {
            wgpu::util::StagingBelt::new(gpu.device.clone(), PLANE_STAGING_CHUNK)
        });
        let space = staging.allocate(size, PLANE_STAGING_ALIGNMENT);
        {
            let mut view = space.get_mapped_range_mut();
            let (pitch, row_bytes) = (pitch as usize, row_bytes as usize);
            if stride == pitch {
                view.copy_from_slice(&data[..view.len()]);
            } else {
                for (row, src) in data.chunks(stride).take(key.height as usize).enumerate() {
                    let at = row * pitch;
                    view.slice(at..at + row_bytes)
                        .copy_from_slice(&src[..row_bytes]);
                }
            }
        }
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: space.buffer(),
                layout: wgpu::TexelCopyBufferLayout {
                    offset: space.offset(),
                    bytes_per_row: Some(pitch),
                    rows_per_image: Some(key.height),
                },
            },
            wgpu::TexelCopyTextureInfo {
                texture: &slot.plane.0,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width: key.width,
                height: key.height,
                depth_or_array_layers: 1,
            },
        );
        Ok((slot.plane.0.clone(), slot.plane.1.clone()))
    }
}

struct RgbaSlot {
    plane: PlaneTexture,
    last_frame: u64,
}

#[derive(Default)]
pub(super) struct RgbaTextureCache {
    entries: FxHashMap<[u8; 32], RgbaSlot>,
    frame: u64,
    textures_created: u32,
}

impl RgbaTextureCache {
    pub(super) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.textures_created = 0;
    }

    pub(super) fn stats(&self) -> TexturePoolStats {
        TexturePoolStats {
            textures_created: self.textures_created,
            retained_textures: self.entries.len(),
        }
    }

    pub(super) fn get(&mut self, key: &[u8; 32]) -> Option<PlaneTexture> {
        let slot = self.entries.get_mut(key)?;
        slot.last_frame = self.frame;
        Some((slot.plane.0.clone(), slot.plane.1.clone()))
    }

    pub(super) fn insert(&mut self, key: [u8; 32], plane: PlaneTexture) {
        if self.entries.len() >= MAX_RETAINED_RGBA_TEXTURES {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, slot)| slot.last_frame)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                self.entries.remove(&oldest);
            }
        }
        self.textures_created = self.textures_created.saturating_add(1);
        self.entries.insert(
            key,
            RgbaSlot {
                plane,
                last_frame: self.frame,
            },
        );
    }
}

/// The texture cache key of `image` uploaded inside a `border`-texel
/// transparent ring.
pub(super) fn rgba_image_key(image: &RgbaImage, expected: usize, border: u32) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(image.width.to_le_bytes());
    digest.update(image.height.to_le_bytes());
    digest.update(border.to_le_bytes());
    digest.update(&image.pixels[..expected]);
    digest.finalize().into()
}

fn create_plane_texture(device: &wgpu::Device, key: PlaneKey) -> PlaneTexture {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("aria-video-plane"),
        size: wgpu::Extent3d {
            width: key.width,
            height: key.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: key.format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

/// Upload a CPU frame's planes into retained textures per its pixel layout,
/// honoring each plane's byte stride. The returned [`PlaneSet`] states the
/// chroma layout, so no separate biplanar flag travels with the textures.
fn upload_cpu_planes(
    gpu: &GpuContext,
    encoder: &mut wgpu::CommandEncoder,
    pool: &mut PlaneTexturePool,
    frame: &VideoFrame,
    image: &CpuImage,
) -> Result<PlaneSet, CompositorError> {
    let (cw, ch) = frame.coded_size;
    let mut plane = |index: usize, width: u32, height: u32, format| {
        upload_plane(
            gpu,
            encoder,
            pool,
            &image.planes[index],
            width,
            height,
            format,
        )
    };
    match frame.format {
        PixelFormat::Nv12 => {
            expect_planes(image.planes.len(), 2, frame.format)?;
            let y = plane(0, cw, ch, wgpu::TextureFormat::R8Unorm)?;
            let uv = plane(
                1,
                cw.div_ceil(2),
                ch.div_ceil(2),
                wgpu::TextureFormat::Rg8Unorm,
            )?;
            Ok(PlaneSet::Biplanar { y, uv })
        }
        PixelFormat::Yuv420p => {
            expect_planes(image.planes.len(), 3, frame.format)?;
            let y = plane(0, cw, ch, wgpu::TextureFormat::R8Unorm)?;
            let (chroma_w, chroma_h) = (cw.div_ceil(2), ch.div_ceil(2));
            let u = plane(1, chroma_w, chroma_h, wgpu::TextureFormat::R8Unorm)?;
            let v = plane(2, chroma_w, chroma_h, wgpu::TextureFormat::R8Unorm)?;
            Ok(PlaneSet::Triplanar { y, u, v })
        }
        PixelFormat::P010 | PixelFormat::Rgba8 | PixelFormat::Bgra8 => {
            Err(CompositorError::UnsupportedFormat(format!(
                "{:?} is not handled by the compositor yet (8-bit NV12 / I420 only)",
                frame.format
            )))
        }
    }
}

/// Stage one decoded CPU plane and record its copy into a pooled
/// [`PlaneTexture`].
///
/// The rows are written into the pool's retained [`wgpu::util::StagingBelt`]
/// at the 256-byte copy pitch and copied by `CommandEncoder::copy_buffer_to_texture`.
/// `Queue::write_texture` instead allocates a staging buffer per plane and
/// frees it once the GPU has read it, and at 4K the freed memory is faulted in
/// again on the next frame.
fn upload_plane(
    gpu: &GpuContext,
    encoder: &mut wgpu::CommandEncoder,
    pool: &mut PlaneTexturePool,
    plane: &Plane,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
) -> Result<PlaneTexture, CompositorError> {
    let texel = format.block_copy_size(None).ok_or_else(|| {
        CompositorError::UnsupportedFormat(format!("{format:?} has no single-aspect copy size"))
    })?;
    let row_bytes = u64::from(width) * u64::from(texel);
    if !plane.is_well_formed() || plane.rows < height as usize || (plane.stride as u64) < row_bytes
    {
        return Err(CompositorError::MalformedFrame(format!(
            "plane has {} bytes at stride {} for {} rows; need {row_bytes} bytes across {height} rows",
            plane.data.len(),
            plane.stride,
            plane.rows,
        )));
    }
    let row_bytes = u32::try_from(row_bytes).map_err(|_| {
        CompositorError::MalformedFrame(format!(
            "plane row of {row_bytes} bytes exceeds the GPU row-pitch limit"
        ))
    })?;
    let key = PlaneKey {
        width,
        height,
        format,
    };
    pool.upload(gpu, encoder, &plane.data, plane.stride, row_bytes, key)
}

fn expect_planes(got: usize, want: usize, format: PixelFormat) -> Result<(), CompositorError> {
    if got == want {
        Ok(())
    } else {
        Err(CompositorError::MalformedFrame(format!(
            "{format:?} needs {want} planes, got {got}"
        )))
    }
}

/// Luma coefficients (Kr, Kb) for the YUV→RGB matrix. `resolved()` upstream maps
/// `Unspecified` to BT.709, so only concrete matrices reach here; `Identity` is
/// degenerate on YUV and falls back to BT.709.
pub(super) fn luma_coeffs(matrix: MatrixCoefficients) -> (f32, f32) {
    match matrix {
        MatrixCoefficients::Bt601 => (0.299, 0.114),
        MatrixCoefficients::Bt2020Ncl => (0.2627, 0.0593),
        MatrixCoefficients::Bt709
        | MatrixCoefficients::Identity
        | MatrixCoefficients::Unspecified => (0.2126, 0.0722),
    }
}

/// Effective clockwise rotation (radians): the placement plus the frame's
/// container rotation, so sideways-recorded phone video plays upright.
pub(super) fn placement_rotation(layer: &CompositeLayer<'_>, frame: &VideoFrame) -> f32 {
    layer.placement.rotation + (frame.rotation.degrees() as f32).to_radians()
}

/// Map the layer's UV rect (in visible-picture space) into the coded texture,
/// since the visible picture may be a sub-rect of a padded decode buffer.
pub(super) fn visible_uv_rect(frame: &VideoFrame, uv: [f32; 4]) -> [f32; 4] {
    let (cw, ch) = (frame.coded_size.0 as f32, frame.coded_size.1 as f32);
    let vx = frame.visible.x as f32 / cw;
    let vy = frame.visible.y as f32 / ch;
    let vw = frame.visible.width as f32 / cw;
    let vh = frame.visible.height as f32 / ch;
    [
        vx + uv[0] * vw,
        vy + uv[1] * vh,
        vx + uv[2] * vw,
        vy + uv[3] * vh,
    ]
}

pub(super) fn placement_affine(
    config: &CompositorConfig,
    p: &LayerPlacement,
    extra_rotation: f32,
) -> ([f32; 4], [f32; 4]) {
    placement_affine_rot(config, p, p.rotation + extra_rotation)
}

/// Build the unit-quad ([-0.5, 0.5]², +y down) → clip-space affine for a
/// placement, using `rotation` (radians, clockwise) instead of `p.rotation`.
pub(super) fn placement_affine_rot(
    config: &CompositorConfig,
    p: &LayerPlacement,
    rotation: f32,
) -> ([f32; 4], [f32; 4]) {
    let (cw, ch) = (config.width as f32, config.height as f32);
    let (cos, sin) = (rotation.cos(), rotation.sin());
    // Canvas space (+y down): pos = center + R·(corner ⊙ size), R the clockwise
    // rotation [cos, -sin; sin, cos] in screen coords.
    let a = cos * p.size[0];
    let b = sin * p.size[0];
    let c = -sin * p.size[1];
    let d = cos * p.size[1];
    // Canvas px → clip space: x' = 2x/cw − 1, y' = 1 − 2y/ch (flip y).
    let linear = [2.0 * a / cw, -2.0 * b / ch, 2.0 * c / cw, -2.0 * d / ch];
    let trans = [
        2.0 * p.center[0] / cw - 1.0,
        1.0 - 2.0 * p.center[1] / ch,
        p.opacity,
        0.0,
    ];
    (linear, trans)
}

/// Pack a layer's grade into the uniform lanes, or the identity fast path.
///
/// The lane layout is [`ColorGrade::lanes`]'s, and the shader reads it back
/// through the accessors generated beside it, so neither side states a slot.
pub(super) fn pack_grade(grade: Option<ColorGrade>) -> [[f32; 4]; ColorGrade::LANES] {
    match grade.filter(|g| !g.is_identity()) {
        Some(g) => g.lanes(g.shader_enabled()),
        None => ColorGrade::IDENTITY.lanes(0.0),
    }
}
