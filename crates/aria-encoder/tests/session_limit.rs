//! An export whose hardware encoder opens and then gets no encode session
//! moves to the next encoder instead of failing.

#![cfg(target_os = "linux")]

use std::path::{Path, PathBuf};

use aria_core::{
    ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane, Rational, RationalTime,
    Rect, Rotation, VideoDecoder, VideoEncoder, VideoFrame,
};
use aria_decoder::{GstDecoder, OutputMode};
use aria_encoder::GstEncoder;

const SIZE: (u32, u32) = (256, 144);
const RATE: Rational = Rational::FPS_30;

fn grey(index: i64) -> VideoFrame {
    let (width, height) = SIZE;
    let pixels = vec![128u8; (width * height * 4) as usize];
    VideoFrame::new(
        RationalTime::new(index, RATE),
        PixelFormat::Rgba8,
        ColorSpace::BT709,
        SIZE,
        Rect::from_size(width, height),
        Rotation::None,
        FrameData::Cpu(CpuImage::new(vec![Plane::new(
            pixels,
            width as usize * 4,
            height as usize,
        )])),
    )
}

fn config() -> EncoderConfig {
    EncoderConfig::new(SIZE, RATE, ColorSpace::BT709)
}

/// A second of frames encoded to `path`, and the encoder that took them.
fn export(path: &Path) -> Result<aria_core::EncoderChoice, aria_core::EncodeError> {
    let mut encoder = GstEncoder::open(path, config())?;
    for index in 0..30 {
        encoder.push(&grey(index))?;
    }
    encoder.finish()?;
    Ok(encoder.choice())
}

/// WHY: closes the class of an export failing outright when the device
/// refuses the hardware encode session after the encoder element opened, as
/// NVENC does at its concurrent-session limit and a CUDA device does under
/// load from other processes. Hardware sessions are held open one more at a
/// time, and after each an export must finish with a file that decodes, until
/// an export no longer gets a hardware session.
///
/// What this does not catch: a refusal after the first frame. A host without a
/// hardware H.264 encoder holds no session and checks one software export.
#[test]
fn an_export_past_the_hardware_session_limit_finishes_on_the_next_encoder() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("session-limit");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    let mut held = Vec::new();
    for n in 0..64 {
        let path = dir.join(format!("export-{n}.mp4"));
        let choice = export(&path)
            .unwrap_or_else(|err| panic!("the export beside {n} held sessions failed: {err}"));
        let mut decoder = GstDecoder::open(&path, OutputMode::Cpu).expect("the export opens");
        let frame = decoder
            .next_frame()
            .expect("the export decodes")
            .expect("the export holds a frame");
        assert_eq!((frame.width(), frame.height()), SIZE);
        drop(decoder);
        if choice.hardware != Some(true) {
            return;
        }
        let mut holder = GstEncoder::open(&dir.join(format!("held-{n}.mp4")), config())
            .expect("an encoder opens");
        holder
            .push(&grey(0))
            .expect("the held session takes a frame");
        held.push(holder);
    }
    panic!("64 hardware sessions held and an export still got one");
}
