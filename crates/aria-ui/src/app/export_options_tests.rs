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
//! the job hands to `aria_render`, so it is the seam worth pinning. The sheet's
//! chips, and that each press sets its own choice, are covered in
//! `panels/export_dialog_tests.rs`.

use super::premiere_tests::with_clip;
use crate::app::export_options::{
    AUDIO_CHOICES, ExportBitrate, ExportGop, ExportRate, ExportScale, WHOLE_TIMELINE,
};

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

/// Each setter puts its own argument in force, from any other choice of its
/// ladder. The class closed here is a setter that ignores its argument or
/// writes a neighboring field.
#[test]
fn every_choice_of_every_ladder_is_set_by_its_setter() {
    let (mut state, _) = with_clip();
    for from in ExportScale::ALL {
        for to in ExportScale::ALL {
            state.set_export_scale(from);
            state.set_export_scale(to);
            assert_eq!(
                state.export_options.scale,
                to,
                "{} from {}",
                to.label(),
                from.label()
            );
        }
    }
    for from in ExportRate::ALL {
        for to in ExportRate::ALL {
            state.set_export_rate(from);
            state.set_export_rate(to);
            assert_eq!(
                state.export_options.rate,
                to,
                "{} from {}",
                to.label(),
                from.label()
            );
        }
    }
    for from in ExportGop::ALL {
        for to in ExportGop::ALL {
            state.set_export_gop(from);
            state.set_export_gop(to);
            assert_eq!(
                state.export_options.gop,
                to,
                "{} from {}",
                to.label(),
                from.label()
            );
        }
    }
    for from in ExportBitrate::ALL {
        for to in ExportBitrate::ALL {
            state.set_export_bitrate(from);
            state.set_export_bitrate(to);
            assert_eq!(
                state.export_options.bitrate,
                to,
                "{} from {}",
                to.label(),
                from.label()
            );
        }
    }
    for codec in aria_core::VideoCodec::ALL {
        state.set_export_codec(codec);
        assert_eq!(state.export_options.codec, codec);
    }
    for (audio, label) in AUDIO_CHOICES {
        state.set_export_audio(!audio);
        state.set_export_audio(audio);
        assert_eq!(state.export_options.audio, audio, "{label}");
    }
    // The ladders did not touch each other's fields or the range.
    assert!(!state.export_options.marked_range);
    assert_eq!(state.export_options.plugin_encoder, None);
}

#[test]
fn a_scale_shrinks_the_output_and_never_below_two_pixels() {
    let (mut state, _) = with_clip();
    let full = state.export_size();
    for preset in ExportScale::ALL {
        state.set_export_scale(preset);
        let (w, h) = state.export_size();
        assert!(w >= 2 && h >= 2, "{} produced {w}x{h}", preset.label());
        assert!(
            w <= full.0 && h <= full.1,
            "{} is larger than the canvas",
            preset.label()
        );
    }
    state.set_export_scale(ExportScale::Half);
    let (w, h) = state.export_size();
    assert_eq!((w, h), (full.0 / 2, full.1 / 2));
}

#[test]
fn a_fixed_rate_overrides_the_project_rate() {
    let (mut state, _) = with_clip();
    state.set_export_rate(ExportRate::Fps60);
    let settings = state.export_settings();
    assert_eq!(settings.frame_rate.num, 60);
    assert_eq!(settings.frame_rate.den, 1);
    state.set_export_rate(ExportRate::Project);
    assert_eq!(
        state.export_settings().frame_rate,
        state.store.sequence.frame_rate
    );
}

#[test]
fn a_keyframe_choice_reaches_the_encoder_config() {
    let (mut state, _) = with_clip();
    state.set_export_gop(ExportGop::Frames(30));
    assert_eq!(state.export_settings().keyframe_interval, Some(30));
    state.set_export_gop(ExportGop::Codec);
    assert_eq!(state.export_settings().keyframe_interval, None);
}

