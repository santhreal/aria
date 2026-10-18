//! Contracts for the export sheet's choices.
//!
//! The class closed here is "an export has exactly one shape": the sheet stated
//! the canvas and the format and offered nothing to change, so a proof copy at
//! half size, a video-only file, a shorter keyframe interval, and a render of the
//! marked range were all impossible without editing the project.
//!
//! Every option is enumerated from its own `ALL`, so a preset added later is
//! covered without editing this file.
//!
//! Not covered: the encode itself, which needs a GPU. `export_settings` is what
//! the job hands to `aria_render`, so it is the seam worth pinning.

use super::premiere_tests::with_clip;
use crate::app::export_options::{ExportBitrate, ExportGop, ExportRate, ExportScale};

#[test]
fn the_defaults_reproduce_the_project() {
    let (state, _) = with_clip();
    let options = &state.export_options;
    assert_eq!(options.scale, ExportScale::Full);
    assert_eq!(options.rate, ExportRate::Project);
    assert_eq!(options.gop, ExportGop::Codec);
    assert!(options.audio);
    assert!(!options.marked_range);
    assert_eq!(options.plugin_encoder, None);
    let settings = state.export_settings();
    assert_eq!(settings.frame_rate, state.store.sequence.frame_rate);
    assert_eq!(settings.keyframe_interval, None);
    assert!(settings.audio);
    assert_eq!(settings.range, None);
}

#[test]
fn every_preset_of_every_option_is_reachable_by_cycling() {
    let (mut state, _) = with_clip();
    let mut seen = Vec::new();
    for _ in 0..ExportScale::ALL.len() {
        seen.push(state.export_options.scale);
        state.cycle_export_scale();
    }
    for preset in ExportScale::ALL {
        assert!(seen.contains(&preset), "{} is unreachable", preset.label());
    }
    assert_eq!(
        state.export_options.scale,
        ExportScale::Full,
        "a full cycle returns to where it started"
    );

    let mut rates = Vec::new();
    for _ in 0..ExportRate::ALL.len() {
        rates.push(state.export_options.rate);
        state.cycle_export_rate();
    }
    for preset in ExportRate::ALL {
        assert!(rates.contains(&preset), "{} is unreachable", preset.label());
    }

    let mut gops = Vec::new();
    for _ in 0..ExportGop::ALL.len() {
        gops.push(state.export_options.gop);
        state.cycle_export_gop();
    }
    for preset in ExportGop::ALL {
        assert!(gops.contains(&preset), "{} is unreachable", preset.label());
    }
}

#[test]
fn a_scale_shrinks_the_output_and_never_below_two_pixels() {
    let (mut state, _) = with_clip();
    let full = state.export_size();
    state.cycle_export_scale();
    for preset in ExportScale::ALL {
        state.export_options.scale = preset;
        let (w, h) = state.export_size();
        assert!(w >= 2 && h >= 2, "{} produced {w}x{h}", preset.label());
        assert!(
            w <= full.0 && h <= full.1,
            "{} is larger than the canvas",
            preset.label()
        );
    }
    state.export_options.scale = ExportScale::Half;
    let (w, h) = state.export_size();
    assert_eq!((w, h), (full.0 / 2, full.1 / 2));
}

#[test]
fn a_fixed_rate_overrides_the_project_rate() {
    let (mut state, _) = with_clip();
    state.export_options.rate = ExportRate::Fps60;
    let settings = state.export_settings();
    assert_eq!(settings.frame_rate.num, 60);
    assert_eq!(settings.frame_rate.den, 1);
    state.export_options.rate = ExportRate::Project;
    assert_eq!(
        state.export_settings().frame_rate,
        state.store.sequence.frame_rate
    );
}

#[test]
fn a_keyframe_choice_reaches_the_encoder_config() {
    let (mut state, _) = with_clip();
    state.export_options.gop = ExportGop::Frames(30);
    assert_eq!(state.export_settings().keyframe_interval, Some(30));
    state.export_options.gop = ExportGop::Codec;
    assert_eq!(state.export_settings().keyframe_interval, None);
}

#[test]
fn audio_off_writes_a_video_only_file() {
    let (mut state, _) = with_clip();
    state.toggle_export_audio();
    assert!(!state.export_settings().audio);
    state.toggle_export_audio();
    assert!(state.export_settings().audio);
}

