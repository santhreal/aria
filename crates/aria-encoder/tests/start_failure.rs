//! An encoder that cannot start on the export's frames reports the encoder and
//! the frame size, not a fault of the output path or the disk.

#![cfg(target_os = "linux")]

use aria_core::{
    ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane, Rational, RationalTime,
    Rect, Rotation, VideoCodec, VideoEncoder, VideoFrame,
};
use aria_encoder::GstEncoder;

/// Wider than `x264enc` and `x265enc` encode: both take at most 16384 pixels a
/// side, and `x265enc` none this thin at that width.
const PAST_THE_SOFTWARE_ENCODERS: (u32, u32) = (16386, 16);

const RATE: Rational = Rational::FPS_30;

fn grey((width, height): (u32, u32)) -> VideoFrame {
    let pixels = vec![128u8; (width * height * 4) as usize];
    VideoFrame::new(
        RationalTime::new(0, RATE),
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        (width, height),
        Rect::from_size(width, height),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![Plane::new(
            pixels,
            width as usize * 4,
            height as usize,
        )])),
    )
}

/// WHY: closes the class of an encoder that opens and then cannot start on the
/// export's frames failing with advice to check the output path and the free
/// disk space, which points at the disk for a frame size the encoder rejects.
/// Each codec's software encoder, the last of
/// [`GstEncoder::video_candidates`], is forced at a size it rejects, and the
/// export must fail naming that encoder and the size, with no disk advice.
///
/// What this does not catch: a hardware encoder's failure to start, which
/// moves the export to the next candidate (see `session_limit`), and an
/// encoder error after the encoder coded a frame, which keeps the disk advice.
#[test]
fn an_encoder_that_cannot_start_on_the_frame_size_reports_it() {
    let (width, height) = PAST_THE_SOFTWARE_ENCODERS;
    for codec in VideoCodec::ALL {
        let software = *GstEncoder::video_candidates(codec)
            .last()
            .expect("every codec has a software encoder");
        let path = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
            .join(format!("start-failure-{software}.mp4"));
        let mut config = EncoderConfig::new(PAST_THE_SOFTWARE_ENCODERS, RATE, ColorSpace::BT709);
        config.video_codec = codec;
        let mut encoder = GstEncoder::open_with_video_encoder(&path, config, software)
            .unwrap_or_else(|err| panic!("{software} opens: {err}"));
        let started = std::time::Instant::now();
        let err = encoder
            .push(&grey(PAST_THE_SOFTWARE_ENCODERS))
            .and_then(|()| encoder.finish())
            .expect_err("the export fails");
        assert!(
            started.elapsed() < std::time::Duration::from_secs(20),
            "{software} took {:?} to fail",
            started.elapsed()
        );
        let text = err.to_string();
        let opening =
            format!("encode failed: {software} could not start encoding {width}x{height} frames: ");
        assert!(text.starts_with(&opening), "{software}: {text}");
        assert!(
            text.ends_with(". Export at a smaller size or in the other codec."),
            "{software}: {text}"
        );
        assert!(!text.contains("disk"), "{software}: {text}");
        let _ = std::fs::remove_file(&path);
    }
}