/// A keyframe chip reads beside the row's own label, so it names the interval
/// and nothing else: `Every 30`.
#[test]
fn a_keyframe_interval_reads_as_every_n() {
    assert_eq!(ExportGop::Codec.label(), "Codec default");
    assert_eq!(ExportGop::Frames(1).label(), "Every 1");
    assert_eq!(ExportGop::Frames(30).label(), "Every 30");
}

#[test]
fn audio_off_writes_a_video_only_file() {
    let (mut state, _) = with_clip();
    state.set_export_audio(false);
    assert!(!state.export_settings().audio);
    state.set_export_audio(true);
    assert!(state.export_settings().audio);
}

#[test]
fn the_marked_range_is_refused_until_a_range_is_marked() {
    let (mut state, _) = with_clip();
    state.last_error = None;
    state.set_export_marked_range(true);
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
    assert_eq!(state.marked_range_label(), "Marked range");
    // Asking for the whole timeline needs no marks and reports nothing.
    state.last_error = None;
    state.set_export_marked_range(false);
    assert_eq!(state.last_error, None);
    assert!(!state.export_options.marked_range);
}

#[test]
fn a_marked_range_becomes_the_exported_span() {
    let (mut state, _) = with_clip();
    state.set_playhead(20);
    state.set_mark_in();
    state.set_playhead(80);
    state.set_mark_out();
    assert_eq!(state.marked_range(), Some((20, 80)));
    // The marked choice names its span before it is in force, so the chip
    // says what a press renders.
    assert_eq!(state.marked_range_label(), "00:00:00:20 to 00:00:03:08");
    assert_eq!(state.export_range_label(), WHOLE_TIMELINE);

    state.set_export_marked_range(true);
    assert!(state.export_options.marked_range);
    let range = state.export_settings().range.expect("a span");
    assert_eq!(range.0.value, 20);
    assert_eq!(range.1.value, 80);
    assert_eq!(
        state.export_range_label(),
        "00:00:00:20 to 00:00:03:08",
        "the sheet must name both ends in one SMPTE range"
    );

    state.set_export_marked_range(false);
    assert_eq!(state.export_settings().range, None);
    assert_eq!(state.export_range_label(), WHOLE_TIMELINE);
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
    state.set_export_marked_range(true);
    assert!(!state.export_options.marked_range);
}

#[test]
fn choosing_how_to_export_is_not_an_edit() {
    let (mut state, _) = with_clip();
    let depth = state.history_depth();
    state.set_export_scale(ExportScale::Half);
    state.set_export_rate(ExportRate::Fps30);
    state.set_export_gop(ExportGop::Frames(15));
    state.set_export_audio(false);
    state.set_export_bitrate(ExportBitrate::Kbps(8_000));
    assert_eq!(state.history_depth(), depth);
}

