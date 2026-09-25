//! Sampled video frames for shot detection.
//!
//! `aria-analysis` compares RGBA frames; the platform decoders emit YUV planes or
//! packed RGB. This module is the seam, and it is deliberately narrow:
//!
//! - **Frames stream, they are never collected.** A sampled frame is converted into one
//!   reused buffer and pushed straight into the detector, so a 30-minute source costs
//!   one small frame's memory rather than thousands.
//! - **Conversion downscales while it converts.** The detector samples a bounded grid
//!   anyway, so producing a full-resolution RGBA copy first would be work nothing reads.
//! - **A format the seam cannot read is refused by name.** A silent empty result would
//!   read as "no cuts in this footage".

use std::path::Path;

use aria_analysis::{Rgba8Frame, ShotBoundary, ShotDetectionConfig, ShotDetector};
use aria_core::{FrameData, PixelFormat, RationalTime, VideoFrame};

/// Width and height the sampler converts into.
///
/// Matches `ShotDetectionConfig`'s own sampling grid: a larger buffer would be
/// subsampled again by the detector, and a smaller one would throw away detail it
/// would have used.
pub const SAMPLE_EDGE: usize = 256;

/// Longest video one shot-detection run samples, in seconds.
///
/// A walk is bounded for the same reason the audio decode is: a feature-length
/// source would otherwise sample for as long as it takes to decode it.
pub const MAX_SHOT_SECONDS: f64 = 60.0 * 30.0;

/// Frames per second sampled for shot detection.
///
/// A cut is a whole-frame change, so it survives coarse sampling; two frames
/// per second finds cuts in edited footage without decoding every frame.
pub const SHOT_SAMPLE_FPS: f64 = 2.0;

/// Detect shot boundaries by sampling `path` at `fps`, up to `max_seconds`.
///
/// One decoder walk, one reused conversion buffer, and one streaming detector. The
/// walk stops early when `cancelled` returns true, so a long source can be abandoned.
pub fn detect_shots_sampled(
    path: &Path,
    fps: f64,
    max_seconds: f64,
    config: ShotDetectionConfig,
    cancelled: impl Fn() -> bool,
) -> Result<Vec<ShotBoundary>, String> {
    if !fps.is_finite() || fps <= 0.0 {
        return Err("the shot sampling rate must be positive".into());
    }
    let mut decoder = aria_decoder::open_video_decoder(path, aria_decoder::OutputMode::Cpu)
        .map_err(|err| format!("{}: {err}", path.display()))?;
    let time_base = decoder.info().time_base;
    detect_shots_from_source(time_base, fps, max_seconds, config, cancelled, |target| {
        decoder
            .frame_at(target)
            .map_err(|err| format!("{}: {err}", path.display()))
    })
}

fn detect_shots_from_source(
    time_base: aria_core::Rational,
    fps: f64,
    max_seconds: f64,
    config: ShotDetectionConfig,
    cancelled: impl Fn() -> bool,
    mut frame_at: impl FnMut(RationalTime) -> Result<Option<VideoFrame>, String>,
) -> Result<Vec<ShotBoundary>, String> {
    let step_seconds = 1.0 / fps;
    let mut detector = ShotDetector::new(config);
    let mut buffer = vec![0_u8; SAMPLE_EDGE * SAMPLE_EDGE * 4];
    let mut boundaries = Vec::new();
    let mut sample = 0_u64;
    let mut last_pushed = f64::NEG_INFINITY;
    let mut consecutive_anomalies = 0usize;
    const MAX_CONSECUTIVE_ANOMALIES: usize = 10;

    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        let target_seconds = sample as f64 * step_seconds;
        if target_seconds > max_seconds {
            break;
        }
        let target = RationalTime::new(
            (target_seconds * f64::from(time_base.num) / f64::from(time_base.den)) as i64,
            time_base,
        );
        let Some(frame) = frame_at(target)? else {
            break;
        };
        sample += 1;

        let seconds = frame.pts.seconds();
        if seconds <= last_pushed {
            consecutive_anomalies += 1;
            if consecutive_anomalies >= MAX_CONSECUTIVE_ANOMALIES {
                break;
            }
            continue;
        }
        consecutive_anomalies = 0;
        last_pushed = seconds;
        let (width, height) = convert_into(&frame, &mut buffer)?;
        let view = Rgba8Frame::new(&buffer, width, height, width * 4, seconds.max(0.0))
            .map_err(|err| format!("sampled frame: {err}"))?;
        if let Some(boundary) = detector
            .push_frame(view)
            .map_err(|err| format!("shot detection: {err}"))?
        {
            boundaries.push(boundary);
        }
    }
    Ok(boundaries)
}

