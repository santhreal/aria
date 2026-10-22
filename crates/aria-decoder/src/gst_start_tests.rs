//! Tests for [`super`]: opening a decode pipeline at a start position.
//!
//! The class this closes: a decoder opened for a frame past stream start
//! decoding frames at stream start for its first seek to flush. The
//! invariants held here: a held pipeline does not preroll until released; a
//! pipeline opened at a start inside the stream prerolls at the sync sample
//! at or before it, with no frame before that sync sample decoded; the first
//! seek to the start flushes nothing and returns the frame a seek there
//! returns, and is not measured as a seek; a later seek to the start seeks
//! again; a start outside the stream opens at stream start. Each runs on a real MP4 with H.264 and AAC, with and
//! without a B-frame edit list.
//!
//! Not covered: a demuxer other than `qtdemux`, a DMABuf pipeline (the stop
//! of a starved attempt is covered in `gst_gpu_tests`), and the decode time
//! saved, which the `scrub` bench scenario measures.

use std::sync::atomic::{AtomicUsize, Ordering};

use aria_core::{FrameData, Rational, RationalTime, VideoDecoder, VideoFrame};
use gstreamer_app as gst_app;

use super::*;
use crate::gst_test_media::h264_aac_mp4;
use crate::{GstDecoder, OutputMode, SeekStats};

/// How long a test waits for a pipeline that should preroll.
const PREROLL: Duration = Duration::from_secs(10);

fn ms(n: u64) -> gst::ClockTime {
    gst::ClockTime::from_mseconds(n)
}

/// Frame `n` of the 60 fps fixtures.
fn frame(n: i64) -> RationalTime {
    RationalTime::new(n, Rational::new(60, 1))
}

fn clock(time: RationalTime) -> gst::ClockTime {
    gst::ClockTime::from_nseconds(time.in_units(crate::gst::NS_PER_SEC) as u64)
}

fn shown_ms(frame: &VideoFrame) -> u64 {
    (frame.pts.seconds() * 1000.0).round() as u64
}

fn pixels(frame: &VideoFrame) -> Vec<&[u8]> {
    match &frame.data {
        FrameData::Cpu(image) => image.planes.iter().map(|p| p.data.as_slice()).collect(),
        FrameData::Gpu(_) => panic!("a CPU open returned a GPU frame"),
    }
}

/// The times of the frames each video decoder `bin` plugs outputs.
fn decoded_times(bin: &gst::Bin) -> Arc<Mutex<Vec<gst::ClockTime>>> {
    let times = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&times);
    bin.connect_deep_element_added(move |_, _, element| {
        if !element
            .factory()
            .is_some_and(|factory| crate::gst::is_video_decoder(&factory))
        {
            return;
        }
        let Some(pad) = element.static_pad("src") else {
            return;
        };
        let seen = Arc::clone(&seen);
        pad.add_probe(gst::PadProbeType::BUFFER, move |_, info| {
            if let Some(gst::PadProbeData::Buffer(buffer)) = &info.data
                && let Some(pts) = buffer.pts()
            {
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(pts);
            }
            gst::PadProbeReturn::Ok
        });
    });
    times
}

/// The number of flushes that reach `decoder`'s sink from now on: one per
/// seek the pipeline takes.
fn flushes(decoder: &GstDecoder) -> Arc<AtomicUsize> {
    let count = Arc::new(AtomicUsize::new(0));
    let seen = Arc::clone(&count);
    let pad = decoder
        .pipeline()
        .by_name("out")
        .and_then(|sink| sink.static_pad("sink"))
        .expect("the decode pipeline has its appsink");
    pad.add_probe(gst::PadProbeType::EVENT_FLUSH, move |_, info| {
        if let Some(gst::PadProbeData::Event(event)) = &info.data
            && event.type_() == gst::EventType::FlushStart
        {
            seen.fetch_add(1, Ordering::Relaxed);
        }
        gst::PadProbeReturn::Ok
    });
    count
}