#[test]
fn a_ranged_span_covers_the_frames_between_the_marks() {
    let (mut state, _) = with_clip();
    state.set_playhead(30);
    state.set_mark_in();
    state.set_playhead(90);
    state.set_mark_out();
    state.set_export_marked_range(true);
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
        for &codec in aria_encoder::available_codecs() {
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

/// The ladder reads in the unit encoder settings use, and a round megabit is
/// not printed as four thousand kilobits.
#[test]
fn a_bitrate_reads_as_megabits_when_it_is_a_round_one() {
    assert_eq!(ExportBitrate::Encoder.label(), "Encoder default");
    assert_eq!(ExportBitrate::Kbps(8_000).label(), "8 Mb/s");
    assert_eq!(ExportBitrate::Kbps(2_500).label(), "2500 kb/s");
    assert_eq!(ExportBitrate::Encoder.kbps(), None);
    assert_eq!(ExportBitrate::Kbps(4_000).kbps(), Some(4_000));
}

/// A sheet that opened before the codec probe answered redraws once when it
/// does, and a sheet opened after it asks for no probe. The class closed here
/// is a Codec row that stays a fact until something else redraws, and a probe
/// waiter started on every open.
#[test]
fn a_codec_probe_lands_once_and_is_not_asked_for_again() {
    let (mut state, _) = with_clip();
    aria_encoder::available_codecs();
    state.codec_probe_pending = true;
    assert!(state.land_codec_probe(), "the answered probe lands");
    assert!(!state.codec_probe_pending);
    assert!(!state.land_codec_probe(), "a probe lands once");
    state.probe_export_codecs();
    assert!(
        !state.codec_probe_pending,
        "a probe that has answered is not waited on again"
    );
    assert!(!state.land_codec_probe());
}

/// The Format row lists the platform encoder and every mounted plugin encoder
/// in registration order, and selecting one moves the extension and the rows
/// it governs. A plugin encoder chosen and then unmounted stays a choice, named
/// as unmounted, so the choice in force is always on the row.
#[test]
fn the_format_choices_are_the_platform_and_each_plugin_encoder() {
    use crate::test_support::NullEncoder;
    use aria_plugin::EncoderSpec;

    let (mut state, _) = with_clip();
    let platform = crate::app::export_options::format_label(state.export_options.codec);
    assert_eq!(
        state.export_format_choices(),
        vec![(None, platform.clone())],
        "with no plugin encoder the platform encoder is the one choice"
    );
    let prores = state
        .ctx
        .register_encoder(
            EncoderSpec {
                id: "prores.422".into(),
                name: "Apple ProRes 422".into(),
                extension: "mov".into(),
                audio: true,
            },
            NullEncoder,
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
            NullEncoder,
        )
        .expect("register gif");
    assert_eq!(
        state.export_format_choices(),
        vec![
            (None, platform.clone()),
            (Some("prores.422".into()), "Apple ProRes 422 (.mov)".into()),
            (Some("anim.gif".into()), "Animated GIF (.gif)".into()),
        ]
    );

    // Platform encoder selected by default.
    assert_eq!(state.export_options.plugin_encoder, None);
    assert_eq!(state.export_extension(), "mp4");
    assert!(state.export_shows_codec_and_bitrate());
    assert!(state.export_shows_audio_toggle());

    // prores.422 (audio = true): hides codec/bitrate, keeps audio, sets .mov.
    state.select_export_plugin_encoder(Some("prores.422".into()));
    assert_eq!(state.export_extension(), "mov");
    assert_eq!(state.export_format_label(), "Apple ProRes 422 (.mov)");
    assert_eq!(state.export_encoder_label(), "prores.422");
    assert!(!state.export_shows_codec_and_bitrate());
    assert!(state.export_shows_audio_toggle());
    assert!(state.export_settings().audio);

    // anim.gif (audio = false): hides codec/bitrate and audio, forces audio off.
    state.select_export_plugin_encoder(Some("anim.gif".into()));
    assert_eq!(state.export_extension(), "gif");
    assert_eq!(state.export_format_label(), "Animated GIF (.gif)");
    assert!(!state.export_shows_codec_and_bitrate());
    assert!(!state.export_shows_audio_toggle());
    assert!(!state.export_settings().audio);

    // The chosen encoder unmounts: it stays the last choice, named as such.
    state.select_export_plugin_encoder(Some("prores.422".into()));
    drop(prores);
    assert_eq!(
        state.export_format_choices(),
        vec![
            (None, platform),
            (Some("anim.gif".into()), "Animated GIF (.gif)".into()),
            (Some("prores.422".into()), "prores.422 (unmounted)".into()),
        ]
    );

    // Back to the platform encoder.
    state.select_export_plugin_encoder(None);
    assert_eq!(state.export_extension(), "mp4");
    assert!(state.export_shows_codec_and_bitrate());
    assert!(state.export_shows_audio_toggle());
}
