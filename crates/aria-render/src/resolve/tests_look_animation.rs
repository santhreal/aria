//! `look_animation` cases.

use super::*;

#[test]
fn keyframed_crop_samples_uv_per_tick() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    let a = CropRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    };
    let b = CropRect {
        x: 0.4,
        y: 0.2,
        w: 0.5,
        h: 0.6,
    };
    project
        .set_param_keyframe(
            clip,
            ClipParam::Crop,
            rt(0),
            ParamValue::Rect([a.x, a.y, a.w, a.h]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Crop,
            rt(10),
            ParamValue::Rect([b.x, b.y, b.w, b.h]),
            Easing::Linear,
            None,
        )
        .unwrap();

    let uv0 = resolve(&project, rt(0)).unwrap().layers[0].uv;
    approx(uv0[0], 0.0);
    approx(uv0[1], 0.0);
    approx(uv0[2], 1.0);
    approx(uv0[3], 1.0);

    let uv5 = resolve(&project, rt(5)).unwrap().layers[0].uv;
    // Midpoint crop [0.2, 0.1, 0.75, 0.8] → UV [u0,v0,u1,v1].
    approx(uv5[0], 0.2);
    approx(uv5[1], 0.1);
    approx(uv5[2], 0.95);
    approx(uv5[3], 0.9);

    let uv10 = resolve(&project, rt(10)).unwrap().layers[0].uv;
    approx(uv10[0], 0.4);
    approx(uv10[1], 0.2);
    approx(uv10[2], 0.9);
    approx(uv10[3], 0.8);
}

#[test]
fn fixed_aspect_presets_resolve_to_1080_baseline() {
    let mut project = Project::new("p", FPS_24);
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Tall9x16,
        background: [0, 0, 0],
        resolution: None,
    });
    assert_eq!(canvas_size(&project), (1080, 1920));

    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Wide16x9,
        background: [0, 0, 0],
        resolution: None,
    });
    assert_eq!(canvas_size(&project), (1920, 1080));

    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Square1x1,
        background: [0, 0, 0],
        resolution: None,
    });
    assert_eq!(canvas_size(&project), (1080, 1080));
}

/// An explicit size wins over both the aspect baseline and the footage, because
/// a delivery spec is the whole point of setting one. It survives an aspect that
/// disagrees with it, so preview and export render the pixels the project names.
#[test]
fn explicit_canvas_resolution_wins_over_aspect_and_footage() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(3840, 2160));
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    // Auto would follow the 4K footage.
    assert_eq!(canvas_size(&project), (3840, 2160));

    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Auto,
        background: [0, 0, 0],
        resolution: Some([1280, 720]),
    });
    assert_eq!(canvas_size(&project), (1280, 720));

    // A fixed aspect no longer decides the size either.
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Tall9x16,
        background: [0, 0, 0],
        resolution: Some([1280, 720]),
    });
    assert_eq!(canvas_size(&project), (1280, 720));

    // Dropping it hands the size back to the aspect.
    project.timeline_mut().set_canvas(CanvasSettings {
        aspect: CanvasAspect::Tall9x16,
        background: [0, 0, 0],
        resolution: None,
    });
    assert_eq!(canvas_size(&project), (1080, 1920));
}

