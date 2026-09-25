//! Integration tests that round-trip through real AVFoundation: synthesize a
//! tiny H.264 movie with `AVAssetWriter`, then decode it back. Hermetic — no
//! external asset needed.
//!
//! Declared from `apple.rs` with `#[path]`, so `use super::*` still means that
//! module and nothing about these cases changed in the move.

use super::*;
use core::cmp::Ordering;
use std::path::PathBuf;

use objc2::AllocAnyThread;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_av_foundation::{
    AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor, AVAssetWriterStatus,
    AVFileTypeQuickTimeMovie, AVMediaTypeVideo, AVVideoCodecKey, AVVideoCodecTypeH264,
    AVVideoColorPrimaries_SMPTE_C, AVVideoColorPrimariesKey, AVVideoColorPropertiesKey,
    AVVideoHeightKey, AVVideoTransferFunction_ITU_R_709_2, AVVideoTransferFunctionKey,
    AVVideoWidthKey, AVVideoYCbCrMatrix_ITU_R_601_4, AVVideoYCbCrMatrixKey,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMTime, CMTimeFlags};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferLockBaseAddress, CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString, NSURL};

/// This case's movie: one fixed path per tag, rewritten on every run.
///
/// A timestamp in the name meant one more file per run, forever, in a directory
/// nobody empties. The tag already separates concurrent cases, so the set of
/// paths this file can name is its own list of cases and never grows.
fn temp_movie(tag: &str) -> PathBuf {
    std::env::temp_dir().join(format!("aria_avf_{tag}.mov"))
}

/// Synthesize `frames` solid-gray BGRA frames at `fps` into an H.264 movie.
/// Exercises the same CoreMedia/CoreVideo stack the decoder reads back. When
/// `tag_601` is set, the file is tagged with SD/Rec.601 color properties so
/// the reader's color parsing can be checked against a non-default value.
#[allow(deprecated)] // finishWriting() (sync) is fine for a test fixture.
fn write_test_movie(path: &std::path::Path, w: u32, h: u32, frames: u32, fps: i32, tag_601: bool) {
    let _ = std::fs::remove_file(path); // AVAssetWriter refuses an existing file.

    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
    let file_type = unsafe { AVFileTypeQuickTimeMovie }.expect("AVFileTypeQuickTimeMovie");
    let writer = unsafe {
        AVAssetWriter::initWithURL_fileType_error(AVAssetWriter::alloc(), &url, file_type)
    }
    .expect("create writer");

    // Output settings: { codec: H.264, width, height }.
    let settings = NSMutableDictionary::<NSString, AnyObject>::new();
    let codec_key = unsafe { AVVideoCodecKey }.expect("AVVideoCodecKey");
    let codec_h264 = unsafe { AVVideoCodecTypeH264 }.expect("AVVideoCodecTypeH264");
    let width_key = unsafe { AVVideoWidthKey }.expect("AVVideoWidthKey");
    let height_key = unsafe { AVVideoHeightKey }.expect("AVVideoHeightKey");
    let width_num = NSNumber::numberWithInt(w as i32);
    let height_num = NSNumber::numberWithInt(h as i32);
    unsafe {
        settings.setObject_forKey(codec_h264, ProtocolObject::from_ref(codec_key));
        settings.setObject_forKey(&width_num, ProtocolObject::from_ref(width_key));
        settings.setObject_forKey(&height_num, ProtocolObject::from_ref(height_key));
    }

    // Optional SD/Rec.601 color tagging: SMPTE-C primaries, 709 transfer,
    // 601 matrix. AVFoundation requires all three together.
    if tag_601 {
        let color_props = NSMutableDictionary::<NSString, AnyObject>::new();
        let prim_key = unsafe { AVVideoColorPrimariesKey }.expect("AVVideoColorPrimariesKey");
        let prim_val =
            unsafe { AVVideoColorPrimaries_SMPTE_C }.expect("AVVideoColorPrimaries_SMPTE_C");
        let trc_key = unsafe { AVVideoTransferFunctionKey }.expect("AVVideoTransferFunctionKey");
        let trc_val = unsafe { AVVideoTransferFunction_ITU_R_709_2 }
            .expect("AVVideoTransferFunction_ITU_R_709_2");
        let mat_key = unsafe { AVVideoYCbCrMatrixKey }.expect("AVVideoYCbCrMatrixKey");
        let mat_val =
            unsafe { AVVideoYCbCrMatrix_ITU_R_601_4 }.expect("AVVideoYCbCrMatrix_ITU_R_601_4");
        unsafe {
            color_props.setObject_forKey(prim_val, ProtocolObject::from_ref(prim_key));
            color_props.setObject_forKey(trc_val, ProtocolObject::from_ref(trc_key));
            color_props.setObject_forKey(mat_val, ProtocolObject::from_ref(mat_key));
        }
        let props_key = unsafe { AVVideoColorPropertiesKey }.expect("AVVideoColorPropertiesKey");
        unsafe {
            settings.setObject_forKey(&color_props, ProtocolObject::from_ref(props_key));
        }
    }

    let media_video = unsafe { AVMediaTypeVideo }.expect("AVMediaTypeVideo");
    let input = unsafe {
        AVAssetWriterInput::initWithMediaType_outputSettings(
            AVAssetWriterInput::alloc(),
            media_video,
            Some(&settings),
        )
    };
    unsafe { input.setExpectsMediaDataInRealTime(false) };

    let adaptor = unsafe {
        AVAssetWriterInputPixelBufferAdaptor::initWithAssetWriterInput_sourcePixelBufferAttributes(
            AVAssetWriterInputPixelBufferAdaptor::alloc(),
            &input,
            None,
        )
    };

    assert!(
        unsafe { writer.canAddInput(&input) },
        "writer rejects input"
    );
    unsafe { writer.addInput(&input) };
    assert!(unsafe { writer.startWriting() }, "startWriting failed");
    unsafe { writer.startSessionAtSourceTime(cm_zero()) };

    for i in 0..frames {
        let pb = make_bgra_buffer(w, h);
        // Few frames + non-realtime: readiness is immediate, but be safe.
        let mut spins = 0;
        while !unsafe { input.isReadyForMoreMediaData() } && spins < 1000 {
            std::thread::sleep(std::time::Duration::from_millis(1));
            spins += 1;
        }
        let pts = CMTime {
            value: i as i64,
            timescale: fps,
            flags: CMTimeFlags::Valid,
            epoch: 0,
        };
        assert!(
            unsafe { adaptor.appendPixelBuffer_withPresentationTime(&pb, pts) },
            "append frame {i} failed"
        );
    }

    unsafe { input.markAsFinished() };
    assert!(unsafe { writer.finishWriting() }, "finishWriting failed");
    assert_eq!(
        unsafe { writer.status() },
        AVAssetWriterStatus::Completed,
        "writer error: {:?}",
        unsafe { writer.error() }.map(|e| e.localizedDescription().to_string())
    );
}

