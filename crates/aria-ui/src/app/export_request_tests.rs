//! What the Export button sends, asserted without rendering anything.
//!
//! Defect class closed: the export sheet's rows wrote into the state and the
//! button read four of them back inside its own click closure, so the wiring
//! between a row and the file that comes out was observable only by encoding
//! one. A row pointed at the wrong option, a scale that never reached the size,
//! or a marked range the button ignored would all have turned nothing red.
//!
//! Every one of these presses a row's own option and reads the request the
//! button would send. Nothing here starts a job, touches an encoder or asks for
//! a device.
//!
//! Fail-by-default: each catalog is walked from its own `ALL`, and the expected
//! value comes from an exhaustive `match` on the variant, so a new scale, rate,
//! keyframe spacing, bitrate or codec stops this file compiling until someone
//! states what the request should carry for it.
//!
//! What this does not catch: whether the queue honours the request, and whether
//! the encoder writes what was asked for. Those are `export_queue_tests` and
//! the render crate's own suite.

use aria_core::{ExportFormat, Rational, VideoCodec};

use super::export_options::{ExportBitrate, ExportGop, ExportRate, ExportScale};
use super::premiere_tests::with_clip;
use super::{AppState, export_request::ExportRequest};

/// A state with a clip on the timeline, and the request it would send.
fn asked() -> (AppState, ExportRequest) {
    let (state, _) = with_clip();
    let request = state.export_request(state.export_destination());
    (state, request)
}

/// The request as it stands now, addressed at wherever the sheet points.
fn asked_for(state: &AppState) -> ExportRequest {
    state.export_request(state.export_destination())
}

#[test]
fn an_unsaved_project_exports_beside_its_data_and_a_saved_one_beside_itself() {
    let (mut state, opened) = asked();
    let name = state.store.project_name.clone();
    assert_eq!(
        opened.destination,
        state.data_dir.join(format!("{name}.mp4")),
        "a project that has never been saved has nowhere else to land"
    );

    state.project_path = Some(state.data_dir.join("cut.aria"));
    assert_eq!(
        asked_for(&state).destination,
        state.data_dir.join("cut.mp4"),
        "a saved project exports beside itself, under its own name"
    );
}

#[test]
fn every_scale_reaches_the_size_the_request_carries() {
    let (mut state, _) = with_clip();
    let [w, h] = state.canvas_size();
    for scale in ExportScale::ALL {
        state.export_options.scale = scale;
        // Stated per variant rather than derived from the scale's own
        // arithmetic: a test that called the same helper would agree with a
        // wrong helper.
        let (num, den) = match scale {
            ExportScale::Full => (1, 1),
            ExportScale::Threequarter => (3, 4),
            ExportScale::Half => (1, 2),
            ExportScale::Quarter => (1, 4),
        };
        let want =
            aria_render::even_export_size((((w * num) / den).max(2), ((h * num) / den).max(2)));
        let got = asked_for(&state);
        assert_eq!(
            got.size,
            want,
            "{} does not reach the size the file will have",
            scale.label()
        );
        assert_eq!(
            got.format.size,
            Some((((w * num) / den).max(2), ((h * num) / den).max(2))),
            "{} does not reach the renderer's own size",
            scale.label()
        );
    }
}

#[test]
fn every_delivery_rate_reaches_the_request() {
    let (mut state, _) = with_clip();
    let project = state.store.sequence.frame_rate;
    for rate in ExportRate::ALL {
        state.export_options.rate = rate;
        let want = match rate {
            ExportRate::Project => project,
            ExportRate::Fps24 => Rational::new(24, 1),
            ExportRate::Fps25 => Rational::new(25, 1),
            ExportRate::Fps30 => Rational::new(30, 1),
            ExportRate::Fps50 => Rational::new(50, 1),
            ExportRate::Fps60 => Rational::new(60, 1),
        };
        assert_eq!(
            asked_for(&state).format.frame_rate,
            Some(want),
            "{} does not reach the request",
            rate.label()
        );
    }
}

#[test]
fn every_keyframe_spacing_reaches_the_request() {
    let (mut state, _) = with_clip();
    for gop in ExportGop::ALL {
        state.export_options.gop = gop;
        let want = match gop {
            ExportGop::Codec => None,
            ExportGop::Frames(frames) => Some(frames),
        };
        assert_eq!(
            asked_for(&state).format.keyframe_interval,
            want,
            "{} does not reach the request",
            gop.label()
        );
    }
}

#[test]
fn every_bitrate_reaches_the_request() {
    let (mut state, _) = with_clip();
    for bitrate in ExportBitrate::ALL {
        state.export_options.bitrate = bitrate;
        let want = match bitrate {
            ExportBitrate::Encoder => None,
            ExportBitrate::Kbps(kbps) => Some(kbps),
        };
        assert_eq!(
            asked_for(&state).format.bitrate_kbps,
            want,
            "{} does not reach the request",
            bitrate.label()
        );
    }
}

