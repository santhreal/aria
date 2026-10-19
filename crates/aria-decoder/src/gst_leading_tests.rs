//! WHY: closes the class "a seek returns an open-GOP leading frame the
//! decoder concealed without flagging it corrupt". `vah264dec` (VA-API)
//! outputs the frames that show before a non-IDR sync sample with their
//! missing references concealed and no `CORRUPTED` flag, so the corrupt-frame
//! back-off never ran and a seek returned a frame sequential decode never
//! shows. The tests reproduce that decoder on any host by clearing the flag
//! on exactly those frames of the software decoder, and hold both ways a
//! caller reaches a frame after a seek: the raw landing and `frame_at`.
//!
//! Not covered: a decoder that conceals frames after the sync sample without
//! flagging them, a seek whose back-off ends at the start of the stream or
//! 64 s before its target on a leading frame (returned as it is), and GPU
//! frames.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use super::*;

fn fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/opengop_h264.mp4")
}

/// An FNV-1a hash of a frame's visible pixel bytes. Row padding is left out:
/// a decoder's buffer pool does not clear it.
fn pixels(frame: &VideoFrame) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    let image = frame.cpu().expect("Cpu mode yields CPU frames");
    let width = frame.visible.width as usize;
    let sample = frame.format.bytes_per_sample();
    for (index, plane) in image.planes.iter().enumerate() {
        let row = match frame.format {
            PixelFormat::Rgba8 | PixelFormat::Bgra8 => width * 4,
            PixelFormat::Yuv420p if index > 0 => width.div_ceil(2),
            _ if index > 0 => width.next_multiple_of(2) * sample,
            _ => width * sample,
        }
        .min(plane.stride);
        for line in plane.data.chunks(plane.stride.max(1)).take(plane.rows) {
            for byte in &line[..row.min(line.len())] {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
    }
    hash
}

/// Clear `CORRUPTED` on each frame `decoder`'s video decoder outputs before
/// the first keyframe it received since its last flush, the way `vah264dec`
/// outputs an open GOP's leading frames. The count of those frames.
fn conceal_leading_frames(decoder: &GstDecoder) -> Arc<AtomicU32> {
    let key = Arc::new(Mutex::new(None::<gst::ClockTime>));
    let leading = Arc::new(AtomicU32::new(0));
    let mut decoders = 0;
    let mut elements = decoder.pipeline.iterate_recurse();
    while let Ok(Some(element)) = elements.next() {
        let klass = element
            .factory()
            .and_then(|factory| {
                factory
                    .metadata(gst::ELEMENT_METADATA_KLASS)
                    .map(str::to_owned)
            })
            .unwrap_or_default();
        if !(klass.contains("Decoder") && klass.contains("Video")) {
            continue;
        }
        decoders += 1;
        let fed = Arc::clone(&key);
        element
            .static_pad("sink")
            .expect("video decoder sink pad")
            .add_probe(
                gst::PadProbeType::BUFFER
                    | gst::PadProbeType::EVENT_DOWNSTREAM
                    | gst::PadProbeType::EVENT_FLUSH,
                move |_, info| {
                    let mut fed = fed.lock().unwrap_or_else(PoisonError::into_inner);
                    match &info.data {
                        Some(gst::PadProbeData::Event(event))
                            if event.type_() == gst::EventType::FlushStop =>
                        {
                            *fed = None;
                        }
                        Some(gst::PadProbeData::Buffer(buffer))
                            if fed.is_none()
                                && !buffer.flags().contains(gst::BufferFlags::DELTA_UNIT) =>
                        {
                            *fed = buffer.pts();
                        }
                        _ => {}
                    }
                    gst::PadProbeReturn::Ok
                },
            );
        let (fed, count) = (Arc::clone(&key), Arc::clone(&leading));
        element
            .static_pad("src")
            .expect("video decoder src pad")
            .add_probe(gst::PadProbeType::BUFFER, move |_, info| {
                let key = *fed.lock().unwrap_or_else(PoisonError::into_inner);
                if let Some(gst::PadProbeData::Buffer(buffer)) = &mut info.data
                    && let (Some(pts), Some(key)) = (buffer.pts(), key)
                    && pts < key
                {
                    if buffer.flags().contains(gst::BufferFlags::CORRUPTED) {
                        buffer.make_mut().unset_flags(gst::BufferFlags::CORRUPTED);
                    }
                    count.fetch_add(1, Ordering::Relaxed);
                }
                gst::PadProbeReturn::Ok
            });
    }
    assert_eq!(decoders, 1, "the fixture plugs one video decoder");
    leading
}

/// Each frame of the fixture in decode order: its tick and pixels, and its
/// time, from a decoder that is never seeked.
fn sequential() -> (Vec<(i64, u64)>, Vec<RationalTime>) {
    let mut decoder = GstDecoder::open(&fixture(), OutputMode::Cpu).expect("open decoder");
    let rate = decoder.info().frame_rate;
    let mut frames = Vec::new();
    let mut times = Vec::new();
    while let Some(frame) = decoder.next_frame().expect("decode") {
        frames.push((aria_core::resample(frame.pts, rate).value, pixels(&frame)));
        times.push(frame.pts);
    }
    assert!(
        frames.len() > 200,
        "the fixture decoded {} frames",
        frames.len()
    );
    (frames, times)
}

fn concealing_decoder() -> (GstDecoder, Arc<AtomicU32>) {
    let decoder = GstDecoder::open(&fixture(), OutputMode::Cpu).expect("open decoder");
    let leading = conceal_leading_frames(&decoder);
    (decoder, leading)
}

fn id(decoder: &GstDecoder, frame: &VideoFrame) -> (i64, u64) {
    (
        aria_core::resample(frame.pts, decoder.info().frame_rate).value,
        pixels(frame),
    )
}

/// A seek to each frame, last first, lands on a frame sequential decode
/// returns at or before its target.
#[test]
fn a_seek_lands_on_a_sequential_frame_past_unflagged_leading_frames() {
    let (sequential, times) = sequential();
    let (mut decoder, leading) = concealing_decoder();
    let mut wrong = Vec::new();
    for (tick, &at) in times.iter().enumerate().rev() {
        decoder.seek(at).expect("seek");
        let landed = decoder
            .next_frame()
            .expect("decode")
            .map(|frame| id(&decoder, &frame));
        match landed.and_then(|id| sequential.iter().position(|seq| *seq == id)) {
            Some(index) if index <= tick => {}
            Some(index) => wrong.push(format!("seek {tick}: landed on {index}")),
            None => wrong.push(format!(
                "seek {tick}: landed on a frame sequential decode never returns"
            )),
        }
    }
    assert!(
        leading.load(Ordering::Relaxed) > 0,
        "no leading frame was output: the fixture no longer seeks into an open GOP"
    );
    assert!(
        wrong.is_empty(),
        "{} seeks landed wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

/// `frame_at` stepping backward, a seek per call, returns the frame
/// sequential decode returns at each time.
#[test]
fn frame_at_backward_returns_the_sequential_frame_past_unflagged_leading_frames() {
    let (sequential, times) = sequential();
    let (mut decoder, leading) = concealing_decoder();
    let mut wrong = Vec::new();
    for (tick, &at) in times.iter().enumerate().rev() {
        let got = decoder
            .frame_at(at)
            .expect("decode")
            .map(|frame| id(&decoder, &frame));
        if got != Some(sequential[tick]) {
            wrong.push(format!("tick {tick}: got {:?}", got.map(|(tick, _)| tick)));
        }
    }
    assert!(
        leading.load(Ordering::Relaxed) > 0,
        "no leading frame was output: the fixture no longer seeks into an open GOP"
    );
    assert!(
        wrong.is_empty(),
        "{} frames differ from sequential decode:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}
