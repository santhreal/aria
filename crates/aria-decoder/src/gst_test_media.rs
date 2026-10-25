//! Synthetic MP4 files the GStreamer decode tests open.

use std::path::{Path, PathBuf};

use gstreamer as gst;
use gstreamer::prelude::*;

/// The AAC encoder this host has; a missing one is a hard failure, as for
/// every other plugin the product needs.
fn aac_encoder() -> &'static str {
    ["avenc_aac", "voaacenc", "fdkaacenc"]
        .into_iter()
        .find(|name| gst::ElementFactory::find(name).is_some())
        .expect("an AAC encoder (gstreamer1.0-libav avenc_aac)")
}

/// A 4 s 60 fps 160x120 H.264 MP4 with a keyframe every 60 frames and a
/// 48 kHz AAC track, whose 1024-sample packets never start on a whole second.
/// `bframes` above zero makes `mp4mux` write an `edts` edit list whose
/// composition offset shifts `segment.start` ahead of `segment.time` by that
/// many frame periods.
pub(crate) fn h264_aac_mp4(dir: &Path, bframes: u32) -> PathBuf {
    crate::gst::ensure_init().expect("GStreamer initializes");
    let path = dir.join(format!("keys-60-b{bframes}-aac.mp4"));
    let desc = format!(
        "videotestsrc num-buffers=240 pattern=ball ! video/x-raw,width=160,height=120,framerate=60/1 \
         ! x264enc key-int-max=60 bframes={bframes} b-adapt=false speed-preset=ultrafast option-string=scenecut=0:min-keyint=60 \
         ! h264parse ! queue ! mp4mux name=mux ! filesink location=\"{}\" \
         audiotestsrc num-buffers=190 samplesperbuffer=1024 ! audio/x-raw,rate=48000,channels=2 \
         ! {} ! queue ! mux.",
        path.display(),
        aac_encoder()
    );
    let pipeline = gst::parse::launch(&desc).expect("the fixture pipeline parses");
    pipeline
        .set_state(gst::State::Playing)
        .expect("the fixture pipeline plays");
    let bus = pipeline.bus().expect("a pipeline has a bus");
    let msg = bus
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(60),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the fixture finishes within 60 s");
    pipeline
        .set_state(gst::State::Null)
        .expect("the fixture pipeline stops");
    assert!(
        matches!(msg.view(), gst::MessageView::Eos(_)),
        "the fixture failed: {msg:?}"
    );
    path
}
