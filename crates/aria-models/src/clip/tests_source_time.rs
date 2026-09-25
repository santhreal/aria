//! `source_time` cases.

use super::*;
use crate::time::Rational;

#[test]
fn extracted_audio_companion_copies_only_audio_and_retime_state() {
    let media = MediaId::from_raw(41);
    let source_range = tr(12, 120, R24);
    let timeline = tr(36, 80, R24);
    let mut source = Clip::from_media(media, source_range, timeline);

    source.link = Some(LinkId::from_raw(9));
    source.speed = Rational::new(3, 2);
    source.reversed = true;
    source.speed_curve = speed_preset("hero").unwrap();
    source.preserve_pitch = false;
    source.volume = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: 0.25,
                easing: Easing::EaseIn,
                tangents: None,
            },
            Keyframe {
                tick: 79,
                value: 0.8,
                easing: Easing::Linear,
                tangents: None,
            },
        ],
    };
    source.pan = Param::Keyframed {
        keyframes: vec![
            Keyframe {
                tick: 0,
                value: -0.5,
                easing: Easing::Linear,
                tangents: None,
            },
            Keyframe {
                tick: 79,
                value: 0.5,
                easing: Easing::EaseOut,
                tangents: None,
            },
        ],
    };
    source.fade_in = 7;
    source.fade_out = 11;
    source.denoise = true;
    source.beats = vec![12, 36, 72];

    // Non-audio state is intentionally noisy: none of it may leak to the
    // audio-lane companion.
    source.transform = ClipTransform {
        position: [0.2, -0.3],
        anchor_point: [0.1, 0.9],
        scale: 1.5.into(),
        rotation: 20.0,
        opacity: 0.6,
    }
    .into();
    source.crop = CropRect {
        x: 0.1,
        y: 0.2,
        w: 0.7,
        h: 0.6,
    }
    .into();
    source.flip_h = true;
    source.filter = Some(crate::look::Filter {
        id: "vivid".into(),
        intensity: 0.4.into(),
    });
    source.animation_in = Some(crate::look::AnimationRef::new("fade_in"));
    source.replaceable = Some(Replaceable::new(3));
    source.text_editable = true;

    let companion = source.extracted_audio_companion().unwrap();
    let mut expected = Clip::from_media(media, source_range, timeline);
    expected.id = companion.id;
    expected.speed = source.speed;
    expected.reversed = source.reversed;
    expected.speed_curve = source.speed_curve.clone();
    expected.preserve_pitch = source.preserve_pitch;
    expected.volume = source.volume.clone();
    expected.pan = source.pan.clone();
    expected.fade_in = source.fade_in;
    expected.fade_out = source.fade_out;
    expected.denoise = source.denoise;
    expected.beats = source.beats.clone();
    expected.audio_role = Some(crate::look::AudioRole::Extracted);

    assert_eq!(companion, expected);
    assert_ne!(companion.id, source.id);
    assert_eq!(companion.timeline, source.timeline);
    assert_eq!(companion.content, source.content);
    assert_eq!(companion.link, None);
}

// --- generator serde compat -------------------------------------------

#[test]
fn legacy_bare_sticker_string_deserializes_to_empty_asset() {
    let g: Generator = serde_json::from_value(serde_json::json!("Sticker")).unwrap();
    assert_eq!(
        g,
        Generator::Sticker {
            asset: String::new()
        }
    );
    // Payload form without the field also defaults to empty.
    let g: Generator = serde_json::from_value(serde_json::json!({"Sticker": {}})).unwrap();
    assert_eq!(
        g,
        Generator::Sticker {
            asset: String::new()
        }
    );
}

#[test]
fn generator_roundtrips_through_custom_deserialize() {
    // One of each variant — pins the Deserialize mirror to the real enum.
    let all = [
        Generator::text("hi"),
        Generator::SolidColor { rgba: [1, 2, 3, 4] },
        Generator::shape(Shape::Ellipse, [9, 8, 7, 6]),
        Generator::sticker("heart"),
        Generator::lottie("/tmp/confetti.json", 256, 256),
        Generator::Effect,
        Generator::Filter,
        Generator::Adjustment,
    ];
    for g in all {
        let json = serde_json::to_value(&g).unwrap();
        let back: Generator = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back, g, "round-trip of {json}");
    }
}

#[test]
fn unknown_generator_variant_errors_with_variant_name() {
    let err = serde_json::from_value::<Generator>(serde_json::json!("Wombat")).unwrap_err();
    assert!(err.to_string().contains("Wombat"), "{err}");
}

#[test]
fn sticker_validate_accepts_catalog_and_empty_ids_only() {
    assert!(Generator::sticker("heart").validate().is_ok());
    assert!(Generator::sticker("").validate().is_ok());
    assert!(Generator::sticker("nope").validate().is_err());
}

// --- constructors -----------------------------------------------------

#[test]
fn from_media_wires_content_and_timeline() {
    let media = MediaId::from_raw(42);
    let source = tr(100, 50, R30);
    let timeline = tr(10, 40, R24);
    let clip = media_clip(media, source, timeline);

    assert_eq!(clip.content, ClipSource::Media { media, source });
    assert_eq!(clip.timeline, timeline);
    assert!(!clip.is_generated());
}

