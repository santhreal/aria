//! Hermetic round-trip: encode synthetic RGBA frames to an mp4 with the native
//! encoder, then decode the file back with the native decoder.

#![cfg(any(target_vendor = "apple", target_os = "windows", target_os = "linux"))]

use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane,
    Rational, RationalTime, Rect, Rotation, VideoDecoder, VideoEncoder, VideoFrame,
};
use aria_decoder::{OutputMode, open_audio_reader, probe};

#[cfg(target_vendor = "apple")]
use aria_decoder::AvfDecoder as NativeDecoder;
#[cfg(target_vendor = "apple")]
use aria_encoder::AvfEncoder as NativeEncoder;

#[cfg(target_os = "windows")]
use aria_decoder::WmfDecoder as NativeDecoder;
#[cfg(target_os = "windows")]
use aria_encoder::WmfEncoder as NativeEncoder;

#[cfg(target_os = "linux")]
use aria_decoder::GstDecoder as NativeDecoder;
#[cfg(target_os = "linux")]
use aria_encoder::GstEncoder as NativeEncoder;

/// This case's output: one fixed path per tag, overwritten on every run.
///
/// The tag separates concurrent cases; a timestamp would only add one more file
/// per run to a directory nothing empties.
fn temp_mp4(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria_enc_{tag}.mp4"))
}

fn solid_rgba_frame(width: u32, height: u32, index: i64, rate: Rational) -> VideoFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        pixels.extend_from_slice(&[20, 120, 220, 255]); // opaque blue, RGBA
    }
    let plane = Plane::new(pixels, width as usize * 4, height as usize);
    VideoFrame::new(
        RationalTime::new(index, rate),
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        (width, height),
        Rect::from_size(width, height),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![plane])),
    )
}

#[test]
fn encodes_an_mp4_that_decodes_back() {
    let (width, height) = (320u32, 240u32);
    let rate = Rational::FPS_30;
    let path = temp_mp4("roundtrip");

    let config = EncoderConfig::new((width, height), rate, ColorSpace::BT709);
    let mut encoder = NativeEncoder::open(&path, config).expect("open encoder");
    for i in 0..6 {
        encoder
            .push(&solid_rgba_frame(width, height, i, rate))
            .expect("push frame");
    }
    encoder.finish().expect("finish");

    let meta = std::fs::metadata(&path).expect("output exists");
    assert!(meta.len() > 0, "encoded file is empty");

    // Decode it back through the platform decoder.
    let mut decoder = NativeDecoder::open(&path, OutputMode::Cpu).expect("open decoder");
    assert_eq!(decoder.info().display_size, (width, height));
    let frame = decoder
        .next_frame()
        .expect("decode")
        .expect("at least one frame");
    assert_eq!((frame.width(), frame.height()), (width, height));

    let _ = std::fs::remove_file(&path);
}

/// The short-GOP config proxies use: every frame must still decode back, and
/// a mid-file `frame_at` must land (proxy scrubbing relies on dense sync
/// points making those seeks cheap).
#[test]
fn encodes_a_short_gop_mp4_that_seeks() {
    let (width, height) = (320u32, 240u32);
    let rate = Rational::FPS_30;
    let path = temp_mp4("shortgop");

    let config =
        EncoderConfig::new((width, height), rate, ColorSpace::BT709).with_keyframe_interval(15);
    let mut encoder = NativeEncoder::open(&path, config).expect("open encoder");
    let frames = 60;
    for i in 0..frames {
        encoder
            .push(&solid_rgba_frame(width, height, i, rate))
            .expect("push frame");
    }
    encoder.finish().expect("finish");

    let mut decoder = NativeDecoder::open(&path, OutputMode::Cpu).expect("open decoder");
    let frame = decoder
        .frame_at(RationalTime::new(45, rate))
        .expect("seek decode")
        .expect("frame at 45 exists");
    assert!(
        frame.pts.seconds() >= 44.0 / 30.0,
        "landed at {:?}, expected ~45/30s",
        frame.pts
    );

    let _ = std::fs::remove_file(&path);
}