/// A pipeline built as `try_open` builds one, with a hold installed, paused.
fn held_pipeline(
    mp4: &std::path::Path,
) -> (
    crate::gst::PipelineGuard,
    gst_app::AppSink,
    Arc<KeySnap>,
    Arc<StartHold>,
    Arc<Mutex<Vec<gst::ClockTime>>>,
) {
    let uri = crate::gst::path_uri(mp4).expect("fixture uri");
    let (pipeline, bin, sink) = crate::gst_gpu::build_decode_pipeline(uri.as_str(), false)
        .expect("the decode pipeline builds");
    let snap = Arc::new(KeySnap::default());
    crate::gst_key_snap::snap_video_to_keyframes(&bin, &snap);
    let decoded = decoded_times(&bin);
    let hold = StartHold::install(&bin);
    assert_eq!(
        pipeline.set_state(gst::State::Paused),
        Ok(gst::StateChangeSuccess::Async)
    );
    (pipeline, sink, snap, hold, decoded)
}

fn preroll_pts(sink: &gst_app::AppSink) -> Option<gst::ClockTime> {
    sink.try_pull_preroll(gst::ClockTime::ZERO)
        .and_then(|sample| sample.buffer().and_then(|buffer| buffer.pts()))
}

#[test]
fn a_held_pipeline_prerolls_only_once_released() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path(), 0);
    let (pipeline, sink, _snap, hold, decoded) = held_pipeline(&mp4);
    let pad = hold
        .wait_reached(PREROLL, || false)
        .expect("a buffer reached a held decoder");
    let decoder = pad.parent_element().expect("a held pad has its decoder");
    assert!(
        decoder
            .factory()
            .is_some_and(|factory| crate::gst::is_video_decoder(&factory)),
        "the hold stopped {} instead of a video decoder",
        decoder.name()
    );
    let (held, ..) = pipeline.state(gst::ClockTime::from_mseconds(300));
    assert_eq!(held, Ok(gst::StateChangeSuccess::Async));
    assert_eq!(preroll_pts(&sink), None, "a held pipeline prerolled");
    assert!(
        decoded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .is_empty(),
        "a held decoder decoded a frame"
    );

    hold.release();
    let (released, ..) = pipeline.state(gst::ClockTime::from_seconds(PREROLL.as_secs()));
    assert_eq!(released, Ok(gst::StateChangeSuccess::Success));
    assert_eq!(preroll_pts(&sink), Some(gst::ClockTime::ZERO));
}

#[test]
fn a_held_seek_decodes_nothing_before_the_sync_sample() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path(), 0);
    for (start, key) in [(frame(90), 1000), (frame(239), 3000)] {
        let (pipeline, sink, snap, hold, decoded) = held_pipeline(&mp4);
        let at = seek_held(&hold, &snap, clock(start), PREROLL, || false);
        assert_eq!(at, clock(start));
        let (prerolled, ..) = pipeline.state(gst::ClockTime::from_seconds(PREROLL.as_secs()));
        assert_eq!(prerolled, Ok(gst::StateChangeSuccess::Success));
        assert_eq!(preroll_pts(&sink), Some(ms(key)));
        let decoded = decoded
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        assert_eq!(
            decoded.first(),
            Some(&ms(key)),
            "the decoder output {decoded:?} for a start at {start:?}"
        );
        assert!(
            decoded.iter().all(|pts| *pts >= ms(key)),
            "the decoder output {decoded:?} before the keyframe at {key} ms"
        );
    }
}

#[test]
fn a_pipeline_opened_inside_the_stream_prerolls_at_the_sync_sample_before_its_start() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    for bframes in [0, 2] {
        let mp4 = h264_aac_mp4(dir.path(), bframes);
        for (start, key) in [
            (frame(60), 1000),
            (frame(90), 1000),
            (frame(120), 2000),
            (frame(239), 3000),
        ] {
            let opened = crate::gst_gpu::open_pipeline(&mp4, OutputMode::Cpu, clock(start))
                .expect("open the fixture");
            let _ = opened.pipeline.set_state(gst::State::Null);
            assert_eq!(
                opened.start,
                clock(start),
                "bframes={bframes}: the open at {start:?} did not preroll there"
            );
            // With B-frames the edit list shifts buffer times two frame
            // periods past the keyframe's presentation time.
            let fed = opened
                .snap
                .first_fed()
                .expect("a buffer reached the decoder after the start seek")
                .mseconds();
            assert!(
                (key..=key + 100).contains(&fed),
                "bframes={bframes}: the decoder started at {fed} ms for a start at {start:?}, not the keyframe at {key} ms"
            );
        }
    }
}