#[test]
fn transform_offsets_center_and_scales_size() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_transform(
            clip,
            ClipTransform {
                position: [0.25, -0.1],
                scale: 2.0.into(),
                opacity: 0.5,
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let layer = &scene.layers[0];
    approx2(layer.center, [1920.0 * 0.75, 1080.0 * 0.4]);
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [3840.0, 2160.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
    approx(layer.opacity, 0.5);
}

#[test]
fn keyframed_position_with_tangents_resolves_off_the_line() {
    // Motion-path sampling flows through Param::sample → resolve automatically.
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Position,
            rt(0),
            ParamValue::Vec2([0.0, 0.0]),
            Easing::Linear,
            Some(SpatialTangents {
                out_t: [0.0, 0.55],
                in_t: [0.0, 0.0],
            }),
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            ClipParam::Position,
            rt(10),
            ParamValue::Vec2([1.0, 1.0]),
            Easing::Linear,
            Some(SpatialTangents {
                out_t: [0.0, 0.0],
                in_t: [-0.55, 0.0],
            }),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let center = scene.layers[0].center;
    // Straight mid position is [0.5, 0.5] → canvas center [cw, ch].
    let straight = [1920.0, 1080.0];
    assert!(
        (center[1] - straight[1]).abs() > 10.0 || (center[0] - straight[0]).abs() > 10.0,
        "expected curved mid off the straight line, got {center:?}"
    );
    // Quarter-ish bulge: y above the diagonal in position space → center_y > ch
    // relative to center_x's progress.
    let pos_x = center[0] / 1920.0 - 0.5;
    let pos_y = center[1] / 1080.0 - 0.5;
    assert!(
        pos_y > pos_x + 0.05,
        "expected position above diagonal, got ({pos_x}, {pos_y}) from center {center:?}"
    );
}

#[test]
fn per_axis_scale_stretches_media_width_only() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: Scale2 { x: 2.0, y: 1.0 },
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    match scene.layers[0].size {
        // Width doubled, height unchanged.
        SizeSpec::Fixed(size) => approx2(size, [3840.0, 1080.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn shape_stroke_uses_isotropic_scale() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let generator = Generator::Shape {
        shape: Shape::Rectangle,
        rgba: Param::Constant([255, 0, 0, 255]),
        width: Param::Constant(100.0),
        height: Param::Constant(50.0),
        corner_radius: Param::Constant(0.0),
        stroke: Some(aria_models::ShapeStroke::new([0, 0, 0, 255], 6.0)),
    };
    let clip = project.add_generated(track, generator, tr(0, 100)).unwrap();

    // Uniform [2,2]: stroke doubles (ref_scale = 1 on 1080p canvas).
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: Scale2::uniform(2.0),
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
    let scene = resolve(&project, rt(5)).unwrap();
    match &scene.layers[0].source {
        LayerSource::Shape { stroke, .. } => {
            approx(stroke.expect("stroke").width, 12.0);
        }
        other => panic!("expected shape, got {other:?}"),
    }

    // Split [4,1]: geometric mean is still 2 → stroke still doubles.
    project
        .set_transform(
            clip,
            ClipTransform {
                scale: Scale2 { x: 4.0, y: 1.0 },
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();
    let scene = resolve(&project, rt(5)).unwrap();
    match &scene.layers[0].source {
        LayerSource::Shape { stroke, .. } => {
            approx(stroke.expect("stroke").width, 12.0);
        }
        other => panic!("expected shape, got {other:?}"),
    }
    // Placement still stretches per-axis (width ×4, height ×1) plus pad.
    match scene.layers[0].size {
        SizeSpec::Fixed(size) => {
            assert!(size[0] > size[1] * 3.0, "width should dominate under [4,1]");
        }
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn look_animation_fade_in_ramps_opacity_at_clip_start() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("fade_in")))
        .unwrap();

    let start = resolve(&project, rt(0)).unwrap();
    assert!(start.layers[0].opacity < 0.05);
    let mid = resolve(&project, rt(50)).unwrap();
    approx(mid.layers[0].opacity, 1.0);
}

#[test]
fn look_animation_slide_up_offsets_center_at_start() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("slide_up")))
        .unwrap();

    let start = resolve(&project, rt(0)).unwrap();
    assert!(start.layers[0].center[1] > 540.0);
    let mid = resolve(&project, rt(50)).unwrap();
    approx2(mid.layers[0].center, [960.0, 540.0]);
}

#[test]
fn look_animation_zoom_in_scales_size_at_start() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("zoom_in")))
        .unwrap();

    let start = resolve(&project, rt(0)).unwrap();
    match start.layers[0].size {
        SizeSpec::Fixed(size) => assert!(size[0] < 1920.0 * 0.5),
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn look_animation_fade_out_ramps_at_clip_tail() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_animation(
            clip,
            AnimationSlot::Out,
            Some(AnimationRef::new("fade_out")),
        )
        .unwrap();

    let tail = resolve(&project, rt(99)).unwrap();
    assert!(tail.layers[0].opacity < 0.05);
    let mid = resolve(&project, rt(40)).unwrap();
    approx(mid.layers[0].opacity, 1.0);
}

#[test]
fn look_animation_combo_loops_and_clears_in_out() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    project
        .set_clip_animation(clip, AnimationSlot::In, Some(AnimationRef::new("fade_in")))
        .unwrap();
    project
        .set_clip_animation(clip, AnimationSlot::Combo, Some(AnimationRef::new("pulse")))
        .unwrap();

    let a = resolve(&project, rt(0)).unwrap();
    let b = resolve(&project, rt(6)).unwrap();
    approx(a.layers[0].opacity, 1.0);
    match (a.layers[0].size, b.layers[0].size) {
        (SizeSpec::Fixed(sa), SizeSpec::Fixed(sb)) => {
            assert!((sa[0] - sb[0]).abs() > 1.0);
        }
        other => panic!("expected fixed sizes, got {other:?}"),
    }
}

#[test]
fn non_center_anchor_offsets_the_quad_center() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    // Anchor on the content's top-left corner, positioned at the canvas
    // center: the quad center lands half a size down-right of it.
    project
        .set_transform(
            clip,
            ClipTransform {
                anchor_point: [0.0, 0.0],
                ..ClipTransform::IDENTITY
            },
            None,
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let layer = &scene.layers[0];
    // The layer's `center` is the anchor's canvas position...
    approx2(layer.center, [960.0, 540.0]);
    assert_eq!(layer.anchor_point, [0.0, 0.0]);
    // ...and the derived quad center hangs down-right of it.
    approx2(layer.quad_center([1920.0, 1080.0]), [1920.0, 1080.0]);
}