/// One block of interleaved-stereo 440 Hz sine, `frames` sample-frames long,
/// starting at output sample `start`.
fn sine_block(start: i64, frames: usize, rate: u32) -> Vec<f32> {
    let mut out = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = (start + i as i64) as f32 / rate as f32;
        let s = (2.0 * std::f32::consts::PI * 440.0 * t).sin() * 0.5;
        out.push(s);
        out.push(s);
    }
    out
}

#[test]
fn encodes_an_mp4_with_an_aac_audio_track() {
    let (width, height) = (320u32, 240u32);
    let rate = Rational::FPS_30;
    let audio_rate = 48_000u32;
    let path = temp_mp4("audio_roundtrip");

    let config = EncoderConfig::new((width, height), rate, ColorSpace::BT709)
        .with_audio(AudioEncoderConfig::new(audio_rate, 2));
    let mut encoder = NativeEncoder::open(&path, config).expect("open encoder");

    // 12 frames @ 30 fps = 0.4 s; each frame owns 48000/30 = 1600 sample frames.
    let frames = 12i64;
    let per_frame = (audio_rate / 30) as usize;
    let mut cursor = 0i64;
    for i in 0..frames {
        encoder
            .push(&solid_rgba_frame(width, height, i, rate))
            .expect("push frame");
        let block = sine_block(cursor, per_frame, audio_rate);
        let pts = RationalTime::new(cursor, Rational::new(audio_rate as i32, 1));
        encoder.push_audio(&block, pts).expect("push audio");
        cursor += per_frame as i64;
    }
    encoder.finish().expect("finish");

    // The container now reports an audio track.
    let meta = probe(&path).expect("probe output");
    assert!(meta.has_audio, "exported file should carry an audio track");

    // Decode the audio back: AAC priming/padding shifts the exact count, so
    // assert we recover most of what we wrote (≈ frames * per_frame).
    let expected = frames as usize * per_frame;
    let mut reader = open_audio_reader(&path, audio_rate, 2).expect("open audio reader");
    let mut total_frames = 0usize;
    let mut buf = vec![0.0f32; 4096 * 2];
    loop {
        let got = reader.read(&mut buf).expect("read audio");
        if got == 0 {
            break;
        }
        total_frames += got;
    }
    assert!(
        total_frames as f64 > expected as f64 * 0.5,
        "recovered {total_frames} sample frames, expected ≈ {expected}"
    );

    let _ = std::fs::remove_file(&path);
}

/// A frame dark left of the center column and bright right of it, as RGBA or
/// as NV12 with rows `width` bytes long.
#[cfg(target_os = "linux")]
fn split_frame(format: PixelFormat, width: u32, height: u32, index: i64) -> VideoFrame {
    let (w, h) = (width as usize, height as usize);
    let side = |x: usize, dark: u8, bright: u8| if x < w / 2 { dark } else { bright };
    let planes = match format {
        PixelFormat::Rgba8 => {
            let row: Vec<u8> = (0..w)
                .flat_map(|x| {
                    let v = side(x, 16, 235);
                    [v, v, v, 255]
                })
                .collect();
            vec![Plane::new(row.repeat(h), w * 4, h)]
        }
        PixelFormat::Nv12 => {
            let row: Vec<u8> = (0..w).map(|x| side(x, 40, 200)).collect();
            vec![
                Plane::new(row.repeat(h), w, h),
                Plane::new(vec![128; w * h / 2], w, h / 2),
            ]
        }
        other => panic!("no split frame for {other:?}"),
    };
    VideoFrame::new(
        RationalTime::new(index, Rational::FPS_30),
        format,
        ColorSpace::BT709,
        (width, height),
        Rect::from_size(width, height),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(planes)),
    )
}

