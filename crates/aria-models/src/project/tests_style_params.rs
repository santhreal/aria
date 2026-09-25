//! `style_params` cases.

use super::*;
use crate::clip::Shape;

// --- shape / style params -----------------------------------------------

#[test]
fn style_params_keyframe_through_project_api() {
    use crate::look::{LayerShadow, LayerStyles};

    let mut project = Project::new("p", R24);
    let track = project.add_track(TrackKind::Video, "V1");
    let media = project.add_media(sample_media(R24, 500));
    // Source window 48 ticks, placed at timeline 100.
    let clip = project.add_clip(track, media, tr(0, 48), rt(100)).unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow::default()),
                ..Default::default()
            },
        )
        .unwrap();

    let blur = ClipParam::Style {
        param: crate::StyleParam::ShadowBlur,
    };
    project
        .set_param_keyframe(
            clip,
            blur,
            rt(100),
            ParamValue::Scalar(4.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            blur,
            rt(140),
            ParamValue::Scalar(20.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    assert_eq!(
        project
            .clip(clip)
            .unwrap()
            .styles
            .shadow
            .as_ref()
            .unwrap()
            .blur
            .sample(20),
        12.0
    );

    // Color / offset kind checks, constant + remove roundtrip.
    let color = ClipParam::Style {
        param: crate::StyleParam::ShadowColor,
    };
    assert!(
        project
            .set_param_keyframe(
                clip,
                color,
                rt(100),
                ParamValue::Scalar(1.0),
                Easing::Linear,
                None
            )
            .is_err()
    );
    let offset = ClipParam::Style {
        param: crate::StyleParam::ShadowOffset,
    };
    assert!(
        project
            .set_param_constant(clip, offset, ParamValue::Scalar(1.0))
            .is_err()
    );
    project
        .set_param_constant(clip, color, ParamValue::Color([10, 20, 30, 40]))
        .unwrap();
    assert_eq!(
        project
            .clip(clip)
            .unwrap()
            .styles
            .shadow
            .as_ref()
            .unwrap()
            .rgba
            .constant(),
        Some([10, 20, 30, 40])
    );
    project
        .set_param_keyframe(
            clip,
            blur,
            rt(120),
            ParamValue::Scalar(8.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project.remove_param_keyframe(clip, blur, rt(120)).unwrap();

    // Without a shadow block the scalar param is rejected.
    let bare = project.add_clip(track, media, tr(0, 48), rt(200)).unwrap();
    assert!(
        project
            .set_param_keyframe(
                bare,
                blur,
                rt(200),
                ParamValue::Scalar(4.0),
                Easing::Linear,
                None
            )
            .is_err()
    );
}

#[test]
fn spatial_tangents_routing_accepts_position_only() {
    use crate::SpatialTangents;
    use crate::look::{LayerShadow, LayerStyles};

    let (mut project, media_id, track) = project_with_media(200);
    let clip = project
        .add_clip(track, media_id, tr(0, 48), rt(100))
        .unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow::default()),
                ..Default::default()
            },
        )
        .unwrap();

    let handles = SpatialTangents {
        out_t: [0.0, 0.4],
        in_t: [0.0, 0.0],
    };
    // Position accepts tangents.
    project
        .set_param_keyframe(
            clip,
            ClipParam::Position,
            rt(100),
            ParamValue::Vec2([0.0, 0.0]),
            Easing::Linear,
            Some(handles),
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Position,
            rt(140),
            ParamValue::Vec2([0.5, 0.5]),
            Easing::Linear,
            Some(SpatialTangents {
                out_t: [0.0, 0.0],
                in_t: [-0.4, 0.0],
            }),
        )
        .unwrap();
    assert_eq!(
        project.clip(clip).unwrap().transform.position.keyframes()[0].tangents,
        Some(handles)
    );

    // Scalar opacity rejects.
    let err = project
        .set_param_keyframe(
            clip,
            ClipParam::Opacity,
            rt(100),
            ParamValue::Scalar(0.5),
            Easing::Linear,
            Some(handles),
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("spatial tangents are only supported on position"),
        "{err}"
    );

    // Vec2 shadow-offset also rejects (motion paths are position-only).
    let offset = ClipParam::Style {
        param: crate::StyleParam::ShadowOffset,
    };
    let err = project
        .set_param_keyframe(
            clip,
            offset,
            rt(100),
            ParamValue::Vec2([4.0, 4.0]),
            Easing::Linear,
            Some(handles),
        )
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("spatial tangents are only supported on position"),
        "{err}"
    );

    // Parallel tangents-only API mirrors the gate.
    let err = project
        .set_param_keyframe_tangents(clip, offset, rt(100), Some(handles))
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("spatial tangents are only supported on position"),
        "{err}"
    );
}

