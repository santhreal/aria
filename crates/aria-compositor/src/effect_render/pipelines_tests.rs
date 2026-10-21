//! Every replacing pipeline writes every pixel of its target.
//!
//! # Why
//!
//! [`color_pass::replace_quad`] draws a [`ReplacePipeline`] onto a target it
//! neither loads nor clears, so the target holds undefined texels until the
//! draw. That is sound only while the pipeline's quad covers the target, its
//! fragment shader never discards, and blending stays off. Each replacing
//! pipeline is drawn here over two different loaded targets: a pixel the draw
//! misses, or blends into, keeps what was under it, and the two results
//! differ. The unloaded draw through `replace_quad` has to match them too.
//!
//! The cases are read from the effect registry rows, the shipped transition
//! ids, and [`BlendMode::ALL`], with the single pipelines listed beside them.
//! A replacing cell of [`SharedPassPipelines::cells`] that no case builds
//! fails the test. A pipeline that blends but writes opaque pixels for these
//! inputs draws the same over both targets and is not caught.

use super::*;
use crate::color_pass;
use crate::effect_render::{Bound, PassCache};
use crate::gpu::GpuContext;
use crate::layer::BlendMode;
use crate::passes::shipped_transition_ids;

const W: u32 = 16;
const H: u32 = 10;

/// Zeroed uniforms, larger than the uniform block of any pass.
const UNIFORMS: [u8; 1024] = [0; 1024];

/// The two contents a target holds before a replacing draw: opaque magenta
/// and a translucent green.
const UNDER: [wgpu::Color; 2] = [
    wgpu::Color {
        r: 1.0,
        g: 0.0,
        b: 1.0,
        a: 1.0,
    },
    wgpu::Color {
        r: 0.0,
        g: 0.5,
        b: 0.0,
        a: 0.25,
    },
];

/// What the layout of a replacing pipeline binds.
#[derive(Clone, Copy)]
enum Inputs {
    /// A texture, the sampler, and uniforms.
    Effect,
    /// Two textures, the sampler, and uniforms.
    Pair,
    /// A texture, the sampler, a 3D LUT, the sampler, and uniforms.
    Lut,
    /// A texture and the sampler.
    Blit,
}

struct Case<'a> {
    label: String,
    pipeline: &'a ReplacePipeline,
    layout: &'a wgpu::BindGroupLayout,
    inputs: Inputs,
}

/// Every replacing pipeline of `shared`, built.
fn cases(shared: &SharedPassPipelines) -> Vec<Case<'_>> {
    let mut cases = Vec::new();
    for (row, pass) in effect_passes().iter().enumerate() {
        for index in 0..pass.shaders.len() {
            cases.push(Case {
                label: format!("effect {} pass {index}", pass.id),
                pipeline: shared.effect_pass(row, index),
                layout: &shared.effect_layout,
                inputs: Inputs::Effect,
            });
        }
    }
    // An id with no shipped pass crossfades.
    for id in shipped_transition_ids()
        .iter()
        .copied()
        .chain(["unshipped"])
    {
        cases.push(Case {
            label: format!("transition {id}"),
            pipeline: shared.transition_pipeline(id),
            layout: &shared.transition_layout,
            inputs: Inputs::Pair,
        });
    }
    for mode in BlendMode::ALL {
        cases.push(Case {
            label: format!("blend {mode:?}"),
            pipeline: shared.blend_pipeline(mode),
            layout: &shared.blend_layout,
            inputs: Inputs::Pair,
        });
    }
    let singles = [
        (
            "passthrough",
            shared.passthrough(),
            &shared.effect_layout,
            Inputs::Effect,
        ),
        (
            "matte",
            shared.matte(),
            &shared.transition_layout,
            Inputs::Pair,
        ),
        (
            "mask stack",
            shared.mask_stack(),
            &shared.transition_layout,
            Inputs::Pair,
        ),
        (
            "grade",
            shared.grade(),
            &shared.effect_layout,
            Inputs::Effect,
        ),
        ("lut", shared.lut(), &shared.lut_layout, Inputs::Lut),
        (
            "replacing blit",
            shared.replace_pipeline(),
            &shared.blit_layout,
            Inputs::Blit,
        ),
        (
            "silhouette",
            shared.silhouette_pipeline(),
            &shared.effect_layout,
            Inputs::Effect,
        ),
    ];
    for (label, pipeline, layout, inputs) in singles {
        cases.push(Case {
            label: label.to_string(),
            pipeline,
            layout,
            inputs,
        });
    }
    cases
}

fn extent(width: u32, height: u32, depth: u32) -> wgpu::Extent3d {
    wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: depth,
    }
}

fn texture(
    gpu: &GpuContext,
    size: wgpu::Extent3d,
    dimension: wgpu::TextureDimension,
    usage: wgpu::TextureUsages,
) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("test.replace"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension,
        format: RT_FORMAT,
        usage,
        view_formats: &[],
    })
}

/// A sampled texture of `size` holding `data`, tightly packed.
fn filled(
    gpu: &GpuContext,
    size: wgpu::Extent3d,
    dimension: wgpu::TextureDimension,
    data: &[u8],
) -> wgpu::TextureView {
    let usage = wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST;
    let texture = texture(gpu, size, dimension, usage);
    gpu.queue.write_texture(
        texture.as_image_copy(),
        data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size.width * 4),
            rows_per_image: Some(size.height),
        },
        size,
    );
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