#[test]
fn every_codec_reaches_the_request() {
    let (mut state, _) = with_clip();
    for codec in VideoCodec::ALL {
        state.export_options.codec = codec;
        assert_eq!(
            asked_for(&state).format.video_codec,
            Some(codec),
            "{} does not reach the request",
            codec.label()
        );
    }
}

#[test]
fn a_video_only_export_says_so_in_the_request() {
    let (mut state, with_audio) = asked();
    assert_eq!(with_audio.format.audio, Some(true));
    state.export_options.audio = false;
    assert_eq!(
        asked_for(&state).format.audio,
        Some(false),
        "the audio row must reach the file's audio track"
    );
}

#[test]
fn the_marked_range_is_the_span_the_request_renders_and_the_words_it_is_named_by() {
    let (mut state, whole) = asked();
    assert_eq!(whole.format.range, None);
    assert_eq!(whole.range, "Whole timeline");

    // Asking for a range with nothing marked leaves the whole timeline: the
    // flag is a request, not a span.
    state.export_options.marked_range = true;
    assert_eq!(
        asked_for(&state).format.range,
        None,
        "an unmarked range cannot become a span"
    );

    state.set_playhead(24);
    state.set_mark_in();
    state.set_playhead(48);
    state.set_mark_out();
    let marked = asked_for(&state);
    let rate = state.store.sequence.frame_rate;
    assert_eq!(
        marked.format.range,
        Some((
            aria_core::RationalTime::new(24, rate),
            aria_core::RationalTime::new(48, rate)
        )),
        "the marked span must be the span the request renders"
    );
    assert_ne!(
        marked.range, "Whole timeline",
        "the words must follow the span"
    );
    assert!(
        marked.range.contains(" to "),
        "a range reads as one timecode to another, not {}",
        marked.range
    );

    // Turning the row off again returns the whole timeline, marks and all.
    state.export_options.marked_range = false;
    let all = asked_for(&state);
    assert_eq!(all.format.range, None);
    assert_eq!(all.range, "Whole timeline");
}

/// The point of the value: reading it costs nothing and starts nothing.
#[test]
fn building_a_request_queues_no_export_and_says_nothing() {
    let (mut state, _) = with_clip();
    state.status_message = None;
    let before = state.export_queue_snapshot().len();
    for _ in 0..3 {
        let _ = asked_for(&state);
    }
    assert_eq!(
        state.export_queue_snapshot().len(),
        before,
        "describing an export must not queue one"
    );
    assert_eq!(
        state.status_message, None,
        "describing an export is not an event to report"
    );
}

/// The request is the whole of what the button sends: what the queue takes is
/// derived from it and from the machine, never from a second read of the state.
#[test]
fn the_default_request_names_the_project_it_would_render() {
    let (state, defaults) = asked();
    let name = state.store.project_name.clone();
    assert_eq!(
        defaults,
        ExportRequest {
            destination: state.data_dir.join(format!("{name}.mp4")),
            format: ExportFormat {
                size: Some(state.canvas_size_tuple()),
                frame_rate: Some(state.store.sequence.frame_rate),
                keyframe_interval: None,
                audio: Some(true),
                range: None,
                video_codec: Some(VideoCodec::H264),
                bitrate_kbps: None,
            },
            size: aria_render::even_export_size(state.canvas_size_tuple()),
            range: "Whole timeline".to_string(),
            plugin_encoder: None,
        },
        "the sheet's defaults must describe the project's own output"
    );
}

#[test]
fn selecting_a_plugin_encoder_updates_destination_extension_and_request_id() {
    use aria_core::{EncodeError, EncoderConfig, VideoEncoder, VideoFrame};
    use aria_plugin::{Encoder, EncoderSpec};
    use std::path::Path;

    struct DummyEncoder;
    impl VideoEncoder for DummyEncoder {
        fn push(&mut self, _frame: &VideoFrame) -> Result<(), EncodeError> {
            Ok(())
        }
        fn finish(&mut self) -> Result<(), EncodeError> {
            Ok(())
        }
    }
    impl Encoder for DummyEncoder {
        fn open(
            &self,
            _path: &Path,
            _config: &EncoderConfig,
        ) -> Result<Box<dyn VideoEncoder>, EncodeError> {
            Ok(Box::new(DummyEncoder))
        }
    }

    let (mut state, _) = with_clip();
    let _reg = state
        .ctx
        .register_encoder(
            EncoderSpec {
                id: "webm.vp9".into(),
                name: "WebM VP9".into(),
                extension: "webm".into(),
                audio: false,
            },
            DummyEncoder,
        )
        .expect("register webm.vp9");
    state.select_export_plugin_encoder(Some("webm.vp9".into()));
    state.project_path = Some(state.data_dir.join("edit.aria"));

    let req = asked_for(&state);
    assert_eq!(req.destination, state.data_dir.join("edit.webm"));
    assert_eq!(req.plugin_encoder.as_deref(), Some("webm.vp9"));
    assert_eq!(req.format.audio, Some(false));
}