#[test]
fn the_marked_range_is_refused_until_a_range_is_marked() {
    let (mut state, _) = with_clip();
    state.last_error = None;
    state.toggle_export_range();
    assert!(!state.export_options.marked_range);
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|err| err.contains("No range is marked")),
        "the refusal must say what to do, got {:?}",
        state.last_error
    );
    assert_eq!(state.export_settings().range, None);
}

#[test]
fn a_marked_range_becomes_the_exported_span() {
    let (mut state, _) = with_clip();
    state.set_playhead(20);
    state.set_mark_in();
    state.set_playhead(80);
    state.set_mark_out();
    assert_eq!(state.marked_range(), Some((20, 80)));

    state.toggle_export_range();
    assert!(state.export_options.marked_range);
    let range = state.export_settings().range.expect("a span");
    assert_eq!(range.0.value, 20);
    assert_eq!(range.1.value, 80);
    assert_eq!(
        state.export_range_label(),
        "00:00:00:20 to 00:00:03:08",
        "the sheet must name both ends in one SMPTE range"
    );

    state.toggle_export_range();
    assert_eq!(state.export_settings().range, None);
    assert_eq!(state.export_range_label(), "Whole timeline");
}

#[test]
fn an_empty_mark_pair_is_not_a_range() {
    let (mut state, _) = with_clip();
    state.set_playhead(30);
    state.set_mark_in();
    state.set_mark_out();
    assert_eq!(
        state.marked_range(),
        None,
        "In and Out on the same frame cover nothing"
    );
    state.toggle_export_range();
    assert!(!state.export_options.marked_range);
}

#[test]
fn choosing_how_to_export_is_not_an_edit() {
    let (mut state, _) = with_clip();
    let depth = state.history_depth();
    state.cycle_export_scale();
    state.cycle_export_rate();
    state.cycle_export_gop();
    state.toggle_export_audio();
    assert_eq!(state.history_depth(), depth);
}

#[test]
fn a_ranged_span_covers_the_frames_between_the_marks() {
    let (mut state, _) = with_clip();
    state.set_playhead(30);
    state.set_mark_in();
    state.set_playhead(90);
    state.set_mark_out();
    state.toggle_export_range();
    let settings = state.export_settings();
    let project = {
        let engine = state.engine().lock();
        engine.project().clone()
    };
    let (start, end) = settings.frame_span(&project);
    assert_eq!(start, 30);
    assert_eq!(end, 90);
    assert_eq!(end - start, 60, "the span is what was marked");

    let whole = {
        let mut plain = settings;
        plain.range = None;
        plain.frame_span(&project)
    };
    assert_eq!(whole.0, 0);
    assert!(whole.1 >= end, "the whole timeline is at least the range");
}

/// The sheet's Format row is read from the encoder, not compiled in: on a build
/// with no backend it must say so rather than promise an MP4, and it names the
/// codec that was chosen rather than the backend's default.
#[test]
fn the_format_row_names_what_this_export_will_write() {
    let cap = aria_encoder::capability();
    let label = crate::app::export_options::format_label(aria_core::VideoCodec::default());
    assert_eq!(label, cap.label());
    if cap.available {
        assert!(label.contains(cap.container), "{label} hides the container");
        assert!(label.contains(cap.video), "{label} hides the codec");
        for codec in aria_encoder::available_codecs() {
            let chosen = crate::app::export_options::format_label(codec);
            assert!(
                chosen.contains(codec.label()),
                "{chosen} does not name the chosen codec"
            );
        }
    } else {
        assert!(
            !label.contains("MP4"),
            "{label} promises a format this build cannot write"
        );
    }
}

/// Codec and bitrate are offered only where the encoder can honour them, and
/// what is offered is what the encoder claims. The class closed here is a
/// picker that cycles to a value the export would then refuse.
#[test]
fn the_codec_and_bitrate_rows_only_offer_what_the_encoder_takes() {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    let cap = aria_encoder::capability();
    let codecs = aria_encoder::available_codecs();

    // Whatever the ladder does, the settings never carry a codec the encoder
    // does not claim, nor a bitrate it does not honour.
    for _ in 0..(ExportBitrate::ALL.len() + codecs.len() + 2) {
        state.cycle_export_codec();
        state.cycle_export_bitrate();
        let settings = state.export_settings();
        if !codecs.is_empty() {
            assert!(
                codecs.contains(&settings.video_codec),
                "{:?} is not offered here",
                settings.video_codec
            );
        }
        assert!(
            cap.bitrate || settings.bitrate_kbps.is_none(),
            "a bitrate was set on a backend that ignores it"
        );
    }

    // A control that cannot move says why instead of doing nothing.
    if codecs.len() < 2 || !cap.bitrate {
        assert!(
            state.last_error.is_some(),
            "a control with one choice went silent"
        );
    }
}

