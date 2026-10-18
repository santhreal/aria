//! Long-form A/V regression: the export loop pushes one video frame and then
//! that frame's audio from a single thread, and the native encoder must keep
//! taking both for as long as the export runs.
//!
//! On Apple, `AVAssetWriter` gates each input's readiness on its interleaving
//! pattern, which wedged the loop after about 1 s of media ("encoder input
//! never became ready"). On Linux, `mp4mux` waits for a buffer on every pad
//! while both `appsrc`s block, so with no queue in front of the muxer the video
//! push blocked forever once AAC's coder delay exceeded the video lead. A wedge
//! shows up here as a missed deadline, not as a hung test run.

#![cfg(any(target_vendor = "apple", target_os = "linux"))]

use std::sync::mpsc;
use std::time::Duration;

use aria_core::{
    AudioEncoderConfig, ColorSpace, CpuImage, EncoderConfig, FrameData, PixelFormat, Plane,
    Rational, RationalTime, Rect, Rotation, VideoCodec, VideoEncoder, VideoFrame,
};
#[cfg(target_vendor = "apple")]
use aria_encoder::AvfEncoder as NativeEncoder;
#[cfg(target_os = "linux")]
use aria_encoder::GstEncoder as NativeEncoder;

/// Longest an interleaved encode below may take before it counts as wedged.
const DEADLINE: Duration = Duration::from_secs(120);

fn solid_rgba_frame(width: u32, height: u32, index: i64, rate: Rational) -> VideoFrame {
    let mut pixels = Vec::with_capacity((width * height * 4) as usize);
    for _ in 0..(width * height) {
        pixels.extend_from_slice(&[20, 120, 220, 255]);
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

/// This case's output: one fixed path per tag, overwritten on every run.
///
/// The tag separates concurrent cases; a timestamp would only add one more file
/// per run to a directory nothing empties.
fn temp_mp4(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("aria_enc_{tag}.mp4"))
}

/// Encode `frames` frames of `size` at `fps`, each followed by its stereo
/// audio, on a thread, and wait at most [`DEADLINE`] for the file to finish.
fn interleaved_export(tag: &str, (width, height): (u32, u32), fps: i32, frames: i64) {
    interleaved_export_in(tag, (width, height), fps, frames, VideoCodec::H264);
}

/// [`interleaved_export`] in `codec`.
fn interleaved_export_in(
    tag: &str,
    (width, height): (u32, u32),
    fps: i32,
    frames: i64,
    codec: VideoCodec,
) {
    let path = temp_mp4(tag);
    let out = path.clone();
    let (done, finished) = mpsc::channel();
    std::thread::spawn(move || {
        let rate = Rational::new(fps, 1);
        let audio_rate = 48_000u32;
        let config = EncoderConfig::new((width, height), rate, ColorSpace::BT709)
            .with_codec(codec)
            .with_audio(AudioEncoderConfig::new(audio_rate, 2));
        let mut encoder = NativeEncoder::open(&out, config).expect("open encoder");
        let per_frame = (audio_rate / fps as u32) as usize;
        let block: Vec<f32> = vec![0.1; per_frame * 2];
        let mut cursor = 0i64;
        for i in 0..frames {
            encoder
                .push(&solid_rgba_frame(width, height, i, rate))
                .unwrap_or_else(|e| panic!("push frame {i}: {e}"));
            let pts = RationalTime::new(cursor, Rational::new(audio_rate as i32, 1));
            encoder
                .push_audio(&block, pts)
                .unwrap_or_else(|e| panic!("push audio {i}: {e}"));
            cursor += per_frame as i64;
        }
        encoder.finish().expect("finish");
        let _ = done.send(());
    });
    finished.recv_timeout(DEADLINE).unwrap_or_else(|_| {
        panic!(
            "{frames} interleaved {codec:?} frames at {fps} fps did not finish within {DEADLINE:?}"
        )
    });

    let meta = std::fs::metadata(&path).expect("output exists");
    assert!(meta.len() > 0, "encoded file is empty");
    let probed = aria_decoder::probe(&path).expect("probe output");
    assert!(probed.has_audio, "output should carry an audio track");
    assert_eq!(probed.frame_count, frames, "all video frames should land");

    let _ = std::fs::remove_file(&path);
}

/// 10 s of interleaved video and audio at 30 fps.
#[test]
fn interleaved_av_export_sustains_ten_seconds() {
    interleaved_export("av300", (320, 240), 30, 300);
}

/// 3 s at 60 fps and 1280 by 720: 800 samples a frame, fewer than one AAC
/// frame, and a video frame larger than an `appsrc`'s default byte limit,
/// which is the shape of a 60 fps export.
#[test]
fn interleaved_av_export_at_sixty_fps_finishes() {
    interleaved_export("av180-60", (1280, 720), 60, 180);
}

/// One frame a second with audio, in every codec this host encodes. A video
/// coder that holds a lookahead before its first output (`x265enc` holds 15
/// frames or more) keeps the muxer waiting on the video pad for that many
/// seconds, while the audio branch fills with every second of AAC pushed in
/// the meantime. With the audio branch's queue missing, or bounded in time
/// below that delay, the audio source fills, the push of the next second of
/// audio blocks, and the frames the video coder waits for are never pushed.
/// Not caught by an encoder that emits each frame at once (`x264enc` is tuned
/// for zero latency), so on Linux the H.265 case carries this test and its
/// absence fails it.
#[test]
fn a_video_coder_delay_of_many_seconds_does_not_wedge_the_audio_branch() {
    let codecs = aria_encoder::available_codecs();
    #[cfg(target_os = "linux")]
    assert!(
        codecs.contains(&VideoCodec::H265),
        "this host encodes no H.265, the codec whose coder delay this test needs: {codecs:?}"
    );
    for codec in codecs {
        interleaved_export_in(&format!("lag-{codec:?}"), (128, 72), 1, 30, codec);
    }
}
