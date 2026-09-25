//! Shot frame format conversion benchmarks.
//!
//! Run: `cargo bench -p aria-detect --bench shot_format_convert`
//!
//! Measures fast-path row-walker conversion and downscaling across representative
//! decoded video frame formats (NV12, YUV420p, RGBA8, BGRA8) into the fixed
//! sampling grid for shot detection.

use aria_core::{
    ColorSpace, CpuImage, FrameData, PixelFormat, Plane, Rational, RationalTime, Rect, Rotation,
    VideoFrame,
};
use aria_detect::{SAMPLE_EDGE, convert_into};
use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};

fn make_nv12_frame(width: u32, height: u32) -> VideoFrame {
    let luma = vec![128_u8; (width * height) as usize];
    let chroma = vec![128_u8; (width * (height / 2)) as usize];
    let planes = vec![
        Plane::new(luma, width as usize, height as usize),
        Plane::new(chroma, width as usize, (height / 2) as usize),
    ];
    VideoFrame {
        pts: RationalTime::new(0, Rational::FPS_24),
        duration: None,
        format: PixelFormat::Nv12,
        color: ColorSpace::BT709,
        coded_size: (width, height),
        visible: Rect::from_size(width, height),
        rotation: Rotation::None,
        data: FrameData::Cpu(CpuImage::new(planes)),
    }
}

fn make_yuv420p_frame(width: u32, height: u32) -> VideoFrame {
    let luma = vec![128_u8; (width * height) as usize];
    let u = vec![128_u8; ((width / 2) * (height / 2)) as usize];
    let v = vec![128_u8; ((width / 2) * (height / 2)) as usize];
    let planes = vec![
        Plane::new(luma, width as usize, height as usize),
        Plane::new(u, (width / 2) as usize, (height / 2) as usize),
        Plane::new(v, (width / 2) as usize, (height / 2) as usize),
    ];
    VideoFrame {
        pts: RationalTime::new(0, Rational::FPS_24),
        duration: None,
        format: PixelFormat::Yuv420p,
        color: ColorSpace::BT709,
        coded_size: (width, height),
        visible: Rect::from_size(width, height),
        rotation: Rotation::None,
        data: FrameData::Cpu(CpuImage::new(planes)),
    }
}

fn make_rgba8_frame(width: u32, height: u32) -> VideoFrame {
    let data = vec![128_u8; (width * height * 4) as usize];
    let planes = vec![Plane::new(data, (width * 4) as usize, height as usize)];
    VideoFrame {
        pts: RationalTime::new(0, Rational::FPS_24),
        duration: None,
        format: PixelFormat::Rgba8,
        color: ColorSpace::BT709,
        coded_size: (width, height),
        visible: Rect::from_size(width, height),
        rotation: Rotation::None,
        data: FrameData::Cpu(CpuImage::new(planes)),
    }
}

fn make_bgra8_frame(width: u32, height: u32) -> VideoFrame {
    let data = vec![128_u8; (width * height * 4) as usize];
    let planes = vec![Plane::new(data, (width * 4) as usize, height as usize)];
    VideoFrame {
        pts: RationalTime::new(0, Rational::FPS_24),
        duration: None,
        format: PixelFormat::Bgra8,
        color: ColorSpace::BT709,
        coded_size: (width, height),
        visible: Rect::from_size(width, height),
        rotation: Rotation::None,
        data: FrameData::Cpu(CpuImage::new(planes)),
    }
}

fn bench_format_convert_1080p(c: &mut Criterion) {
    let width = 1920;
    let height = 1080;
    let mut buffer = vec![0_u8; SAMPLE_EDGE * SAMPLE_EDGE * 4];

    let formats: [(&str, VideoFrame, u64); 4] = [
        (
            "nv12",
            make_nv12_frame(width, height),
            (width * height * 3 / 2) as u64,
        ),
        (
            "yuv420p",
            make_yuv420p_frame(width, height),
            (width * height * 3 / 2) as u64,
        ),
        (
            "rgba8",
            make_rgba8_frame(width, height),
            (width * height * 4) as u64,
        ),
        (
            "bgra8",
            make_bgra8_frame(width, height),
            (width * height * 4) as u64,
        ),
    ];

    let mut group = c.benchmark_group("detect/shot_format_convert/1080p");
    for (name, frame, bytes) in &formats {
        group.throughput(Throughput::Bytes(*bytes));
        group.bench_with_input(BenchmarkId::new("format", name), frame, |b, f| {
            b.iter(|| {
                let dims = convert_into(black_box(f), black_box(&mut buffer)).expect("convert");
                black_box(dims);
                black_box(&buffer[0..4]);
            });
        });
    }
    group.finish();
}

fn bench_format_convert_4k(c: &mut Criterion) {
    let width = 3840;
    let height = 2160;
    let mut buffer = vec![0_u8; SAMPLE_EDGE * SAMPLE_EDGE * 4];

    let formats: [(&str, VideoFrame, u64); 4] = [
        (
            "nv12",
            make_nv12_frame(width, height),
            (width * height * 3 / 2) as u64,
        ),
        (
            "yuv420p",
            make_yuv420p_frame(width, height),
            (width * height * 3 / 2) as u64,
        ),
        (
            "rgba8",
            make_rgba8_frame(width, height),
            (width * height * 4) as u64,
        ),
        (
            "bgra8",
            make_bgra8_frame(width, height),
            (width * height * 4) as u64,
        ),
    ];

    let mut group = c.benchmark_group("detect/shot_format_convert/4k");
    for (name, frame, bytes) in &formats {
        group.throughput(Throughput::Bytes(*bytes));
        group.bench_with_input(BenchmarkId::new("format", name), frame, |b, f| {
            b.iter(|| {
                let dims = convert_into(black_box(f), black_box(&mut buffer)).expect("convert");
                black_box(dims);
                black_box(&buffer[0..4]);
            });
        });
    }
    group.finish();
}

fn bench_format_convert_360p(c: &mut Criterion) {
    let width = 640;
    let height = 360;
    let mut buffer = vec![0_u8; SAMPLE_EDGE * SAMPLE_EDGE * 4];

    let formats: [(&str, VideoFrame, u64); 4] = [
        (
            "nv12",
            make_nv12_frame(width, height),
            (width * height * 3 / 2) as u64,
        ),
        (
            "yuv420p",
            make_yuv420p_frame(width, height),
            (width * height * 3 / 2) as u64,
        ),
        (
            "rgba8",
            make_rgba8_frame(width, height),
            (width * height * 4) as u64,
        ),
        (
            "bgra8",
            make_bgra8_frame(width, height),
            (width * height * 4) as u64,
        ),
    ];

    let mut group = c.benchmark_group("detect/shot_format_convert/360p");
    for (name, frame, bytes) in &formats {
        group.throughput(Throughput::Bytes(*bytes));
        group.bench_with_input(BenchmarkId::new("format", name), frame, |b, f| {
            b.iter(|| {
                let dims = convert_into(black_box(f), black_box(&mut buffer)).expect("convert");
                black_box(dims);
                black_box(&buffer[0..4]);
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    bench_format_convert_1080p,
    bench_format_convert_4k,
    bench_format_convert_360p,
);
criterion_main!(benches);