#[test]
fn a_start_outside_the_stream_opens_at_stream_start() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path(), 0);
    for start in [gst::ClockTime::ZERO, gst::ClockTime::from_seconds(10)] {
        let opened =
            crate::gst_gpu::open_pipeline(&mp4, OutputMode::Cpu, start).expect("open the fixture");
        let _ = opened.pipeline.set_state(gst::State::Null);
        assert_eq!(opened.start, gst::ClockTime::ZERO, "the open at {start}");
        assert_eq!(opened.snap.first_fed(), None, "the open at {start} seeked");
    }
    let mut decoder = GstDecoder::open_at(&mp4, OutputMode::Cpu, frame(600)).expect("open");
    let first = decoder.next_frame().expect("decode").expect("a frame");
    assert_eq!(shown_ms(&first), 0);
    // The preroll at stream start does not serve a seek to the start.
    let mut decoder = GstDecoder::open_at(&mp4, OutputMode::Cpu, frame(600)).expect("open");
    let flushed = flushes(&decoder);
    let _ = decoder.seek(frame(600));
    assert_eq!(flushed.load(Ordering::Relaxed), 1);
}

#[test]
fn a_decoder_opened_at_a_start_returns_the_frame_a_seek_there_returns_without_seeking() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    for bframes in [0, 2] {
        let mp4 = h264_aac_mp4(dir.path(), bframes);
        for start in [frame(60), frame(90), frame(120), frame(239)] {
            let mut seeked = GstDecoder::open(&mp4, OutputMode::Cpu).expect("open");
            let want = seeked
                .frame_at(start)
                .expect("decode")
                .expect("a frame at the start");
            let mut opened = GstDecoder::open_at(&mp4, OutputMode::Cpu, start).expect("open");
            let flushed = flushes(&opened);
            let got = opened
                .frame_at(start)
                .expect("decode")
                .expect("a frame at the start");
            assert_eq!(
                flushed.load(Ordering::Relaxed),
                0,
                "bframes={bframes}: the first seek to the start at {start:?} seeked"
            );
            assert_eq!(got.pts, want.pts, "bframes={bframes}, start {start:?}");
            assert!(
                pixels(&got) == pixels(&want),
                "bframes={bframes}: the frame at {start:?} decoded different pixels"
            );
        }
    }
}

/// A frame pulled from the preroll, through a seek or without one, moves the
/// pipeline past it, so a seek to the start afterwards seeks.
#[test]
fn a_seek_to_the_start_after_a_frame_is_pulled_seeks_again() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path(), 0);
    let start = frame(120);
    for seek_first in [true, false] {
        let mut decoder = GstDecoder::open_at(&mp4, OutputMode::Cpu, start).expect("open");
        let flushed = flushes(&decoder);
        if seek_first {
            decoder.seek(start).expect("seek");
        }
        for want in [2000, 2017] {
            let shown = decoder.next_frame().expect("decode").expect("a frame");
            assert_eq!(shown_ms(&shown), want, "seek first: {seek_first}");
        }
        assert_eq!(
            flushed.load(Ordering::Relaxed),
            0,
            "seek first: {seek_first}"
        );
        decoder.seek(start).expect("seek");
        let shown = decoder.next_frame().expect("decode").expect("a frame");
        assert_eq!(shown_ms(&shown), 2000, "seek first: {seek_first}");
        assert_eq!(
            flushed.load(Ordering::Relaxed),
            1,
            "seek first: {seek_first}"
        );
    }
}

/// A seek the preroll serves costs nothing at request time. Measured as a
/// seek or a snap, it shuts the roll windows, and each later short step
/// forward seeks instead of rolling.
#[test]
fn a_seek_the_preroll_serves_is_not_measured() {
    let dir = tempfile::tempdir().expect("a scratch directory");
    let mp4 = h264_aac_mp4(dir.path(), 0);
    let start = frame(120);
    for nearest in [false, true] {
        let ask = |decoder: &mut GstDecoder| {
            if nearest {
                decoder.frame_at_nearest(start)
            } else {
                decoder.frame_at(start)
            }
            .expect("decode")
            .expect("a frame at the start")
        };
        let mut opened = GstDecoder::open_at(&mp4, OutputMode::Cpu, start).expect("open");
        ask(&mut opened);
        assert_eq!(
            opened.seek_stats(),
            SeekStats::default(),
            "nearest: {nearest}: the preroll was measured as a seek"
        );
        let mut seeked = GstDecoder::open(&mp4, OutputMode::Cpu).expect("open");
        ask(&mut seeked);
        assert_ne!(
            seeked.seek_stats(),
            SeekStats::default(),
            "nearest: {nearest}: a seek was not measured"
        );
    }
}
