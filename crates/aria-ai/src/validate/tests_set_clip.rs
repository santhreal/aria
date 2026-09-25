//! `set_clip` cases.

use super::*;
use crate::wire;

#[test]
fn set_clip_blend_mode_lowers_and_guards() {
    use aria_models::BlendMode;

    let (mut project, media, _, _, clip, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::SetClipBlendMode(wire::SetClipBlendMode {
            clip,
            mode: wire::WireBlendMode::Multiply,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipBlendMode {
            clip: ClipId::from_raw(clip),
            mode: BlendMode::Multiply,
        }
    );

    let lane = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project
        .add_clip(
            lane,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetClipBlendMode(wire::SetClipBlendMode {
            clip: audio_clip.raw(),
            mode: wire::WireBlendMode::Screen,
        }),
    );
    assert!(msg.contains("visual frame"), "{msg}");

    let err = WireCommand::from_tool_call(
        "set_clip_blend_mode",
        serde_json::json!({ "clip": clip, "mode": "nope" }),
    )
    .unwrap_err();
    assert!(
        err.contains("nope") || err.contains("unknown variant"),
        "{err}"
    );

    let adj_lane = project.add_track(TrackKind::Adjustment, "ADJ");
    let adj = project
        .add_generated(
            adj_lane,
            Generator::Adjustment,
            TimeRange::at_rate(0, 24, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetClipBlendMode(wire::SetClipBlendMode {
            clip: adj.raw(),
            mode: wire::WireBlendMode::Multiply,
        }),
    );
    assert!(msg.contains("adjustment/effect/filter"), "{msg}");
}

#[test]
fn set_motion_blur_lowers_and_guards() {
    use aria_models::MotionBlur;

    let (mut project, media, _, _, clip, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::SetMotionBlur(wire::SetMotionBlur {
            clip,
            enabled: true,
            shutter_deg: Some(270.0),
            samples: Some(12),
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipMotionBlur {
            clip: ClipId::from_raw(clip),
            motion_blur: MotionBlur {
                enabled: true,
                shutter_deg: 270.0,
                samples: 12,
            },
        }
    );

    let lane = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project
        .add_clip(
            lane,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetMotionBlur(wire::SetMotionBlur {
            clip: audio_clip.raw(),
            enabled: true,
            shutter_deg: None,
            samples: None,
        }),
    );
    assert!(msg.contains("visual frame"), "{msg}");

    let msg = reject(
        &project,
        WireCommand::SetMotionBlur(wire::SetMotionBlur {
            clip,
            enabled: true,
            shutter_deg: Some(900.0),
            samples: None,
        }),
    );
    assert!(msg.contains("shutter") || msg.contains("720"), "{msg}");

    let fx_lane = project.add_track(TrackKind::Effect, "FX");
    let fx = project
        .add_generated(fx_lane, Generator::Effect, TimeRange::at_rate(0, 24, R24))
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetMotionBlur(wire::SetMotionBlur {
            clip: fx.raw(),
            enabled: true,
            shutter_deg: None,
            samples: None,
        }),
    );
    assert!(msg.contains("adjustment/effect/filter"), "{msg}");
}

#[test]
fn set_layer_styles_lowers_and_guards() {
    use aria_models::{LayerShadow, LayerStyles, Param};

    let (mut project, media, _, _, clip, _) = fixture();
    let edit = lower(
        &project,
        WireCommand::SetClipLayerStyles(wire::SetClipLayerStyles {
            clip,
            styles: wire::WireLayerStyles {
                shadow: Some(wire::WireLayerShadow {
                    rgba: [0, 0, 0, 128],
                    offset: [4.0, 4.0],
                    blur: 8.0,
                }),
                outline: Some(wire::WireLayerOutline {
                    rgba: [255, 255, 255, 255],
                    width: 2.0,
                }),
                ..Default::default()
            },
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetClipLayerStyles {
            clip: ClipId::from_raw(clip),
            styles: LayerStyles {
                shadow: Some(LayerShadow {
                    rgba: Param::Constant([0, 0, 0, 128]),
                    offset: Param::Constant([4.0, 4.0]),
                    blur: Param::Constant(8.0),
                }),
                outline: Some(aria_models::LayerOutline {
                    rgba: Param::Constant([255, 255, 255, 255]),
                    width: Param::Constant(2.0),
                }),
                ..Default::default()
            },
        }
    );

    let lane = project.add_track(TrackKind::Audio, "A1");
    let audio_clip = project
        .add_clip(
            lane,
            aria_models::MediaId::from_raw(media),
            TimeRange::at_rate(0, 240, R24),
            RationalTime::new(0, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetClipLayerStyles(wire::SetClipLayerStyles {
            clip: audio_clip.raw(),
            styles: wire::WireLayerStyles {
                glow: Some(wire::WireLayerGlow {
                    rgba: [255, 255, 255, 255],
                    radius: 12.0,
                    intensity: 1.0,
                }),
                ..Default::default()
            },
        }),
    );
    assert!(msg.contains("visual frame"), "{msg}");

    let filter_lane = project.add_track(TrackKind::Filter, "FLT");
    let filter_clip = project
        .add_generated(
            filter_lane,
            Generator::Filter,
            TimeRange::at_rate(0, 24, R24),
        )
        .unwrap();
    let msg = reject(
        &project,
        WireCommand::SetClipLayerStyles(wire::SetClipLayerStyles {
            clip: filter_clip.raw(),
            styles: wire::WireLayerStyles {
                shadow: Some(wire::WireLayerShadow {
                    rgba: [0, 0, 0, 128],
                    offset: [2.0, 2.0],
                    blur: 4.0,
                }),
                ..Default::default()
            },
        }),
    );
    assert!(msg.contains("adjustment/effect/filter"), "{msg}");
}

#[test]
fn style_param_keyframe_lowers_by_value_kind() {
    let (project, _, _, _, clip, _) = fixture();

    let edit = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Style {
                param: wire::WireStyleParam::ShadowBlur,
            },
            at: 1.0,
            value: Some(12.0),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        edit,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Style {
                param: aria_models::StyleParam::ShadowBlur,
            },
            at: RationalTime::new(24, R24),
            value: ParamValue::Scalar(12.0),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Style {
                param: wire::WireStyleParam::ShadowOffset,
            },
            at: 1.0,
            value: Some(4.0),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert!(msg.contains("position"), "{msg}");

    let offset = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Style {
                param: wire::WireStyleParam::ShadowOffset,
            },
            at: 1.0,
            value: None,
            position: Some([4.0, -2.0]),
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        offset,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Style {
                param: aria_models::StyleParam::ShadowOffset,
            },
            at: RationalTime::new(24, R24),
            value: ParamValue::Vec2([4.0, -2.0]),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    let msg = reject(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Style {
                param: wire::WireStyleParam::GlowColor,
            },
            at: 1.0,
            value: Some(1.0),
            position: None,
            rgba: None,
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert!(msg.contains("rgba"), "{msg}");

    let color = lower(
        &project,
        WireCommand::SetParamKeyframe(wire::SetParamKeyframe {
            clip,
            param: wire::WireClipParam::Style {
                param: wire::WireStyleParam::GlowColor,
            },
            at: 1.0,
            value: None,
            position: None,
            rgba: Some([255, 200, 0, 255]),
            rect: None,
            easing: None,

            tangent_out: None,
            tangent_in: None,
        }),
    );
    assert_eq!(
        color,
        EditCommand::SetParamKeyframe {
            clip: ClipId::from_raw(clip),
            param: ClipParam::Style {
                param: aria_models::StyleParam::GlowColor,
            },
            at: RationalTime::new(24, R24),
            value: ParamValue::Color([255, 200, 0, 255]),
            easing: Easing::Linear,
            tangents: None,
        }
    );

    // Tagged nesting on the wire: {"style":{"param":"shadow_blur"}}
    let from_json = WireCommand::from_tool_call(
        "set_param_keyframe",
        serde_json::json!({
            "clip": clip,
            "param": { "style": { "param": "shadow_blur" } },
            "at": 1.0,
            "value": 8.0,
        }),
    )
    .unwrap();
    let WireCommand::SetParamKeyframe(args) = from_json else {
        panic!("expected SetParamKeyframe");
    };
    assert_eq!(
        args.param,
        wire::WireClipParam::Style {
            param: wire::WireStyleParam::ShadowBlur
        }
    );
}