/// Convert one decoded frame into `buffer` as packed RGBA, downscaled to at most
/// [`SAMPLE_EDGE`] on each axis. Returns the written dimensions.
///
/// Nearest-neighbour by design: the detector compares a sampled grid and a histogram,
/// and an interpolating filter would blur exactly the edges a cut moves.
pub fn convert_into(frame: &VideoFrame, buffer: &mut [u8]) -> Result<(usize, usize), String> {
    let FrameData::Cpu(image) = &frame.data else {
        return Err("the decoder returned a GPU surface for analysis".into());
    };
    let source_width = frame.visible.width.max(1) as usize;
    let source_height = frame.visible.height.max(1) as usize;
    let width = source_width.min(SAMPLE_EDGE);
    let height = source_height.min(SAMPLE_EDGE);

    let planes = &image.planes;
    let luma = planes.first().ok_or("the decoded frame has no planes")?;
    match frame.format {
        PixelFormat::Nv12 => {
            let chroma = planes
                .get(1)
                .ok_or("the decoded frame has no chroma plane")?;
            convert_nv12(
                luma,
                chroma,
                source_width,
                source_height,
                width,
                height,
                buffer,
            );
        }
        PixelFormat::Yuv420p => {
            let u_plane = planes
                .get(1)
                .ok_or("the decoded frame has no U chroma plane")?;
            let v_plane = planes
                .get(2)
                .ok_or("the decoded frame has no V chroma plane")?;
            convert_yuv420p(
                luma,
                u_plane,
                v_plane,
                source_width,
                source_height,
                width,
                height,
                buffer,
            );
        }
        PixelFormat::Rgba8 => {
            convert_rgb_packed(
                luma,
                source_width,
                source_height,
                width,
                height,
                buffer,
                false,
            );
        }
        PixelFormat::Bgra8 => {
            convert_rgb_packed(
                luma,
                source_width,
                source_height,
                width,
                height,
                buffer,
                true,
            );
        }
        PixelFormat::P010 => {
            return Err("10-bit frames are not sampled for shot detection yet".into());
        }
    }
    Ok((width, height))
}

fn convert_nv12(
    luma: &aria_core::Plane,
    chroma: &aria_core::Plane,
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    buffer: &mut [u8],
) {
    let luma_stride = luma.stride;
    let chroma_stride = chroma.stride;
    let luma_data = &luma.data;
    let chroma_data = &chroma.data;

    for y in 0..height {
        let source_y = y * source_height / height;
        let luma_row_offset = source_y * luma_stride;
        let chroma_row_offset = (source_y / 2) * chroma_stride;
        let dst_row_start = y * width * 4;

        for x in 0..width {
            let source_x = x * source_width / width;
            let luma_idx = luma_row_offset + source_x;
            let y_val = luma_data.get(luma_idx).copied().unwrap_or(0);
            let chroma_base = chroma_row_offset + (source_x / 2) * 2;
            let u_val = chroma_data.get(chroma_base).copied().unwrap_or(128);
            let v_val = chroma_data.get(chroma_base + 1).copied().unwrap_or(128);
            let rgb = yuv_to_rgb(y_val, u_val, v_val);
            let dst_offset = dst_row_start + x * 4;
            if let Some(slot) = buffer.get_mut(dst_offset..dst_offset + 4) {
                slot[0] = rgb[0];
                slot[1] = rgb[1];
                slot[2] = rgb[2];
                slot[3] = 255;
            }
        }
    }
}

