//! GPU scope counts: the compute pass counts a premultiplied frame target into
//! the same bins `aria_analysis::measure_scopes` counts the frame read back
//! and unpremultiplied into, every histogram, waveform and vectorscope cell
//! and the sampled count, at every stride, grid size, and waveform trace.
//!
//! The frames cover every grey, the six saturated primaries and secondaries,
//! partly and fully transparent pixels, and a pseudo-random spread, at sizes
//! that leave partial workgroups on both edges. A pass that drifts from the
//! CPU fixed point, misplaces a column, keeps the last frame's counts, or
//! counts a transparent pixel fails here. Not caught: the speed of the pass,
//! which the preview worker's playback runs measure.

use aria_analysis::{
    FrameScopes, Rgba8Frame, ScopeConfig, WaveformChannel, WaveformTrace, measure_scopes,
    scope_sample_stride,
};
use aria_compositor::{
    CompositorError, GpuContext, GpuScopes, MAX_SCOPE_COLUMNS, MAX_SCOPE_GRID, RgbaImage,
    ScopeChannel, ScopeCounts, ScopeGrid, create_frame_target, wgpu,
};

fn try_gpu() -> Option<(GpuContext, GpuScopes)> {
    let gpu = match GpuContext::new_headless_blocking() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("skipping scope pass test: no GPU adapter ({e})");
            return None;
        }
    };
    let Some(pass) = GpuScopes::new(&gpu.device, &gpu.queue) else {
        eprintln!("skipping scope pass test: the adapter runs no compute pass");
        return None;
    };
    Some((gpu, pass))
}

macro_rules! gpu_or_skip {
    () => {
        match try_gpu() {
            Some(found) => found,
            None => return,
        }
    };
}

/// Straight-alpha pixels: every grey, the primaries and secondaries, a
/// transparent and a half-covered run, then a pseudo-random spread with one
/// pixel in seven partly transparent.
fn straight(width: u32, height: u32) -> RgbaImage {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    let named: [[u8; 4]; 8] = [
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 0, 255],
        [0, 255, 255, 255],
        [255, 0, 255, 255],
        [90, 200, 30, 0],
        [200, 60, 140, 128],
    ];
    let mut seed: u32 = 0x9e37_79b9;
    for i in 0..width * height {
        let px = if i < 256 {
            let grey = i as u8;
            [grey, grey, grey, 255]
        } else if i < 256 + 8 * 16 {
            named[((i - 256) / 16) as usize]
        } else {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let [r, g, b, a] = seed.to_le_bytes();
            let alpha = if a % 7 == 0 { a | 1 } else { 255 };
            [r, g, b, alpha]
        };
        pixels.extend_from_slice(&px);
    }
    RgbaImage::new(width, height, pixels)
}

/// `image` premultiplied the way the compositor stores a frame: each channel
/// times alpha over 255, rounded to nearest.
fn premultiplied(image: &RgbaImage) -> Vec<u8> {
    let mut out = image.pixels.clone();
    for px in out.as_chunks_mut::<4>().0 {
        let a = u16::from(px[3]);
        for c in &mut px[..3] {
            *c = ((u16::from(*c) * a + 127) / 255) as u8;
        }
    }
    out
}

/// A frame target on `gpu` holding `pixels`, premultiplied RGBA8.
fn upload(gpu: &GpuContext, width: u32, height: u32, pixels: &[u8]) -> wgpu::Texture {
    let texture = create_frame_target(&gpu.device, width, height);
    gpu.queue.write_texture(
        texture.as_image_copy(),
        pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
    texture
}

/// What the CPU counts in `premultiplied` read back, unpremultiplied, under
/// `config`.
fn cpu(width: u32, height: u32, premultiplied: &[u8], config: ScopeConfig) -> FrameScopes {
    let mut read = RgbaImage::new(width, height, premultiplied.to_vec());
    read.unpremultiply();
    let view = Rgba8Frame::new(
        &read.pixels,
        width as usize,
        height as usize,
        width as usize * 4,
        0.0,
    )
    .expect("a well-formed frame");
    measure_scopes(view, config)
}

/// The pass's channels for the CPU's waveform `trace`, enumerated from the
/// trace so a channel added to it fails here until the pass counts it too.
fn channels(trace: WaveformTrace) -> &'static [ScopeChannel] {
    const LUMA: &[ScopeChannel] = &[ScopeChannel::Luma];
    const RGB: &[ScopeChannel] = &[ScopeChannel::Red, ScopeChannel::Green, ScopeChannel::Blue];
    const YCBCR: &[ScopeChannel] = &[ScopeChannel::Luma, ScopeChannel::Cb, ScopeChannel::Cr];
    let pass = match trace {
        WaveformTrace::Luma => LUMA,
        WaveformTrace::RgbParade => RGB,
        WaveformTrace::YcbcrParade => YCBCR,
    };
    let named: Vec<ScopeChannel> = trace
        .channels()
        .iter()
        .map(|channel| match channel {
            WaveformChannel::Luma => ScopeChannel::Luma,
            WaveformChannel::Red => ScopeChannel::Red,
            WaveformChannel::Green => ScopeChannel::Green,
            WaveformChannel::Blue => ScopeChannel::Blue,
            WaveformChannel::Cb => ScopeChannel::Cb,
            WaveformChannel::Cr => ScopeChannel::Cr,
        })
        .collect();
    assert_eq!(pass, named.as_slice(), "{trace:?}");
    pass
}

