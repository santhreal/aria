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
}

impl PlaneTexturePool {
    pub(super) fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.textures_created = 0;
    }

    pub(super) fn stats(&self) -> TexturePoolStats {
        TexturePoolStats {
            textures_created: self.textures_created,
            retained_textures: self.slots.len(),
        }
    }

    fn upload(
        &mut self,
        gpu: &GpuContext,
        data: &[u8],
        bytes_per_row: u32,
        width: u32,
        height: u32,
        format: wgpu::TextureFormat,
    ) -> PlaneTexture {
        let key = PlaneKey {
            width,
            height,
            format,
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
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &slot.plane.0,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        (slot.plane.0.clone(), slot.plane.1.clone())
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

pub(super) fn rgba_image_key(image: &RgbaImage, expected: usize) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(image.width.to_le_bytes());
    digest.update(image.height.to_le_bytes());
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
pub(super) fn upload_cpu_planes(
    gpu: &GpuContext,
    pool: &mut PlaneTexturePool,
    frame: &VideoFrame,
    image: &CpuImage,
    cw: u32,
    ch: u32,
) -> Result<PlaneSet, CompositorError> {
    match frame.format {
        PixelFormat::Nv12 => {
            expect_planes(image.planes.len(), 2, frame.format)?;
            let y = upload_plane(
                gpu,
                pool,
                &image.planes[0],
                cw,
                ch,
                cw,
                wgpu::TextureFormat::R8Unorm,
            )?;
            let uv = upload_plane(
                gpu,
                pool,
                &image.planes[1],
                cw.div_ceil(2),
                ch.div_ceil(2),
                cw,
                wgpu::TextureFormat::Rg8Unorm,
            )?;
            Ok(PlaneSet::Biplanar { y, uv })
        }
        PixelFormat::Yuv420p => {
            expect_planes(image.planes.len(), 3, frame.format)?;
            let y = upload_plane(
                gpu,
                pool,
                &image.planes[0],
                cw,
                ch,
                cw,
                wgpu::TextureFormat::R8Unorm,
            )?;
            let u = upload_plane(
                gpu,
                pool,
                &image.planes[1],
                cw / 2,
                ch / 2,
                cw / 2,
                wgpu::TextureFormat::R8Unorm,
            )?;
            let v = upload_plane(
                gpu,
                pool,
                &image.planes[2],
                cw / 2,
                ch / 2,
                cw / 2,
                wgpu::TextureFormat::R8Unorm,
            )?;
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

fn upload_plane(
    gpu: &GpuContext,
    pool: &mut PlaneTexturePool,
    plane: &Plane,
    width: u32,
    height: u32,
    minimum_row_bytes: u32,
    format: wgpu::TextureFormat,
) -> Result<PlaneTexture, CompositorError> {
    if !plane.is_well_formed()
        || plane.rows < height as usize
        || plane.stride < minimum_row_bytes as usize
    {
        return Err(CompositorError::MalformedFrame(format!(
            "plane has {} bytes at stride {} for {} rows; need {minimum_row_bytes} bytes across {height} rows",
            plane.data.len(),
            plane.stride,
            plane.rows,
        )));
    }
    let bytes_per_row = u32::try_from(plane.stride).map_err(|_| {
        CompositorError::MalformedFrame(format!(
            "plane stride {} exceeds the GPU row-pitch limit",
            plane.stride
        ))
    })?;
    Ok(pool.upload(gpu, &plane.data, bytes_per_row, width, height, format))
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
        Some(g) => g.lanes(1.0),
        None => ColorGrade::IDENTITY.lanes(0.0),
    }
}