fn convert_yuv420p(
    luma: &aria_core::Plane,
    u_plane: &aria_core::Plane,
    v_plane: &aria_core::Plane,
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    buffer: &mut [u8],
) {
    let luma_stride = luma.stride;
    let u_stride = u_plane.stride;
    let v_stride = v_plane.stride;
    let luma_data = &luma.data;
    let u_data = &u_plane.data;
    let v_data = &v_plane.data;

    for y in 0..height {
        let source_y = y * source_height / height;
        let luma_row_offset = source_y * luma_stride;
        let uv_row_y = source_y / 2;
        let u_row_offset = uv_row_y * u_stride;
        let v_row_offset = uv_row_y * v_stride;
        let dst_row_start = y * width * 4;

        for x in 0..width {
            let source_x = x * source_width / width;
            let uv_x = source_x / 2;
            let y_val = luma_data
                .get(luma_row_offset + source_x)
                .copied()
                .unwrap_or(0);
            let u_val = u_data.get(u_row_offset + uv_x).copied().unwrap_or(128);
            let v_val = v_data.get(v_row_offset + uv_x).copied().unwrap_or(128);
            let rgb = yuv_to_rgb(y_val, u_val, v_val);
            let dst_offset = dst_row_start + x * 4;
            if let Some(slot) = buffer.get_mut(dst_offset..dst_offset + 4) {
                slot[0] = rgb[0];
                slot[1] = rgb[1];
                slot[2] = rgb[2];
                slot[3] = 255;
            }
        }
    }
}

fn convert_rgb_packed(
    plane: &aria_core::Plane,
    source_width: usize,
    source_height: usize,
    width: usize,
    height: usize,
    buffer: &mut [u8],
    swap_rb: bool,
) {
    let stride = plane.stride;
    let data = &plane.data;

    for y in 0..height {
        let source_y = y * source_height / height;
        let row_offset = source_y * stride;
        let dst_row_start = y * width * 4;

        for x in 0..width {
            let source_x = x * source_width / width;
            let base = row_offset + source_x * 4;
            let pixel = data.get(base..base + 4).unwrap_or(&[0, 0, 0, 255]);
            let dst_offset = dst_row_start + x * 4;
            if let Some(slot) = buffer.get_mut(dst_offset..dst_offset + 4) {
                if swap_rb {
                    slot[0] = pixel[2];
                    slot[1] = pixel[1];
                    slot[2] = pixel[0];
                } else {
                    slot[0] = pixel[0];
                    slot[1] = pixel[1];
                    slot[2] = pixel[2];
                }
                slot[3] = 255;
            }
        }
    }
}

fn write_pixel(buffer: &mut [u8], offset: usize, rgb: [u8; 3]) {
    if let Some(slot) = buffer.get_mut(offset..offset + 4) {
        slot[0] = rgb[0];
        slot[1] = rgb[1];
        slot[2] = rgb[2];
        slot[3] = 255;
    }
}