/// The ladder reads in the unit an operator thinks in, and a round megabit is
/// not printed as four thousand kilobits.
#[test]
fn a_bitrate_reads_as_megabits_when_it_is_a_round_one() {
    assert_eq!(ExportBitrate::Encoder.label(), "Encoder default");
    assert_eq!(ExportBitrate::Kbps(8_000).label(), "8 Mb/s");
    assert_eq!(ExportBitrate::Kbps(2_500).label(), "2500 kb/s");
    assert_eq!(ExportBitrate::Encoder.kbps(), None);
    assert_eq!(ExportBitrate::Kbps(4_000).kbps(), Some(4_000));
}

#[test]
fn cycle_buttons_wraparound_invariants_and_custom_range_duration_math() {
    let (mut state, _) = with_clip();

    // Wraparound invariant: cycling through any option array exactly N times returns to start
    for _ in 0..ExportScale::ALL.len() {
        state.cycle_export_scale();
    }
    assert_eq!(state.export_options.scale, ExportScale::Full);

    for _ in 0..ExportRate::ALL.len() {
        state.cycle_export_rate();
    }
    assert_eq!(state.export_options.rate, ExportRate::Project);

    for _ in 0..ExportGop::ALL.len() {
        state.cycle_export_gop();
    }
    assert_eq!(state.export_options.gop, ExportGop::Codec);

    // Custom-range duration math
    state.set_playhead(48);
    state.set_mark_in();
    state.set_playhead(120);
    state.set_mark_out();
    assert_eq!(state.marked_range(), Some((48, 120)));
    let range_span = 120 - 48;
    assert_eq!(range_span, 72); // 3 seconds at 24fps
}

#[test]
fn plugin_encoders_list_in_format_cycle_and_control_dialog_visibility_and_extension() {
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
        fn choice(&self) -> aria_core::EncoderChoice {
            aria_core::EncoderChoice::only("dummy", None)
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
    let _prores = state
        .ctx
        .register_encoder(
            EncoderSpec {
                id: "prores.422".into(),
                name: "Apple ProRes 422".into(),
                extension: "mov".into(),
                audio: true,
            },
            DummyEncoder,
        )
        .expect("register prores");
    let _gif = state
        .ctx
        .register_encoder(
            EncoderSpec {
                id: "anim.gif".into(),
                name: "Animated GIF".into(),
                extension: "gif".into(),
                audio: false,
            },
            DummyEncoder,
        )
        .expect("register gif");

    // Platform encoder selected by default.
    assert_eq!(state.export_options.plugin_encoder, None);
    assert_eq!(state.export_extension(), "mp4");
    assert!(state.export_shows_codec_and_bitrate());
    assert!(state.export_shows_audio_toggle());

    // Step 1 -> prores.422 (audio = true): hides codec/bitrate, keeps audio toggle, sets .mov.
    state.cycle_export_format();
    assert_eq!(
        state.export_options.plugin_encoder.as_deref(),
        Some("prores.422")
    );
    assert_eq!(state.export_extension(), "mov");
    assert_eq!(state.export_format_label(), "Apple ProRes 422 (.mov)");
    assert_eq!(state.export_encoder_label(), "prores.422");
    assert!(!state.export_shows_codec_and_bitrate());
    assert!(state.export_shows_audio_toggle());
    assert!(state.export_settings().audio);

    // Step 2 -> anim.gif (audio = false): hides codec/bitrate AND audio toggle, sets .gif, forces audio = false.
    state.cycle_export_format();
    assert_eq!(
        state.export_options.plugin_encoder.as_deref(),
        Some("anim.gif")
    );
    assert_eq!(state.export_extension(), "gif");
    assert_eq!(state.export_format_label(), "Animated GIF (.gif)");
    assert!(!state.export_shows_codec_and_bitrate());
    assert!(!state.export_shows_audio_toggle());
    assert!(!state.export_settings().audio);

    // Step 3 -> wraps back to platform encoder (`None`).
    state.cycle_export_format();
    assert_eq!(state.export_options.plugin_encoder, None);
    assert_eq!(state.export_extension(), "mp4");
    assert!(state.export_shows_codec_and_bitrate());
    assert!(state.export_shows_audio_toggle());
}