/// A solid-gray BGRA pixel buffer.
fn make_bgra_buffer(w: u32, h: u32) -> CFRetained<CVPixelBuffer> {
    let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
    let r = unsafe {
        CVPixelBufferCreate(
            None,
            w as usize,
            h as usize,
            FOURCC_BGRA,
            None,
            NonNull::from(&mut out),
        )
    };
    assert_eq!(r, 0, "CVPixelBufferCreate failed");
    let pb = unsafe { CFRetained::from_raw(NonNull::new(out).expect("null pixel buffer")) };

    unsafe {
        CVPixelBufferLockBaseAddress(&pb, CVPixelBufferLockFlags(0));
        let base = CVPixelBufferGetBaseAddress(&pb);
        let bytes_per_row = CVPixelBufferGetBytesPerRow(&pb);
        if let Some(base) = NonNull::new(base) {
            std::ptr::write_bytes(base.as_ptr() as *mut u8, 128, bytes_per_row * h as usize);
        }
        CVPixelBufferUnlockBaseAddress(&pb, CVPixelBufferLockFlags(0));
    }
    pb
}

#[test]
fn opens_probes_and_decodes_first_frame() {
    let path = temp_movie("decode");
    write_test_movie(&path, 320, 240, 6, 30, false);

    let mut dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    assert_eq!(dec.info().display_size, (320, 240));
    assert!(dec.info().frame_rate.as_f64() > 0.0);

    let frame = dec.next_frame().expect("decode").expect("first frame");
    assert_eq!(frame.width(), 320);
    assert_eq!(frame.height(), 240);
    assert!(frame.is_cpu());
    // H.264 decodes to biplanar NV12 on Apple.
    assert_eq!(frame.format, PixelFormat::Nv12);
    let img = frame.cpu().expect("cpu image");
    assert_eq!(img.planes.len(), 2);
    assert!(img.planes.iter().all(Plane::is_well_formed));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn sequential_decode_advances_pts() {
    let path = temp_movie("seq");
    write_test_movie(&path, 160, 120, 8, 30, false);

    let mut dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    let f0 = dec.next_frame().expect("decode").expect("frame 0");
    let f1 = dec.next_frame().expect("decode").expect("frame 1");
    assert_eq!(f0.pts.compare(f1.pts), Ordering::Less, "pts must advance");

    let _ = std::fs::remove_file(&path);
}

#[test]
fn frame_at_lands_at_or_after_target() {
    let path = temp_movie("seek");
    write_test_movie(&path, 160, 120, 30, 30, false);

    let mut dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    // ~frame 10 (10/30 s), expressed at a *different* rate to exercise the
    // rate-agnostic comparison.
    let target = RationalTime::new(10, Rational::FPS_30);
    let frame = dec.frame_at(target).expect("frame_at").expect("frame");
    assert_ne!(
        frame.pts.compare(target),
        Ordering::Less,
        "decoded frame must be at or after the seek target"
    );

    let _ = std::fs::remove_file(&path);
}

/// Sequential `frame_at` targets (the playback/scrub pattern) must land on
/// the same frames as a fresh seek per target — the roll-forward fast path
/// may change cost, never results.
#[test]
fn frame_at_roll_forward_matches_seek_per_target() {
    let path = temp_movie("roll");
    write_test_movie(&path, 160, 120, 30, 30, false);

    let mut rolled = AvfDecoder::open(&path, OutputMode::Cpu).expect("open rolled");
    let mut seeked = AvfDecoder::open(&path, OutputMode::Cpu).expect("open seeked");

    for value in [0i64, 2, 3, 7, 8, 15, 22, 29] {
        let target = RationalTime::new(value, Rational::FPS_30);
        let a = rolled.frame_at(target).expect("frame_at").expect("frame");
        // Byte-identical reference: explicit seek + walk on the other decoder.
        seeked.seek(target).expect("seek");
        let b = loop {
            let f = seeked.next_frame().expect("decode").expect("frame");
            if f.pts.compare(target) != Ordering::Less {
                break f;
            }
        };
        assert_eq!(a.pts, b.pts, "target {value}");
    }

    let _ = std::fs::remove_file(&path);
}

/// Backward and repeated targets fall off the roll-forward path and must
/// still land correctly (via a real seek).
#[test]
fn frame_at_handles_backward_and_repeated_targets() {
    let path = temp_movie("back");
    write_test_movie(&path, 160, 120, 30, 30, false);

    let mut dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    for value in [20i64, 5, 5, 12, 0] {
        let target = RationalTime::new(value, Rational::FPS_30);
        let frame = dec.frame_at(target).expect("frame_at").expect("frame");
        assert_ne!(frame.pts.compare(target), Ordering::Less, "target {value}");
        // Solid-color 30fps fixture: frame_at must land exactly on `value`.
        assert_eq!(
            aria_core::resample(frame.pts, Rational::FPS_30).value,
            value
        );
    }

    // Rolling past the end of the stream reports end-of-stream, and the
    // decoder recovers with a later backward target.
    let last = RationalTime::new(29, Rational::FPS_30);
    dec.frame_at(last).expect("frame_at").expect("last frame");
    assert!(
        dec.frame_at(RationalTime::new(31, Rational::FPS_30))
            .expect("frame_at")
            .is_none(),
        "past-the-end target must be None"
    );
    let recovered = dec
        .frame_at(RationalTime::new(3, Rational::FPS_30))
        .expect("frame_at")
        .expect("frame");
    assert_eq!(
        aria_core::resample(recovered.pts, Rational::FPS_30).value,
        3
    );

    let _ = std::fs::remove_file(&path);
}

/// Sustained seek churn — many reader rebuilds in a tight loop — must not
/// degrade the decoder. Guards the `cancelReading` lifecycle: without it,
/// each rebuild leaks an in-flight decode pipeline, and enough of those
/// exhaust process-wide decode resources ("Cannot Decode" on every
/// subsequent read; reproduced with ~300 rebuilds against a 4K source).
/// The fixture is too small to trip the ceiling itself, so this is a
/// smoke test of the cancel path plus decoder health after churn.
#[test]
fn seek_churn_keeps_decoder_healthy() {
    let path = temp_movie("churn");
    write_test_movie(&path, 160, 120, 30, 30, false);

    let mut dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    for i in 0..150 {
        let target = RationalTime::new((i * 7) % 30, Rational::FPS_30);
        dec.seek(target).expect("seek");
        dec.next_frame().expect("decode").expect("frame");
    }
    // Still healthy: an exact frame_at keeps landing correctly.
    let frame = dec
        .frame_at(RationalTime::new(11, Rational::FPS_30))
        .expect("frame_at")
        .expect("frame");
    assert_eq!(aria_core::resample(frame.pts, Rational::FPS_30).value, 11);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn gpu_mode_hands_back_a_live_pixel_buffer() {
    let path = temp_movie("gpu");
    write_test_movie(&path, 320, 240, 6, 30, false);

    let mut dec = AvfDecoder::open(&path, OutputMode::Gpu).expect("open");
    let frame = dec.next_frame().expect("decode").expect("first frame");

    assert!(frame.is_gpu(), "gpu mode must yield a GPU frame");
    assert_eq!(frame.width(), 320);
    assert_eq!(frame.height(), 240);

    let surface = frame.gpu().expect("gpu surface");
    assert_eq!(surface.kind(), GpuSurfaceKind::CoreVideoPixelBuffer);
    assert_ne!(surface.handle(), 0, "handle must be a real pointer");

    // The handle must be a usable CVPixelBuffer kept alive by the frame's
    // retained wrapper: reinterpret it and read its dimensions back.
    let pb: &CVPixelBuffer =
        unsafe { (surface.handle() as *const CVPixelBuffer).as_ref() }.expect("live buffer");
    assert_eq!(CVPixelBufferGetWidth(pb), 320);
    assert_eq!(CVPixelBufferGetHeight(pb), 240);

    let _ = std::fs::remove_file(&path);
}

/// Manual benchmark, not a correctness test: per-target latency of
/// sequential `frame_at` (roll-forward) vs a seek per target. Run with
/// `cargo test -p aria-decoder --release -- --ignored bench_roll --nocapture`.
#[test]
#[ignore = "manual benchmark"]
fn bench_roll_forward_vs_seek_per_target() {
    use std::time::Instant;

    let path = temp_movie("bench");
    let frames = 300; // 10s @ 30fps
    write_test_movie(&path, 320, 180, frames, 30, false);

    let targets: Vec<RationalTime> = (0..frames as i64)
        .map(|i| RationalTime::new(i, Rational::FPS_30))
        .collect();

    let mut rolled = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    let start = Instant::now();
    for &t in &targets {
        rolled.frame_at(t).expect("frame_at").expect("frame");
    }
    let roll_ms = start.elapsed().as_secs_f64() * 1000.0 / targets.len() as f64;

    let mut seeked = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    let start = Instant::now();
    for &t in &targets {
        seeked.seek(t).expect("seek");
        loop {
            let f = seeked.next_frame().expect("decode").expect("frame");
            if f.pts.compare(t) != Ordering::Less {
                break;
            }
        }
    }
    let seek_ms = start.elapsed().as_secs_f64() * 1000.0 / targets.len() as f64;

    println!(
        "sequential playback, {frames} frames: frame_at {roll_ms:.3} ms/frame, \
         seek-per-frame {seek_ms:.3} ms/frame ({:.1}x)",
        seek_ms / roll_ms
    );

    let _ = std::fs::remove_file(&path);
}

/// Open-GOP H.264 fixture (`tests/fixtures/opengop_h264.mp4`; 320×180,
/// 24 fps, 10 s, x264 `open_gop=1` with the reference structure of a
/// real-world 4K export that hard-failed every seek): sync samples every
/// 2 s, only frame 0 IDR, and the sync samples at 4 s and 8 s carry three
/// leading B-frames referencing the previous GOP — the start points
/// VideoToolbox refuses with `AVErrorDecodeFailed` ("Cannot Decode").
fn opengop_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opengop_h264.mp4")
}

const OPENGOP_FPS: Rational = Rational::new(24, 1);

/// Open-GOP recovery cases, driven by the fixture above.
#[path = "apple_opengop_tests.rs"]
mod opengop;

#[test]
fn hermetic_movie_reports_no_audio_track() {
    let path = temp_movie("noaudio");
    write_test_movie(&path, 160, 120, 4, 30, false);
    let dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    assert!(!dec.has_audio_track(), "video-only fixture");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn probes_color_metadata_from_format_description() {
    let path = temp_movie("color");
    // Tag the file SD/Rec.601 — distinct from the BT.709 default, so a pass
    // proves the tags were actually read (not just defaulted).
    write_test_movie(&path, 160, 120, 4, 30, true);

    let dec = AvfDecoder::open(&path, OutputMode::Cpu).expect("open");
    let color = dec.info().color;
    assert_eq!(color.matrix, MatrixCoefficients::Bt601, "matrix from tag");
    assert_eq!(
        color.primaries,
        ColorPrimaries::Bt601_525,
        "SMPTE-C primaries from tag"
    );
    assert_eq!(color.transfer, TransferFunction::Bt709, "transfer from tag");
    assert!(!color.is_hdr());

    let _ = std::fs::remove_file(&path);
}
