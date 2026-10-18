//! Specialized blend pipeline composite benchmarks.
//!
//! Run: `cargo bench -p aria-compositor --bench blend_pipeline`
//!
//! Measures full-frame compositing throughput across specialized blend mode
//! pipelines (13 pipeline-overridable constant variants compiled with `@id(0)`).

use aria_compositor::{
    BlendMode, CompositeLayer, Compositor, CompositorConfig, GpuContext, LayerPlacement, RgbaImage,
};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

fn bench_blend_modes_solid(c: &mut Criterion) {
    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping blend benchmark: no GPU adapter");
        return;
    };

    let mut comp = Compositor::new(&gpu);
    let width = 1920;
    let height = 1080;
    let bytes_per_frame = (width as u64) * (height as u64) * 4;
    let config = CompositorConfig::new(width, height).with_background([30, 40, 50, 255]);
    let placement = LayerPlacement::full_canvas(&config);

    let mut group = c.benchmark_group("compositor/blend_pipeline/solid_1080p");
    group.throughput(Throughput::Bytes(bytes_per_frame));

    for &mode in &BlendMode::ALL {
        let mode_name = format!("{mode:?}").to_ascii_lowercase();
        group.bench_with_input(BenchmarkId::new("mode", &mode_name), &mode, |b, &m| {
            let layer = CompositeLayer::solid([200, 120, 60, 220], placement).with_blend_mode(m);
            b.iter(|| {
                let image = comp
                    .render(&gpu, &config, black_box(&[layer]))
                    .expect("render");
                black_box(image);
            });
        });
    }
    group.finish();
}

fn bench_blend_modes_bitmap(c: &mut Criterion) {
    let Ok(gpu) = GpuContext::new_headless_blocking() else {
        eprintln!("skipping blend benchmark: no GPU adapter");
        return;
    };

    let mut comp = Compositor::new(&gpu);
    let width = 1920;
    let height = 1080;
    let bytes_per_frame = (width as u64) * (height as u64) * 4;
    let config = CompositorConfig::new(width, height).with_background([20, 20, 20, 255]);
    let placement = LayerPlacement::full_canvas(&config);

    // Create a 1080p RGBA bitmap layer
    let mut pixels = vec![0_u8; width as usize * height as usize * 4];
    for (i, chunk) in pixels.chunks_exact_mut(4).enumerate() {
        chunk[0] = ((i * 7) % 256) as u8;
        chunk[1] = ((i * 13) % 256) as u8;
        chunk[2] = ((i * 29) % 256) as u8;
        chunk[3] = 200;
    }
    let bitmap = RgbaImage::new(width, height, pixels);

    let mut group = c.benchmark_group("compositor/blend_pipeline/bitmap_1080p");
    group.throughput(Throughput::Bytes(bytes_per_frame));

    // Representative blend modes across blend categories:
    // normal (fast-path), darken (min), multiply (separable), screen (separable),
    // overlay (complex dual), color_dodge (complex cancel), difference (inversion), add (linear)
    let representative_modes = [
        ("normal", BlendMode::Normal),
        ("multiply", BlendMode::Multiply),
        ("screen", BlendMode::Screen),
        ("overlay", BlendMode::Overlay),
        ("darken", BlendMode::Darken),
        ("lighten", BlendMode::Lighten),
        ("color_dodge", BlendMode::ColorDodge),
        ("color_burn", BlendMode::ColorBurn),
        ("hard_light", BlendMode::HardLight),
        ("difference", BlendMode::Difference),
        ("add", BlendMode::Add),
    ];

    for (name, mode) in representative_modes {
        group.bench_with_input(BenchmarkId::new("mode", name), &mode, |b, &m| {
            let layer = CompositeLayer::rgba(&bitmap, placement).with_blend_mode(m);
            b.iter(|| {
                let image = comp
                    .render(&gpu, &config, black_box(&[layer]))
                    .expect("render");
                black_box(image);
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_blend_modes_solid, bench_blend_modes_bitmap);
criterion_main!(benches);