/// A `W`x`H` input whose texel at `(x, y)` is `texel(x, y)`.
fn input(gpu: &GpuContext, texel: impl Fn(u8, u8) -> [u8; 4]) -> wgpu::TextureView {
    let mut data = Vec::with_capacity((W * H * 4) as usize);
    for y in 0..H as u8 {
        for x in 0..W as u8 {
            data.extend_from_slice(&texel(x, y));
        }
    }
    filled(gpu, extent(W, H, 1), wgpu::TextureDimension::D2, &data)
}

/// A 2x2x2 LUT whose corners are the corners of the RGB cube.
fn lut(gpu: &GpuContext) -> wgpu::TextureView {
    let mut data = Vec::with_capacity(2 * 2 * 2 * 4);
    for b in [0, 255] {
        for g in [0, 255] {
            for r in [0, 255] {
                data.extend_from_slice(&[r, g, b, 255]);
            }
        }
    }
    filled(gpu, extent(2, 2, 2), wgpu::TextureDimension::D3, &data)
}

/// The pixels of `target`, read back through the staging buffer of `cache`.
fn read(gpu: &GpuContext, cache: &PassCache, target: &wgpu::Texture) -> Vec<u8> {
    let staging = cache.take_cpu_staging(&gpu.device, W, H);
    let mut encoder = gpu
        .device
        .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &staging.readback_buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(staging.padded_bytes_per_row),
                rows_per_image: Some(H),
            },
        },
        extent(W, H, 1),
    );
    gpu.queue.submit([encoder.finish()]);
    let slice = staging.readback_buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |mapped| {
        mapped.expect("map the readback buffer");
    });
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("wait for the readback");
    let pixels: Vec<u8> = slice
        .get_mapped_range()
        .chunks(staging.padded_bytes_per_row as usize)
        .flat_map(|row| &row[..(W * 4) as usize])
        .copied()
        .collect();
    staging.readback_buffer.unmap();
    cache.return_cpu_staging(staging);
    pixels
}

#[test]
fn every_replacing_pipeline_writes_every_pixel() {
    let gpu = GpuContext::new_headless_blocking().expect("headless GPU");
    let shared = SharedPassPipelines::new(&gpu.device);
    let cache = PassCache::new(gpu.queue.clone());
    let first = input(&gpu, |x, y| {
        [x * 16, y * 25, 255 - x * 16, 40 + x * 8 + y * 6]
    });
    let second = input(&gpu, |x, y| [y * 25, 200 - x * 8, x * 16, 255 - y * 20]);
    let lut = lut(&gpu);
    let usage = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
    let targets: [wgpu::Texture; 3] =
        std::array::from_fn(|_| texture(&gpu, extent(W, H, 1), wgpu::TextureDimension::D2, usage));
    let views = targets
        .each_ref()
        .map(|target| target.create_view(&wgpu::TextureViewDescriptor::default()));

    let cases = cases(&shared);
    let (replacing, _) = shared.cells();
    assert_eq!(
        replacing.filter(|cell| cell.get().is_none()).count(),
        0,
        "a replacing pipeline has no case"
    );

    for case in &cases {
        let bounds = match case.inputs {
            Inputs::Effect => vec![
                Bound::Texture(&first),
                Bound::Sampler(&shared.sampler),
                Bound::Uniform(&UNIFORMS),
            ],
            Inputs::Pair => vec![
                Bound::Texture(&first),
                Bound::Texture(&second),
                Bound::Sampler(&shared.sampler),
                Bound::Uniform(&UNIFORMS),
            ],
            Inputs::Lut => vec![
                Bound::Texture(&first),
                Bound::Sampler(&shared.sampler),
                Bound::Texture(&lut),
                Bound::Sampler(&shared.sampler),
                Bound::Uniform(&UNIFORMS),
            ],
            Inputs::Blit => vec![Bound::Texture(&first), Bound::Sampler(&shared.sampler)],
        };
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
        color_pass::clear(&mut encoder, &views[0], UNDER[0]);
        color_pass::clear(&mut encoder, &views[1], UNDER[1]);
        color_pass::clear(&mut encoder, &views[2], UNDER[0]);
        {
            let bind_group = cache.bind_group(&gpu.device, case.layout, "test.replace", &bounds);
            for view in &views[..2] {
                let pipeline = case.pipeline.raw();
                color_pass::draw_quad(&mut encoder, view, color_pass::KEEP, pipeline, &bind_group);
            }
            color_pass::replace_quad(&mut encoder, &views[2], case.pipeline, &bind_group);
        }
        gpu.queue.submit([encoder.finish()]);
        let [over_first, over_second, unloaded] =
            targets.each_ref().map(|target| read(&gpu, &cache, target));
        assert_eq!(
            over_first, over_second,
            "{}: a pixel keeps what was under it",
            case.label
        );
        assert_eq!(
            unloaded, over_first,
            "{}: the unloaded draw writes other pixels",
            case.label
        );
    }
}