#[test]
fn from_media_assigns_distinct_ids() {
    let media = MediaId::from_raw(1);
    let source = tr(0, 10, R24);
    let timeline = tr(0, 10, R24);
    let a = media_clip(media, source, timeline);
    let b = media_clip(media, source, timeline);
    assert_ne!(a.id, b.id);
}

#[test]
fn generated_text_clip() {
    let timeline = tr(0, 48, R24);
    let clip = Clip::generated(Generator::text("Hello"), timeline);
    assert_eq!(
        clip.content,
        ClipSource::Generated(Generator::text("Hello"))
    );
    assert_eq!(clip.timeline, timeline);
    assert!(clip.is_generated());
}

#[test]
fn generated_all_variants() {
    let timeline = tr(0, 10, R24);

    let solid = Clip::generated(
        Generator::SolidColor {
            rgba: [255, 0, 0, 255],
        },
        timeline,
    );
    assert!(matches!(
        solid.content,
        ClipSource::Generated(Generator::SolidColor { .. })
    ));

    let shape = Clip::generated(
        Generator::shape(Shape::Ellipse, [0, 128, 255, 255]),
        timeline,
    );
    assert!(matches!(
        shape.content,
        ClipSource::Generated(Generator::Shape {
            shape: Shape::Ellipse,
            ..
        })
    ));

    let adj = Clip::generated(Generator::Adjustment, timeline);
    assert!(matches!(
        adj.content,
        ClipSource::Generated(Generator::Adjustment)
    ));
}

#[test]
fn generated_assigns_distinct_ids() {
    let timeline = tr(0, 10, R24);
    let a = Clip::generated(Generator::Adjustment, timeline);
    let b = Clip::generated(Generator::Adjustment, timeline);
    assert_ne!(a.id, b.id);
}

// --- beats (M8 Phase 6) -----------------------------------------------

#[test]
fn beat_timeline_ticks_map_proportionally_within_window() {
    // Source window [0, 100) at 24fps maps onto a [10, 60) timeline span
    // (50 ticks): a beat at source 50 (half-way) lands at 10 + 25 = 35.
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(10, 50, R24));
    clip.beats = vec![0, 50, 99];
    assert_eq!(clip.beat_timeline_ticks(), vec![10, 35, 60]);
}

#[test]
fn beat_timeline_ticks_skip_out_of_window_beats() {
    // Window [40, 80): beats before/after it (left by a trim/split) drop.
    let mut clip = media_clip(MediaId::from_raw(1), tr(40, 40, R24), tr(0, 40, R24));
    clip.beats = vec![10, 40, 60, 80, 100];
    assert_eq!(clip.beat_timeline_ticks(), vec![0, 20]);
}

#[test]
fn beat_timeline_ticks_mirror_when_reversed() {
    // Reversed: a beat near the source window end appears near clip start.
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    clip.reversed = true;
    clip.beats = vec![25, 75];
    assert_eq!(clip.beat_timeline_ticks(), vec![25, 75]);
    clip.beats = vec![10];
    assert_eq!(clip.beat_timeline_ticks(), vec![90]);
}

#[test]
fn beats_serialize_only_when_present() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 100, R24), tr(0, 100, R24));
    let json = serde_json::to_value(&clip).unwrap();
    assert!(
        json.get("beats").is_none(),
        "beat-free clips serialize without the field"
    );
    // Pre-beats files (no key) deserialize to an empty list.
    let back: Clip = serde_json::from_value(json).unwrap();
    assert!(!back.has_beats());

    let mut beaten = clip;
    beaten.beats = vec![10, 20, 30];
    let json = serde_json::to_value(&beaten).unwrap();
    assert_eq!(json["beats"], serde_json::json!([10, 20, 30]));
    let round: Clip = serde_json::from_value(json).unwrap();
    assert_eq!(round.beats, vec![10, 20, 30]);
}

// --- accessors --------------------------------------------------------

#[test]
fn media_clip_accessors() {
    let media = MediaId::from_raw(7);
    let source = tr(50, 25, R24);
    let timeline = tr(100, 25, R24);
    let clip = media_clip(media, source, timeline);

    assert_eq!(clip.media(), Some(media));
    assert_eq!(clip.source_range(), Some(source));
    assert_eq!(clip.start(), rt(100, R24));
    assert_eq!(clip.end().unwrap(), rt(125, R24));
}

#[test]
fn generated_clip_accessors_are_none() {
    let clip = Clip::generated(Generator::text("x"), tr(5, 10, R24));
    assert_eq!(clip.media(), None);
    assert_eq!(clip.source_range(), None);
    assert_eq!(clip.start().value, 5);
    assert_eq!(clip.end().unwrap().value, 15);
}

#[test]
fn clip_clone_and_eq() {
    let media = MediaId::from_raw(1);
    let source = tr(0, 10, R24);
    let timeline = tr(0, 10, R24);
    let a = media_clip(media, source, timeline);
    let b = a.clone();
    assert_eq!(a, b);
    assert_eq!(a.id, b.id);
}