/// WHY: GStreamer's default NV12 layout rounds each row up to 4 bytes, and
/// the Linux encoder writes rows `width` bytes long. At a width 2 more than a
/// multiple of 4, such as 854 or 1366, `x264enc` failed every frame and an
/// encoder that maps the default layout read each row 2 bytes further along.
/// This encodes a frame split dark and bright at the center column, from
/// RGBA and from NV12 input, at 322x240, and checks every 8th decoded row
/// keeps the split at the center. Linux only: the macOS and Windows encoders
/// lay out their own input.
#[cfg(target_os = "linux")]
#[test]
fn encodes_a_width_two_more_than_a_multiple_of_four_without_shear() {
    let (width, height) = (322u32, 240u32);
    for format in [PixelFormat::Rgba8, PixelFormat::Nv12] {
        let path = temp_mp4(&format!("stride_{format:?}"));
        let config = EncoderConfig::new((width, height), Rational::FPS_30, ColorSpace::BT709);
        let mut encoder = NativeEncoder::open(&path, config).expect("open encoder");
        for i in 0..6 {
            encoder
                .push(&split_frame(format, width, height, i))
                .unwrap_or_else(|e| panic!("{format:?}: push frame {i}: {e}"));
        }
        encoder
            .finish()
            .unwrap_or_else(|e| panic!("{format:?}: finish: {e}"));

        let mut decoder = NativeDecoder::open(&path, OutputMode::Cpu).expect("open decoder");
        let frame = decoder
            .next_frame()
            .expect("decode")
            .expect("at least one frame");
        assert_eq!((frame.width(), frame.height()), (width, height));
        let luma = &frame.cpu().expect("a CPU frame").planes[0];
        let (dark_x, bright_x) = (width as usize / 4, width as usize * 3 / 4);
        for row in (0..height as usize).step_by(8).chain([height as usize - 1]) {
            let at = |x: usize| luma.data[row * luma.stride + x];
            assert!(
                at(dark_x) < 100 && at(bright_x) > 150,
                "{format:?}: row {row} reads {} left and {} right of the split",
                at(dark_x),
                at(bright_x)
            );
        }
        let _ = std::fs::remove_file(&path);
    }
}

/// Deterministic pseudo-random NV12 frame of `width` by `height` seeded by
/// `seed`, so every macroblock / CTU carries high spatial and temporal entropy.
#[cfg(target_os = "linux")]
fn noise_nv12_planes(width: u32, height: u32, seed: u64) -> Vec<Plane> {
    let (w, h) = (width as usize, height as usize);
    let mut state = seed.wrapping_mul(0xD1B5_4A32_D192_ED03) ^ 0xA5A5_5A5A_F0F0_0F0F;
    let mut y = vec![0u8; w * h];
    for chunk in y.as_chunks_mut::<8>().0 {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^= z >> 31;
        for (dst, byte) in chunk.iter_mut().zip(z.to_le_bytes()) {
            *dst = 16 + (u16::from(byte) * 219 / 255) as u8;
        }
    }
    vec![
        Plane::new(y, w, h),
        Plane::new(vec![128u8; w * (h / 2)], w, h / 2),
    ]
}