/// The grid the CPU reads `width`×`height` under `config` with.
fn grid(width: u32, height: u32, config: ScopeConfig) -> ScopeGrid {
    let stride = scope_sample_stride(width as usize, height as usize, config.maximum_samples());
    ScopeGrid {
        columns: config.waveform_columns() as u32,
        size: config.vectorscope_size() as u32,
        stride: stride as u32,
        waveform: channels(config.waveform_trace()),
    }
}

fn counted(config: ScopeConfig, counts: ScopeCounts) -> FrameScopes {
    FrameScopes::from_counts(
        config,
        counts.histogram,
        counts.waveform,
        counts.vectorscope,
        counts.sampled,
        0.0,
    )
    .expect("the pass wrote counts for its grid")
}

/// Every bin the pass counts equals the CPU's, for strides 1 to 7 and grids
/// from one cell to the largest each scope takes.
#[test]
fn the_pass_counts_what_the_cpu_counts() {
    let (gpu, mut pass) = gpu_or_skip!();
    let (width, height) = (97, 61);
    let image = straight(width, height);
    let frame = premultiplied(&image);
    let texture = upload(&gpu, width, height, &frame);
    let pixels = (width * height) as usize;
    let configs = [
        (256, 128, pixels),
        (97, 64, pixels),
        (1, 1, pixels),
        (MAX_SCOPE_COLUMNS as usize, MAX_SCOPE_GRID as usize, pixels),
        // ceil(97/2) * ceil(61/2), ceil(97/3) * ceil(61/3), ceil(97/7) * ceil(61/7).
        (64, 31, 49 * 31),
        (300, 200, 33 * 21),
        (13, 511, 14 * 9),
    ];
    let mut strides = Vec::new();
    for (columns, size, budget) in configs {
        let base = ScopeConfig::new(columns, size, budget).expect("a valid config");
        strides.push(grid(width, height, base).stride);
        for trace in WaveformTrace::ALL {
            let config = base.with_waveform_trace(trace);
            let grid = grid(width, height, config);
            let counts = pass.measure(&texture, grid).expect("the pass counts");
            let expected = cpu(width, height, &frame, config);
            assert!(
                expected.sampled > 0,
                "the CPU sampled nothing under {grid:?}"
            );
            assert_eq!(
                counted(config, counts),
                expected,
                "the pass and the CPU counted the frame differently under {grid:?}"
            );
        }
    }
    assert_eq!(
        strides,
        [1, 1, 1, 1, 2, 3, 7],
        "the cases stopped covering strides"
    );
}

/// A pass run again, on another frame or under another grid, counts that
/// frame alone: nothing carries over from the run before.
#[test]
fn each_run_counts_its_own_frame() {
    let (gpu, mut pass) = gpu_or_skip!();
    let (width, height) = (40, 33);
    let first = premultiplied(&straight(width, height));
    let grey = premultiplied(&RgbaImage::new(
        width,
        height,
        [77, 77, 77, 255].repeat((width * height) as usize),
    ));
    let one = upload(&gpu, width, height, &first);
    let two = upload(&gpu, width, height, &grey);
    let small = ScopeConfig::new(16, 32, 4096).expect("a valid config");
    let large = ScopeConfig::new(512, 256, 4096).expect("a valid config");
    let rgb = small.with_waveform_trace(WaveformTrace::RgbParade);
    let ycbcr = large.with_waveform_trace(WaveformTrace::YcbcrParade);
    for (texture, frame, config) in [
        (&one, &first, small),
        (&one, &first, small),
        (&two, &grey, small),
        (&one, &first, rgb),
        (&two, &grey, rgb),
        (&one, &first, large),
        (&two, &grey, large),
        (&one, &first, ycbcr),
        (&one, &first, small),
    ] {
        let counts = pass
            .measure(texture, grid(width, height, config))
            .expect("the pass counts");
        assert_eq!(counted(config, counts), cpu(width, height, frame, config));
    }
}