// --- source_time_at: same-rate media ----------------------------------

#[test]
fn source_time_at_same_rate_maps_one_to_one() {
    // source [100, 110) placed at timeline [10, 20) — 1:1 at 24fps.
    let clip = media_clip(MediaId::from_raw(1), tr(100, 10, R24), tr(10, 10, R24));

    assert_eq!(
        clip.source_time_at(rt(15, R24)).unwrap(),
        Some(rt(105, R24))
    );
    assert_eq!(
        clip.source_time_at(rt(10, R24)).unwrap(),
        Some(rt(100, R24))
    );
    assert_eq!(
        clip.source_time_at(rt(19, R24)).unwrap(),
        Some(rt(109, R24))
    );
}

#[test]
fn source_time_at_half_open_boundaries() {
    let clip = media_clip(MediaId::from_raw(1), tr(0, 10, R24), tr(10, 10, R24));

    // Exclusive end is not contained.
    assert_eq!(clip.source_time_at(rt(20, R24)).unwrap(), None);
    // Before start.
    assert_eq!(clip.source_time_at(rt(9, R24)).unwrap(), None);
    // After end.
    assert_eq!(clip.source_time_at(rt(21, R24)).unwrap(), None);
}

#[test]
fn source_time_at_generated_always_none() {
    let clip = Clip::generated(Generator::text("title"), tr(0, 100, R24));
    assert_eq!(clip.source_time_at(rt(50, R24)).unwrap(), None);
}

// --- source_time_at: mixed rates ------------------------------------

#[test]
fn source_time_at_resamples_across_rates() {
    // 120 source ticks @ 30fps -> 96 timeline ticks @ 24fps.
    let clip = media_clip(MediaId::from_raw(1), tr(0, 120, R30), tr(0, 96, R24));

    // Timeline midpoint should land near source midpoint after resample.
    let src = clip.source_time_at(rt(48, R24)).unwrap().unwrap();
    assert_eq!(src.rate, R30);
    // 48 @ 24fps = 60 @ 30fps offset from source start 0.
    assert_eq!(src.value, 60);

    // Timeline start maps to source start regardless of rate.
    assert_eq!(clip.source_time_at(rt(0, R24)).unwrap(), Some(rt(0, R30)));
}

#[test]
fn source_time_at_offset_from_nonzero_source_start() {
    // source [200, 300) @ 30fps at timeline [0, 80) @ 24fps.
    let clip = media_clip(MediaId::from_raw(1), tr(200, 100, R30), tr(0, 80, R24));

    let at_start = clip.source_time_at(rt(0, R24)).unwrap().unwrap();
    assert_eq!(at_start, rt(200, R30));

    // 40 timeline ticks @ 24fps -> 50 source ticks @ 30fps from in-point.
    let mid = clip.source_time_at(rt(40, R24)).unwrap().unwrap();
    assert_eq!(mid, rt(250, R30));
}

// --- source_time_at: speed & reverse (M1) -----------------------------

#[test]
fn source_time_at_scales_by_speed() {
    // 2× speed: source [100, 200) occupies timeline [0, 50).
    let mut clip = media_clip(MediaId::from_raw(1), tr(100, 100, R24), tr(0, 50, R24));
    clip.speed = Rational::new(2, 1);
    assert!(clip.is_retimed());
    assert_eq!(clip.source_time_at(rt(0, R24)).unwrap(), Some(rt(100, R24)));
    assert_eq!(
        clip.source_time_at(rt(20, R24)).unwrap(),
        Some(rt(140, R24))
    );
    assert_eq!(
        clip.source_time_at(rt(49, R24)).unwrap(),
        Some(rt(198, R24))
    );
}

#[test]
fn source_time_at_half_speed_holds_frames() {
    // ½ speed: source [0, 50) stretches over timeline [0, 100); each
    // source frame holds for two timeline ticks.
    let mut clip = media_clip(MediaId::from_raw(1), tr(0, 50, R24), tr(0, 100, R24));
    clip.speed = Rational::new(1, 2);
    assert_eq!(clip.source_time_at(rt(0, R24)).unwrap(), Some(rt(0, R24)));
    assert_eq!(clip.source_time_at(rt(50, R24)).unwrap(), Some(rt(25, R24)));
    assert_eq!(clip.source_time_at(rt(51, R24)).unwrap(), Some(rt(25, R24)));
    assert_eq!(clip.source_time_at(rt(99, R24)).unwrap(), Some(rt(49, R24)));
}

#[test]
fn source_time_at_reversed_walks_backward() {
    let mut clip = media_clip(MediaId::from_raw(1), tr(100, 50, R24), tr(0, 50, R24));
    clip.reversed = true;
    assert!(clip.is_retimed());
    assert_eq!(clip.source_time_at(rt(0, R24)).unwrap(), Some(rt(149, R24)));
    assert_eq!(
        clip.source_time_at(rt(25, R24)).unwrap(),
        Some(rt(124, R24))
    );
    assert_eq!(
        clip.source_time_at(rt(49, R24)).unwrap(),
        Some(rt(100, R24))
    );
}