/// BT.601 limited-range YUV to RGB.
///
/// The detector only compares frames against each other, so the exact matrix matters
/// less than using one consistently; 601 limited range is what every decoder here emits
/// for SDR 8-bit content.
fn yuv_to_rgb(y: u8, u: u8, v: u8) -> [u8; 3] {
    let luma = (f32::from(y) - 16.0) * 1.164_383;
    let chroma_u = f32::from(u) - 128.0;
    let chroma_v = f32::from(v) - 128.0;
    [
        (luma + 1.596_027 * chroma_v).clamp(0.0, 255.0) as u8,
        (luma - 0.391_762 * chroma_u - 0.812_968 * chroma_v).clamp(0.0, 255.0) as u8,
        (luma + 2.017_232 * chroma_u).clamp(0.0, 255.0) as u8,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use aria_core::{ColorSpace, CpuImage, Plane, Rect, Rotation};

    fn frame(format: PixelFormat, planes: Vec<Plane>, width: u32, height: u32) -> VideoFrame {
        VideoFrame {
            pts: RationalTime::new(0, aria_core::Rational::new(1, 1)),
            duration: None,
            format,
            color: ColorSpace::BT709,
            coded_size: (width, height),
            visible: Rect::from_size(width, height),
            rotation: Rotation::None,
            data: FrameData::Cpu(CpuImage::new(planes)),
        }
    }

    /// The class closed here is "a decoded frame the seam misreads becomes a silent
    /// absence of cuts". Every format the decoders emit is either converted or refused
    /// by name, and the conversion keeps a flat colour flat.
    #[test]
    fn every_cpu_pixel_format_is_converted_or_refused_by_name() {
        let mut buffer = vec![0_u8; SAMPLE_EDGE * SAMPLE_EDGE * 4];

        // Packed RGBA passes through; BGRA has its channels swapped back.
        let rgba = frame(
            PixelFormat::Rgba8,
            vec![Plane::new(vec![10, 20, 30, 255, 10, 20, 30, 255], 8, 1)],
            2,
            1,
        );
        assert_eq!(convert_into(&rgba, &mut buffer).unwrap(), (2, 1));
        assert_eq!(&buffer[0..4], &[10, 20, 30, 255]);

        let bgra = frame(
            PixelFormat::Bgra8,
            vec![Plane::new(vec![10, 20, 30, 255, 10, 20, 30, 255], 8, 1)],
            2,
            1,
        );
        convert_into(&bgra, &mut buffer).unwrap();
        assert_eq!(&buffer[0..4], &[30, 20, 10, 255]);

        // Neutral chroma with mid luma is grey, not a colour cast.
        let nv12 = frame(
            PixelFormat::Nv12,
            vec![
                Plane::new(vec![128, 128, 128, 128], 2, 2),
                Plane::new(vec![128, 128], 2, 1),
            ],
            2,
            2,
        );
        convert_into(&nv12, &mut buffer).unwrap();
        let grey = buffer[0];
        assert_eq!(&buffer[0..4], &[grey, grey, grey, 255], "chroma cast");

        let yuv420p = frame(
            PixelFormat::Yuv420p,
            vec![
                Plane::new(vec![128, 128, 128, 128], 2, 2),
                Plane::new(vec![128], 1, 1),
                Plane::new(vec![128], 1, 1),
            ],
            2,
            2,
        );
        convert_into(&yuv420p, &mut buffer).unwrap();
        assert_eq!(&buffer[0..4], &[grey, grey, grey, 255]);

        // 10-bit is refused with a reason rather than producing garbage.
        let p010 = frame(
            PixelFormat::P010,
            vec![Plane::new(vec![0; 8], 4, 2), Plane::new(vec![0; 4], 4, 1)],
            2,
            2,
        );
        let err = convert_into(&p010, &mut buffer).expect_err("10-bit was converted");
        assert!(err.contains("10-bit"), "{err}");
    }

    /// A frame larger than the sampling grid is reduced to it, so buffer size is a
    /// property of the detector rather than of whatever was imported.
    #[test]
    fn a_large_frame_is_downscaled_to_the_sampling_grid() {
        let mut buffer = vec![0_u8; SAMPLE_EDGE * SAMPLE_EDGE * 4];
        let width = SAMPLE_EDGE * 4;
        let height = SAMPLE_EDGE * 3;
        let large = frame(
            PixelFormat::Rgba8,
            vec![Plane::new(vec![7; width * height * 4], width * 4, height)],
            width as u32,
            height as u32,
        );
        assert_eq!(
            convert_into(&large, &mut buffer).unwrap(),
            (SAMPLE_EDGE, SAMPLE_EDGE)
        );
        assert_eq!(&buffer[0..4], &[7, 7, 7, 255]);
    }

    /// The production sampling loop skips isolated duplicate and out-of-order
    /// timestamps, then requests and analyzes later frames.
    #[test]
    fn pts_anomaly_tolerance_continues_detection() {
        let rate = aria_core::Rational::new(10, 1);
        let pts = [0, 5, 5, 10, 8, 15, 20];
        let mut source: std::collections::VecDeque<VideoFrame> = pts
            .into_iter()
            .map(|pts| {
                let mut sample = frame(
                    PixelFormat::Rgba8,
                    vec![Plane::new(vec![32, 32, 32, 255], 4, 1)],
                    1,
                    1,
                );
                sample.pts = RationalTime::new(pts, rate);
                sample
            })
            .collect();
        let calls = std::cell::Cell::new(0usize);

        let boundaries = detect_shots_from_source(
            rate,
            2.0,
            10.0,
            ShotDetectionConfig::default(),
            || false,
            |_| {
                calls.set(calls.get() + 1);
                Ok(source.pop_front())
            },
        )
        .expect("isolated timestamp anomalies are tolerated");

        assert!(boundaries.is_empty());
        assert_eq!(calls.get(), pts.len() + 1);
    }
}
