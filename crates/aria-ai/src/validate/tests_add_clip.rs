//! `add_clip` cases.

use super::*;
use crate::wire;
use aria_models::MediaSource;

#[test]
fn trim_clip_converts_seconds_to_frame_ticks() {
    let (project, _, _, _, clip, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::TrimClip(wire::TrimClip {
            clip,
            start: 4.0,
            duration: 6.0,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::TrimClip {
            clip: ClipId::from_raw(clip),
            timeline: TimeRange::at_rate(96, 144, R24),
        }
    );
}

#[test]
fn fractional_seconds_snap_to_nearest_frame() {
    let (project, _, _, _, clip, _) = fixture();
    // 1.02 s at 24 fps = 24.48 frames -> 24; duration 0.01 s -> 0.24
    // frames -> clamps to 1 frame.
    let edit = lower(
        &project,
        WireCommand::TrimClip(wire::TrimClip {
            clip,
            start: 1.02,
            duration: 0.01,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::TrimClip {
            clip: ClipId::from_raw(clip),
            timeline: TimeRange::at_rate(24, 1, R24),
        }
    );
}

#[test]
fn add_clip_uses_media_rate_for_source_and_timeline_rate_for_start() {
    let mut project = Project::new("mixed-rates", R24);
    let media = project.add_media(MediaSource::new(
        "/tmp/30fps.mp4",
        1920,
        1080,
        Rational::FPS_30,
        300,
        true,
    ));
    let track = project.add_track(TrackKind::Video, "V1");

    let edit = lower(
        &project,
        WireCommand::AddClip(wire::AddClip {
            track: track.raw(),
            media: media.raw(),
            source_start: 1.0,
            source_duration: 4.0,
            start: 2.0,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::AddClip {
            track,
            media,
            source: TimeRange::at_rate(30, 120, Rational::FPS_30),
            start: RationalTime::new(48, R24),
        }
    );
}

#[test]
fn add_clip_rejects_out_of_bounds_source_with_media_extent() {
    let (project, media, video, _, _, _) = fixture();
    let msg = reject(
        &project,
        WireCommand::AddClip(wire::AddClip {
            track: video,
            media,
            source_start: 55.0,
            source_duration: 10.0,
            start: 0.0,
        }),
    );
    assert!(msg.contains("exceeds media"), "{msg}");
    assert!(msg.contains("60.000s"), "{msg}");
}

#[test]
fn unknown_ids_list_existing_ones() {
    let (project, _, video, text, clip, title) = fixture();

    let msg = reject(
        &project,
        WireCommand::RemoveClip(wire::RemoveClip { clip: 999 }),
    );
    assert!(msg.contains("clip 999 does not exist"), "{msg}");
    assert!(msg.contains(&clip.to_string()), "{msg}");
    assert!(msg.contains(&title.to_string()), "{msg}");

    let msg = reject(
        &project,
        WireCommand::RemoveTrack(wire::RemoveTrack { track: 999 }),
    );
    assert!(msg.contains("track 999 does not exist"), "{msg}");
    assert!(msg.contains(&video.to_string()), "{msg}");
    assert!(msg.contains(&text.to_string()), "{msg}");

    let msg = reject(
        &project,
        WireCommand::AddClip(wire::AddClip {
            track: video,
            media: 999,
            source_start: 0.0,
            source_duration: 1.0,
            start: 0.0,
        }),
    );
    assert!(msg.contains("media 999 does not exist"), "{msg}");
}

#[test]
fn remove_track_rejects_the_main_track() {
    let (mut project, _, video, text, _, _) = fixture();

    let msg = reject(
        &project,
        WireCommand::RemoveTrack(wire::RemoveTrack { track: video }),
    );
    assert!(msg.contains("main track"), "{msg}");

    // Non-main lanes stay removable.
    let overlay = project.add_track(TrackKind::Video, "V2");
    lower(
        &project,
        WireCommand::RemoveTrack(wire::RemoveTrack {
            track: overlay.raw(),
        }),
    );
    lower(
        &project,
        WireCommand::RemoveTrack(wire::RemoveTrack { track: text }),
    );
}

/// The lane a generator belongs on is a catalog column, so the refusal is
/// asserted against that column rather than against a sentence: the wording
/// `sticker (overlay) track` outlived the message that said it, and a
/// hardcoded phrase went stale without the rule changing.
#[test]
fn generators_must_match_lane_kind() {
    let (project, _, video, text, _, _) = fixture();

    let msg = reject(
        &project,
        WireCommand::AddGenerated(wire::AddGenerated {
            track: video,
            generator: WireGenerator::Text {
                content: "hi".into(),
            },
            start: 0.0,
            duration: 2.0,
        }),
    );
    let text_lane = aria_models::TrackKind::Text.spec().id;
    assert!(msg.contains(&format!("needs a {text_lane} track")), "{msg}");

    let msg = reject(
        &project,
        WireCommand::AddGenerated(wire::AddGenerated {
            track: text,
            generator: WireGenerator::Solid {
                rgba: [0, 0, 0, 255],
            },
            start: 0.0,
            duration: 2.0,
        }),
    );
    let sticker_lane = aria_models::TrackKind::Sticker.spec().id;
    assert!(
        msg.contains(&format!("needs a {sticker_lane} track")),
        "{msg}"
    );
    assert!(
        msg.contains(&format!("is a {text_lane} lane")),
        "the refusal names the lane the clip was aimed at: {msg}"
    );
}

#[test]
fn media_clips_cannot_land_on_generator_lanes() {
    let (project, media, _, text, _, _) = fixture();
    let msg = reject(
        &project,
        WireCommand::AddClip(wire::AddClip {
            track: text,
            media,
            source_start: 0.0,
            source_duration: 1.0,
            start: 0.0,
        }),
    );
    assert!(
        msg.contains("media clips need a video or audio track"),
        "{msg}"
    );
}

#[test]
fn set_generator_preserves_text_style_and_rejects_media_clips() {
    let (mut project, _, _, _, clip, title) = fixture();

    // Give the title a non-default style, then replace its content.
    let styled = Generator::Text {
        content: "INTRO".into(),
        style: aria_models::TextStyle {
            size: 120.0.into(),
            ..Default::default()
        },
    };
    project
        .set_generator(ClipId::from_raw(title), styled)
        .unwrap();

    let edit = lower(
        &project,
        WireCommand::SetGenerator(wire::SetGenerator {
            clip: title,
            generator: WireGenerator::Text {
                content: "OUTRO".into(),
            },
        }),
    );
    match edit {
        EditCommand::SetGenerator {
            generator: Generator::Text { content, style },
            ..
        } => {
            assert_eq!(content, "OUTRO");
            assert_eq!(
                style.size.sample(0),
                120.0,
                "existing style must be preserved"
            );
        }
        other => panic!("unexpected lowering: {other:?}"),
    }

    let msg = reject(
        &project,
        WireCommand::SetGenerator(wire::SetGenerator {
            clip,
            generator: WireGenerator::Text {
                content: "nope".into(),
            },
        }),
    );
    assert!(msg.contains("is a media clip"), "{msg}");
}

#[test]
fn transform_merges_with_current_values() {
    let (mut project, _, _, _, clip, _) = fixture();
    project
        .set_transform(
            ClipId::from_raw(clip),
            ClipTransform {
                position: [0.25, 0.0],
                scale: 0.5.into(),
                rotation: 10.0,
                opacity: 0.8,
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();

    let edit = lower(
        &project,
        WireCommand::SetClipTransform(wire::SetClipTransform {
            clip,
            position_x: None,
            position_y: Some(-0.1),
            anchor_x: None,
            anchor_y: None,
            scale: None,
            rotation: None,
            opacity: Some(1.0),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipTransform {
            clip: ClipId::from_raw(clip),
            transform: ClipTransform {
                position: [0.25, -0.1],
                scale: 0.5.into(),
                rotation: 10.0,
                opacity: 1.0,
                ..ClipTransform::IDENTITY
            },
            at: None,
        }
    );

    let msg = reject(
        &project,
        WireCommand::SetClipTransform(wire::SetClipTransform {
            clip,
            position_x: None,
            position_y: None,
            anchor_x: None,
            anchor_y: None,
            scale: Some(wire::WireScale::Uniform(0.0)),
            rotation: None,
            opacity: None,
        }),
    );
    assert!(msg.contains("invalid transform"), "{msg}");
}

#[test]
fn transform_scale_accepts_legacy_number_and_axes_array() {
    let (project, _, _, _, clip, _) = fixture();

    // Old agent payloads: bare number → uniform Scale2.
    let legacy = WireCommand::from_tool_call(
        "set_clip_transform",
        serde_json::json!({ "clip": clip, "scale": 1.5 }),
    )
    .unwrap();
    let edit = lower(&project, legacy);
    assert_eq!(
        edit,
        EditCommand::SetClipTransform {
            clip: ClipId::from_raw(clip),
            transform: ClipTransform {
                scale: aria_models::Scale2::uniform(1.5),
                ..ClipTransform::IDENTITY
            },
            at: None,
        }
    );

    let split = WireCommand::from_tool_call(
        "set_clip_transform",
        serde_json::json!({ "clip": clip, "scale": [2.0, 1.0] }),
    )
    .unwrap();
    let edit = lower(&project, split);
    assert_eq!(
        edit,
        EditCommand::SetClipTransform {
            clip: ClipId::from_raw(clip),
            transform: ClipTransform {
                scale: aria_models::Scale2 { x: 2.0, y: 1.0 },
                ..ClipTransform::IDENTITY
            },
            at: None,
        }
    );
}

#[test]
fn scale_keyframe_accepts_scalar_and_position_vec2() {
    let (project, _, _, _, clip, _) = fixture();

    let uniform = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Scale,
            at: 1.0,
            value: Some(2.0),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        uniform,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Scale,
            at: RationalTime::new(24, R24),
            value: ParamValue::Scalar(2.0),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    let axes = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Scale,
            at: 1.0,
            value: None,
            position: Some([2.0, 1.0]),
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        axes,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Scale,
            at: RationalTime::new(24, R24),
            value: ParamValue::Vec2([2.0, 1.0]),
            easing: Easing::Linear,
            tangents: None,
        }
    );
}