#[test]
fn effect_color_param_keyframes_through_project_api() {
    let (mut project, media_id, track) = project_with_media(100);
    let clip = project
        .add_clip(track, media_id, tr(0, 48), rt(100))
        .unwrap();
    project.add_effect(clip, "color_overlay").unwrap();

    let color = ClipParam::Effect {
        effect: 0,
        param: 0,
    };
    assert!(
        project
            .set_param_keyframe(
                clip,
                color,
                rt(100),
                ParamValue::Scalar(0.5),
                Easing::Linear,
                None
            )
            .is_err()
    );
    project
        .set_param_keyframe(
            clip,
            color,
            rt(100),
            ParamValue::Color([0, 0, 0, 255]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            color,
            rt(140),
            ParamValue::Color([255, 0, 0, 255]),
            Easing::Linear,
            None,
        )
        .unwrap();
    assert_eq!(
        project.clip(clip).unwrap().effects[0].sample_color_param("color", 20.0),
        Some([128, 0, 0, 255])
    );

    let offset = ClipParam::Effect {
        effect: 0,
        param: 1,
    };
    project
        .set_param_constant(clip, offset, ParamValue::Vec2([0.5, -0.25]))
        .unwrap();
    assert_eq!(
        project.clip(clip).unwrap().effects[0].sample_vec2_param("offset", 0.0),
        Some([0.5, -0.25])
    );
}

#[test]
fn shape_params_keyframe_through_project_api() {
    let mut project = Project::new("p", R24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::shape(Shape::Ellipse, [255, 255, 255, 255]),
            tr(100, 48),
        )
        .unwrap();

    // Keyframe ticks are clip-relative: timeline 100/140 → ticks 0/40.
    let width = ClipParam::Shape {
        param: crate::ShapeParam::Width,
    };
    project
        .set_param_keyframe(
            clip,
            width,
            rt(100),
            ParamValue::Scalar(100.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            width,
            rt(140),
            ParamValue::Scalar(500.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    let ClipSource::Generated(Generator::Shape { width: w, .. }) =
        &project.clip(clip).unwrap().content
    else {
        panic!("expected shape");
    };
    assert_eq!(w.sample(20), 300.0);

    // Constants flatten; out-of-range and wrong-kind values are rejected
    // with the clip unchanged.
    project
        .set_param_constant(clip, width, ParamValue::Scalar(250.0))
        .unwrap();
    assert!(
        project
            .set_param_keyframe(
                clip,
                width,
                rt(100),
                ParamValue::Color([0; 4]),
                Easing::Linear,
                None
            )
            .is_err()
    );
    assert!(
        project
            .set_param_constant(clip, width, ParamValue::Scalar(f32::NAN))
            .is_err()
    );

    // Media clips reject shape params outright.
    let media = project.add_media(sample_media(R24, 500));
    let vtrack = project.add_track(TrackKind::Video, "V1");
    let mclip = project.add_clip(vtrack, media, tr(0, 100), rt(0)).unwrap();
    assert!(
        project
            .set_param_constant(mclip, width, ParamValue::Scalar(10.0))
            .is_err()
    );
}