/// WHY: `--bitrate` and the Export sheet's Bitrate row (`4 Mb/s`, `8 Mb/s`,
/// `16 Mb/s`) claim a target video bitrate across every GStreamer candidate in
/// `candidates(codec)`. On `x265enc`, setting `bitrate` without VBV/CBR options
/// left x265 in unconstrained ABR from QP 22, writing >2x the requested rate
/// on high-entropy footage (8.4 Mb/s at 4 Mb/s, 16.1 Mb/s at 8 Mb/s); on
/// `nvh264enc`, `nvh265enc`, and `nvcudah264enc`, `rc-mode=cbr` /
/// `rate-control=cbr` without `strict-gop=true` overshot 4 and 8 Mb/s by
/// 20.5% to 30%.
///
/// Closes the class of candidate encoders whose rate controller ignores or
/// drifts past the requested target bitrate on a high-entropy source: derives
/// every candidate per codec at runtime from `GstEncoder::video_candidates`,
/// encodes 60 frames (2.0 s at 30 fps, 640x360) of uncorrelated NV12 noise at
/// 4, 8, and 16 Mb/s on every candidate that opens on this host, and asserts
/// the written file's bitrate lands within ±20% of the target.
#[cfg(target_os = "linux")]
#[test]
fn high_entropy_noise_exported_at_4_8_and_16_mbps_lands_within_twenty_percent_across_every_installed_encoder()
 {
    use aria_core::VideoCodec;

    let (width, height) = (640u32, 360u32);
    let rate = Rational::FPS_30;
    let frame_count = 60i64;
    let duration_secs = frame_count as f64 / 30.0;
    let mut pool: Vec<VideoFrame> = (0..frame_count as u64)
        .map(|seed| {
            VideoFrame::new(
                RationalTime::new(0, rate),
                PixelFormat::Nv12,
                ColorSpace::BT709,
                (width, height),
                Rect::from_size(width, height),
                Rotation::None,
                FrameData::Cpu(CpuImage::new(noise_nv12_planes(width, height, seed + 1))),
            )
        })
        .collect();

    let mut tested_encoders = Vec::new();
    let mut failures = Vec::new();

    for codec in VideoCodec::ALL {
        for &enc_name in NativeEncoder::video_candidates(codec) {
            let mut opened_any = false;
            for target_kbps in [4_000u32, 8_000, 16_000] {
                let path = temp_mp4(&format!("bitrate_{enc_name}_{target_kbps}"));
                let config = EncoderConfig::new((width, height), rate, ColorSpace::BT709)
                    .with_codec(codec)
                    .with_bitrate_kbps(target_kbps);
                let mut encoder =
                    match NativeEncoder::open_with_video_encoder(&path, config, enc_name) {
                        Ok(enc) => enc,
                        Err(aria_core::EncodeError::Unsupported(_)) => break,
                        Err(err) => panic!(
                            "{enc_name} ({codec:?}) failed to open at {target_kbps} kbps: {err}"
                        ),
                    };
                opened_any = true;
                assert_eq!(
                    encoder.choice().encoder,
                    enc_name,
                    "open_with_video_encoder did not select {enc_name}"
                );
                for i in 0..frame_count {
                    let frame = &mut pool[i as usize];
                    frame.pts = RationalTime::new(i, rate);
                    encoder
                        .push(frame)
                        .unwrap_or_else(|e| panic!("{enc_name} push frame {i}: {e}"));
                }
                encoder
                    .finish()
                    .unwrap_or_else(|e| panic!("{enc_name} finish at {target_kbps} kbps: {e}"));

                let bytes = std::fs::metadata(&path)
                    .unwrap_or_else(|e| panic!("{enc_name} output metadata: {e}"))
                    .len();
                let _ = std::fs::remove_file(&path);
                let actual_kbps = (bytes as f64 * 8.0) / (duration_secs * 1000.0);
                let ratio = actual_kbps / f64::from(target_kbps);
                let pct = (ratio - 1.0) * 100.0;
                eprintln!(
                    "[BitrateHonored] {enc_name} ({codec:?}) target={target_kbps} kbps -> actual={actual_kbps:.0} kbps ({pct:+.1}%)"
                );
                if !(0.80..=1.20).contains(&ratio) {
                    failures.push(format!(
                        "{enc_name} ({codec:?}) target={target_kbps} kbps wrote {actual_kbps:.0} kbps ({pct:+.1}%, outside ±20%)"
                    ));
                }
            }
            if opened_any {
                tested_encoders.push(enc_name);
            }
        }
    }

    assert!(
        tested_encoders.contains(&"x264enc") && tested_encoders.contains(&"x265enc"),
        "expected both software encoders to be tested; tested: {tested_encoders:?}"
    );
    assert!(
        failures.is_empty(),
        "encoders missed target bitrate by more than ±20%:\n  {}",
        failures.join("\n  ")
    );
}