/// A fully transparent frame counts nothing; a frame of one grey lands in
/// that grey's bin in every histogram and on the vectorscope's centre.
#[test]
fn transparent_pixels_are_not_counted_and_greys_are_neutral() {
    let (gpu, mut pass) = gpu_or_skip!();
    let (width, height) = (18, 17);
    let config = ScopeConfig::new(9, 33, 1024).expect("a valid config");
    let clear = upload(&gpu, width, height, &vec![0; (width * height * 4) as usize]);
    let counts = pass
        .measure(&clear, grid(width, height, config))
        .expect("the pass counts");
    assert_eq!(counts.sampled, 0);
    assert!(counts.histogram.iter().flatten().all(|n| *n == 0));
    assert!(
        counts
            .waveform
            .iter()
            .chain(&counts.vectorscope)
            .all(|n| *n == 0)
    );

    let grey = upload(
        &gpu,
        width,
        height,
        &[198, 198, 198, 255].repeat((width * height) as usize),
    );
    let counts = pass
        .measure(&grey, grid(width, height, config))
        .expect("the pass counts");
    let all = width * height;
    assert_eq!(counts.sampled, all);
    for channel in &counts.histogram {
        assert_eq!(channel[198], all);
    }
    assert_eq!(
        counts.vectorscope[16 * 33 + 16],
        all,
        "grey left the centre"
    );

    // A grey is its own luma and neutral Cb and Cr, code 128, in every column.
    let ycbcr = config.with_waveform_trace(WaveformTrace::YcbcrParade);
    let counts = pass
        .measure(&grey, grid(width, height, ycbcr))
        .expect("the pass counts");
    let plane = 9 * 256;
    for (index, code) in [(0, 198), (1, 128), (2, 128)] {
        let at_code: u32 = (0..9)
            .map(|column| counts.waveform[index * plane + column * 256 + code])
            .sum();
        assert_eq!(at_code, all, "waveform plane {index} left code {code}");
    }
}

/// A frame the pass cannot read, and a grid outside its bounds, fail with the
/// error that names them before anything is dispatched.
#[test]
fn an_unreadable_frame_or_an_unbounded_grid_is_named() {
    let (gpu, mut pass) = gpu_or_skip!();
    let ok = ScopeGrid {
        columns: 8,
        size: 8,
        stride: 1,
        waveform: &[ScopeChannel::Luma],
    };
    let make = |format, usage| {
        gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d {
                width: 4,
                height: 4,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
    };
    for (format, usage) in [
        (
            wgpu::TextureFormat::Bgra8Unorm,
            wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        (
            wgpu::TextureFormat::Rgba8UnormSrgb,
            wgpu::TextureUsages::TEXTURE_BINDING,
        ),
        (
            wgpu::TextureFormat::Rgba8Unorm,
            wgpu::TextureUsages::COPY_DST,
        ),
    ] {
        let texture = make(format, usage);
        assert!(
            matches!(
                pass.measure(&texture, ok),
                Err(CompositorError::ScopeSource { format: f, usage: u }) if f == format && u == usage
            ),
            "{format:?} with {usage:?} was read"
        );
    }
    let frame = upload(&gpu, 4, 4, &[0; 64]);
    for bad in [
        ScopeGrid { columns: 0, ..ok },
        ScopeGrid {
            columns: MAX_SCOPE_COLUMNS + 1,
            ..ok
        },
        ScopeGrid { size: 0, ..ok },
        ScopeGrid {
            size: MAX_SCOPE_GRID + 1,
            ..ok
        },
        ScopeGrid { stride: 0, ..ok },
        ScopeGrid {
            waveform: &[],
            ..ok
        },
        ScopeGrid {
            waveform: &[
                ScopeChannel::Luma,
                ScopeChannel::Red,
                ScopeChannel::Green,
                ScopeChannel::Blue,
            ],
            ..ok
        },
    ] {
        assert!(
            matches!(
                pass.measure(&frame, bad),
                Err(CompositorError::ScopeGrid { columns, size, stride, channels })
                    if (columns, size, stride, channels)
                        == (bad.columns, bad.size, bad.stride, bad.waveform.len())
            ),
            "{bad:?} was counted"
        );
    }
    for edge in [
        ScopeGrid {
            columns: MAX_SCOPE_COLUMNS,
            size: MAX_SCOPE_GRID,
            stride: 1,
            waveform: &[ScopeChannel::Cr, ScopeChannel::Cb, ScopeChannel::Luma],
        },
        ScopeGrid {
            columns: 1,
            size: 1,
            stride: 1000,
            waveform: &[ScopeChannel::Luma],
        },
    ] {
        assert!(pass.measure(&frame, edge).is_ok(), "{edge:?} was refused");
    }
}
