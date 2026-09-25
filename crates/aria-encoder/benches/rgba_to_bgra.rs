//! 4K RGBA-to-BGRA conversion benchmarks for hardware video encoders.
//!
//! Run: `cargo bench -p aria-encoder --bench rgba_to_bgra`
//!
//! Measures chunked SIMD/as_chunks swizzle throughput for converting packed RGBA
//! frames into VideoToolbox's native 32-bit BGRA pixel buffers.

use aria_encoder::{copy_bgra, swizzle_rgba_to_bgra};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

fn bench_rgba_to_bgra_4k_frame(c: &mut Criterion) {
    let width = 3840usize;
    let height = 2160usize;
    let frame_bytes = width * height * 4;

    let src = vec![0xAA_u8; frame_bytes];
    let mut dst = vec![0_u8; frame_bytes];

    let mut group = c.benchmark_group("encoder/rgba_to_bgra/4k_frame");
    group.throughput(Throughput::Bytes(frame_bytes as u64));

    group.bench_function("swizzle_4k_3840x2160", |b| {
        b.iter(|| {
            for y in 0..height {
                let row_offset = y * width * 4;
                let src_row = &src[row_offset..row_offset + width * 4];
                let dst_row = &mut dst[row_offset..row_offset + width * 4];
                swizzle_rgba_to_bgra(black_box(src_row), black_box(dst_row), width);
            }
            black_box(&dst[0..4]);
        });
    });

    group.bench_function("copy_bgra_4k_3840x2160_baseline", |b| {
        b.iter(|| {
            for y in 0..height {
                let row_offset = y * width * 4;
                let src_row = &src[row_offset..row_offset + width * 4];
                let dst_row = &mut dst[row_offset..row_offset + width * 4];
                copy_bgra(black_box(src_row), black_box(dst_row), width);
            }
            black_box(&dst[0..4]);
        });
    });

    group.finish();
}

fn bench_rgba_to_bgra_rows(c: &mut Criterion) {
    let resolutions = [
        ("4k_row_3840", 3840usize),
        ("1080p_row_1920", 1920usize),
        ("720p_row_1280", 1280usize),
    ];

    let mut group = c.benchmark_group("encoder/rgba_to_bgra/row");

    for &(label, width) in &resolutions {
        let row_bytes = width * 4;
        let src = vec![0xBB_u8; row_bytes];
        let mut dst = vec![0_u8; row_bytes];

        group.throughput(Throughput::Bytes(row_bytes as u64));
        group.bench_with_input(BenchmarkId::new("swizzle", label), &width, |b, &w| {
            b.iter(|| {
                swizzle_rgba_to_bgra(black_box(&src), black_box(&mut dst), w);
                black_box(&dst[0..4]);
            });
        });
    }

    group.finish();
}

fn bench_rgba_to_bgra_1080p_frame(c: &mut Criterion) {
    let width = 1920usize;
    let height = 1080usize;
    let frame_bytes = width * height * 4;

    let src = vec![0xCC_u8; frame_bytes];
    let mut dst = vec![0_u8; frame_bytes];

    let mut group = c.benchmark_group("encoder/rgba_to_bgra/1080p_frame");
    group.throughput(Throughput::Bytes(frame_bytes as u64));

    group.bench_function("swizzle_1080p_1920x1080", |b| {
        b.iter(|| {
            for y in 0..height {
                let row_offset = y * width * 4;
                let src_row = &src[row_offset..row_offset + width * 4];
                let dst_row = &mut dst[row_offset..row_offset + width * 4];
                swizzle_rgba_to_bgra(black_box(src_row), black_box(dst_row), width);
            }
            black_box(&dst[0..4]);
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_rgba_to_bgra_4k_frame,
    bench_rgba_to_bgra_rows,
    bench_rgba_to_bgra_1080p_frame,
);
criterion_main!(benches);
