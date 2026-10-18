//! The DMABuf cause text, which decoders are starved, and what a starved
//! decoder receives.

use gstreamer_app as gst_app;

use super::*;

fn caps(description: &str) -> gst::Caps {
    gst::init().expect("GStreamer initializes");
    description.parse().expect("caps parse")
}

fn nvdec_caps() -> gst::Caps {
    caps(
        "video/x-raw(memory:CUDAMemory), format=NV12; \
         video/x-raw(memory:GLMemory), format=NV12; video/x-raw, format=NV12",
    )
}

fn vadec_caps() -> gst::Caps {
    caps("video/x-raw(memory:VAMemory); video/x-raw(memory:DMABuf); video/x-raw")
}

/// WHY: the DMABuf fallback was logged as "Internal data stream error", which
/// names no element. The cause states each decoder plugged and the memory it
/// can output, so a host without DMABuf output is identified from the log.
/// Does not cover a decoder that offers DMABuf and still fails to link: that
/// case is reported as such, without the downstream reason.
#[test]
fn the_dmabuf_cause_names_each_decoder_and_its_output_memory() {
    assert_eq!(
        dmabuf_cause(&[("nvh264dec".into(), nvdec_caps())]),
        "nvh264dec outputs memory:CUDAMemory, memory:GLMemory, system memory and no \
         memory:DMABuf"
    );
    assert_eq!(
        dmabuf_cause(&[("vah264dec".into(), vadec_caps())]),
        "vah264dec can output memory:DMABuf, but none reached the sink"
    );
    assert_eq!(
        dmabuf_cause(&[
            ("avdec_h264".into(), caps("video/x-raw")),
            ("anydec".into(), caps("ANY")),
        ]),
        "avdec_h264 outputs system memory and no memory:DMABuf; anydec outputs any memory \
         and no memory:DMABuf"
    );
    assert_eq!(dmabuf_cause(&[]), "no video decoder was plugged");
}

/// WHY: starving a decoder that can output DMABuf would turn a working GPU
/// decode into CPU decode. Only a decoder whose every output is some other
/// memory is starved; one that allows any memory is not.
#[test]
fn only_a_decoder_without_dmabuf_output_is_starved() {
    assert!(!offers_dmabuf(&nvdec_caps()));
    assert!(!offers_dmabuf(&caps("video/x-raw, format=NV12")));
    assert!(offers_dmabuf(&vadec_caps()));
    assert!(offers_dmabuf(&caps(
        "video/x-raw(memory:DMABuf), format=NV12"
    )));
    assert!(offers_dmabuf(&caps("ANY")));
    assert!(offers_dmabuf(&caps("video/x-raw(ANY)")));
}

/// WHY: the decoder of a doomed DMABuf attempt must not open. The starved
/// element receives no caps and no buffer, so it creates no decoder session,
/// while the stream-start and EOS events still reach it and the pipeline ends
/// normally. Consuming an item must not log a GStreamer critical: a `Drop`
/// return printed one `gst_mini_object_unref` critical per starved item on
/// every CPU-decoded open. Does not cover a real hardware decoder; see
/// `a_dmabuf_attempt_stops_once_its_decoder_has_no_dmabuf_output`.
#[test]
fn a_starved_element_gets_no_caps_and_no_buffers_and_marks_the_watch() {
    gst::init().expect("GStreamer initializes");
    let criticals = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let count = Arc::clone(&criticals);
    let handler = gst::glib::log_set_handler(
        Some("GStreamer"),
        gst::glib::LogLevels::LEVEL_CRITICAL,
        false,
        false,
        move |_, _, _| {
            count.fetch_add(1, Ordering::Relaxed);
        },
    );
    let pipeline = gst::parse::launch(
        "appsrc name=in format=time caps=video/x-raw,format=NV12,width=16,height=16,framerate=30/1 \
         ! identity name=dec ! appsink name=out sync=false",
    )
    .expect("pipeline")
    .downcast::<gst::Pipeline>()
    .expect("a pipeline");
    let dec = pipeline.by_name("dec").expect("dec");
    let watch = Arc::new(DmabufWatch::default());
    starve(&dec, &watch);
    assert!(!watch.stopped(), "stopped before anything arrived");

    let passed = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&passed);
    dec.static_pad("src")
        .expect("src pad")
        .add_probe(gst::PadProbeType::BUFFER, move |_, _| {
            counter.fetch_add(1, Ordering::Relaxed);
            gst::PadProbeReturn::Ok
        });

    pipeline.set_state(gst::State::Playing).expect("playing");
    let src = pipeline
        .by_name("in")
        .expect("in")
        .downcast::<gst_app::AppSrc>()
        .expect("appsrc");
    for _ in 0..3 {
        let mut buffer = gst::Buffer::with_size(16 * 16 * 3 / 2).expect("buffer");
        buffer
            .get_mut()
            .expect("writable")
            .set_pts(gst::ClockTime::ZERO);
        src.push_buffer(buffer).expect("push");
    }
    src.end_of_stream().expect("eos");
    let bus = pipeline.bus().expect("bus");
    let msg = bus
        .timed_pop_filtered(
            gst::ClockTime::from_seconds(5),
            &[gst::MessageType::Eos, gst::MessageType::Error],
        )
        .expect("the pipeline ends within 5 s");
    let sink_pad = dec.static_pad("sink").expect("sink pad");
    let caps_seen = sink_pad.current_caps();
    let stream_started = sink_pad
        .sticky_event::<gst::event::StreamStart>(0)
        .is_some();
    pipeline.set_state(gst::State::Null).expect("null");
    gst::glib::log_remove_handler(Some("GStreamer"), handler);

    assert_eq!(msg.type_(), gst::MessageType::Eos, "{msg:?}");
    assert!(
        caps_seen.is_none(),
        "the starved element got caps {caps_seen:?}"
    );
    assert_eq!(
        passed.load(Ordering::Relaxed),
        0,
        "buffers reached the element"
    );
    assert!(stream_started, "stream-start was dropped too");
    assert!(watch.stopped(), "the watch was not marked");
    assert_eq!(
        criticals.load(Ordering::Relaxed),
        0,
        "consuming the starved items logged GStreamer criticals"
    );
}
