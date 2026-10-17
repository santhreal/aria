//! `quad_center` cases.

use super::*;

#[test]
fn quad_center_rotates_about_the_anchor() {
    let layer = SceneLayer {
        clip: None,
        source: LayerSource::Solid([255, 255, 255, 255]),
        center: [960.0, 540.0],
        anchor_point: [0.0, 0.0],
        size: SizeSpec::Fixed([1920.0, 1080.0]),
        rotation: std::f32::consts::FRAC_PI_2, // 90° clockwise
        opacity: 1.0,
        uv: [0.0, 0.0, 1.0, 1.0],
        effects: Vec::new(),
        masks: Vec::new(),
        chroma_key: None,
        color_grade: None,
        lut: None,
        blend_mode: BlendMode::Normal,
        styles: None,
        blur_passes: Vec::new(),
    };
    // to_center (960, 540) rotated 90° cw (+y down) → (-540, 960).
    approx2(layer.quad_center([1920.0, 1080.0]), [420.0, 1500.0]);
}

#[test]
fn resolve_with_substitutes_transform_and_generator_for_one_clip() {
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

    let overrides = ResolveOverrides {
        transform: Some((
            clip,
            ClipTransform {
                position: [0.25, 0.0],
                scale: 0.5.into(),
                ..ClipTransform::IDENTITY
            },
        )),
        generator: Some((
            clip,
            &Generator::SolidColor {
                rgba: [0, 255, 0, 255],
            },
        )),
        look: Some((
            clip,
            None,
            &ColorAdjustments {
                brightness: 0.25.into(),
                ..ColorAdjustments::default()
            },
        )),
        styles: None,
        animation: None,
        crop: None,
        effect_vec2: None,
        transparent: false,
    };
    let scene = resolve_with(&project, rt(5), overrides).unwrap();
    let layer = &scene.layers[0];
    assert_eq!(layer.source, LayerSource::Solid([0, 255, 0, 255]));
    assert_eq!(
        layer.color_grade,
        Some(effective_grade(
            None,
            &ColorAdjustments {
                brightness: 0.25.into(),
                ..ColorAdjustments::default()
            }
        ))
    );
    approx2(layer.center, [1920.0 * 0.75, 540.0]);
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [960.0, 540.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }

    // Plain resolve still sees only committed state.
    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers[0].source, LayerSource::Solid([255, 0, 0, 255]));
    assert_eq!(scene.layers[0].color_grade, None);
    approx2(scene.layers[0].center, [960.0, 540.0]);
}

#[test]
fn image_clip_resolves_to_an_aspect_fit_still_layer() {
    let mut project = Project::new("p", FPS_24);
    // 800×600 still on the default 1920×1080 canvas: fit = 1.8.
    let media = project.add_media(MediaSource::image("/photos/pic.png", 800, 600));
    let window = project.media(media).unwrap().full_range();
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, media, window, rt(0)).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    let layer = &scene.layers[0];
    assert_eq!(layer.source, LayerSource::Still { media });
    approx2(layer.center, [960.0, 540.0]);
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [1440.0, 1080.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn still_layer_ignores_retiming_and_covers_the_whole_clip() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(MediaSource::image("/photos/pic.png", 1920, 1080));
    let window = project.media(media).unwrap().full_range();
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, window, rt(0)).unwrap();
    // Reverse would make a video clip walk its source backward; a still
    // must keep producing its one frame at every covered tick.
    project
        .set_clip_speed(clip, Rational::new(1, 1), true)
        .unwrap();
    let duration = project.timeline().duration().value;
    assert!(duration > 0);

    for t in [0, duration / 2, duration - 1] {
        let scene = resolve(&project, rt(t)).unwrap();
        assert_eq!(scene.layers.len(), 1, "tick {t}");
        assert_eq!(scene.layers[0].source, LayerSource::Still { media });
    }
    // Past the clip: nothing.
    let scene = resolve(&project, rt(duration)).unwrap();
    assert!(scene.layers.is_empty());
}

#[test]
fn audio_track_is_skipped() {
    let mut project = Project::new("p", FPS_24);
    let video_track = project.add_track(TrackKind::Video, "V1");
    let media = project.add_media(video(1920, 1080));
    project
        .add_clip(video_track, media, tr(0, 100), rt(0))
        .unwrap();

    let audio_track = project.add_track(TrackKind::Audio, "A1");
    let song = project.add_media(MediaSource::new("/tmp/a.mp3", 0, 0, FPS_24, 600, true));
    project
        .add_clip(audio_track, song, tr(0, 100), rt(0))
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(matches!(scene.layers[0].source, LayerSource::Media { .. }));
}

#[test]
fn plain_rectangle_keeps_the_solid_fast_path() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::shape(Shape::Rectangle, [255, 255, 255, 255]),
            tr(0, 100),
        )
        .unwrap();
    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(matches!(scene.layers[0].source, LayerSource::Solid(_)));
    // Drop size 200×200 reference px on a 1080 canvas: 1:1.
    assert_eq!(scene.layers[0].size, SizeSpec::Fixed([200.0, 200.0]));
}

#[test]
fn ellipse_resolves_to_a_padded_sdf_layer() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::shape(Shape::Ellipse, [10, 20, 30, 255]),
            tr(0, 100),
        )
        .unwrap();
    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    let layer = &scene.layers[0];
    match &layer.source {
        LayerSource::Shape {
            params,
            fill,
            stroke,
            pad,
        } => {
            assert_eq!(*params, aria_shapes::SdfParams::Ellipse);
            assert_eq!(*fill, [10, 20, 30, 255]);
            assert!(stroke.is_none());
            // No stroke: pad is the 2px AA margin only.
            approx(*pad, 2.0 * aria_shapes::SDF_AA);
        }
        other => panic!("expected shape source, got {other:?}"),
    }
    // Quad = 200×200 shape + pad on each side.
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [204.0, 204.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn rounded_or_stroked_rectangle_leaves_the_fast_path() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let generator = Generator::Shape {
        shape: Shape::Rectangle,
        rgba: aria_models::Param::Constant([255, 0, 0, 255]),
        width: aria_models::Param::Constant(100.0),
        height: aria_models::Param::Constant(50.0),
        corner_radius: aria_models::Param::Constant(8.0),
        stroke: Some(aria_models::ShapeStroke::new([0, 0, 0, 255], 6.0)),
    };
    project.add_generated(track, generator, tr(0, 100)).unwrap();
    let scene = resolve(&project, rt(5)).unwrap();
    let layer = &scene.layers[0];
    match &layer.source {
        LayerSource::Shape {
            params,
            stroke,
            pad,
            ..
        } => {
            assert_eq!(*params, aria_shapes::SdfParams::RoundedRect { radius: 8.0 });
            let s = stroke.expect("stroke resolved");
            approx(s.width, 6.0);
            // Pad covers the stroke's outward half plus the AA margin.
            approx(*pad, 3.0 + 2.0 * aria_shapes::SDF_AA);
        }
        other => panic!("expected shape source, got {other:?}"),
    }
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [110.0, 60.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn animated_shape_params_sample_at_the_clip_tick() {
    use aria_models::{ClipParam, Easing, ParamValue, ShapeParam};

    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let clip = project
        .add_generated(
            track,
            Generator::shape(
                Shape::Star {
                    points: 5,
                    inner_ratio: aria_models::Param::Constant(0.5),
                },
                [255, 255, 255, 255],
            ),
            tr(0, 100),
        )
        .unwrap();
    let width = ClipParam::Shape {
        param: ShapeParam::Width,
    };
    let fill = ClipParam::Shape {
        param: ShapeParam::Fill,
    };
    for (param, at, value) in [
        (width, 0, ParamValue::Scalar(100.0)),
        (width, 50, ParamValue::Scalar(300.0)),
        (fill, 0, ParamValue::Color([0, 0, 0, 255])),
        (fill, 50, ParamValue::Color([200, 100, 0, 255])),
    ] {
        project
            .set_param_keyframe(clip, param, rt(at), value, Easing::Linear, None)
            .unwrap();
    }

    let scene = resolve(&project, rt(25)).unwrap();
    let layer = &scene.layers[0];
    match &layer.source {
        LayerSource::Shape {
            params, fill, pad, ..
        } => {
            assert!(matches!(
                params,
                aria_shapes::SdfParams::Star { points: 5, .. }
            ));
            assert_eq!(*fill, [100, 50, 0, 255], "colors lerp per channel");
            // Width halfway between 100 and 300 → 200 + 2·pad quad.
            match layer.size {
                SizeSpec::Fixed(size) => approx(size[0], 200.0 + 2.0 * pad),
                other => panic!("expected fixed size, got {other:?}"),
            }
        }
        other => panic!("expected shape source, got {other:?}"),
    }
}

#[test]
fn pen_path_resolves_to_a_bitmap_scaled_path_layer() {
    use aria_models::{ShapePath, ShapePathPoint};

    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let path = ShapePath {
        points: vec![
            ShapePathPoint::corner([-40.0, -40.0]),
            ShapePathPoint::corner([40.0, -40.0]),
            ShapePathPoint::corner([0.0, 40.0]),
        ],
        closed: true,
    };
    project
        .add_generated(
            track,
            Generator::shape(Shape::Path(path), [0, 255, 0, 255]),
            tr(0, 100),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let layer = &scene.layers[0];
    match &layer.source {
        LayerSource::PathShape {
            path,
            fill,
            raster_scale,
            ..
        } => {
            assert_eq!(path.points.len(), 3);
            assert!(path.closed);
            assert_eq!(*fill, [0, 255, 0, 255]);
            approx(*raster_scale, 1.0); // 1080 canvas → reference scale 1
        }
        other => panic!("expected path source, got {other:?}"),
    }
    // Transform scale rides the quad, not the raster.
    assert_eq!(layer.size, SizeSpec::BitmapScaled([1.0, 1.0]));
}

#[test]
fn untouched_clip_resolves_to_identity_grade() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers[0].color_grade, None);
}

#[test]
fn clip_adjust_and_filter_flow_into_scene_layer_grade() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    let adjust_clip = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    project
        .set_clip_adjustments(
            adjust_clip,
            ColorAdjustments {
                saturation: (-1.0).into(),
                ..ColorAdjustments::default()
            },
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let expected = effective_grade(
        None,
        &ColorAdjustments {
            saturation: (-1.0).into(),
            ..ColorAdjustments::default()
        },
    );
    assert_eq!(scene.layers[0].color_grade, Some(expected));
    assert_eq!(scene.layers[0].color_grade.unwrap().saturation, -1.0);

    let filter_track = project.add_track(TrackKind::Sticker, "S2");
    let filter_clip = project
        .add_generated(
            filter_track,
            Generator::SolidColor {
                rgba: [0, 255, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    let filter = Filter {
        id: "mono".into(),
        intensity: 0.8.into(),
    };
    project
        .set_clip_filter(filter_clip, Some(filter.clone()))
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let filter_layer = scene
        .layers
        .iter()
        .find(|l| matches!(l.source, LayerSource::Solid([0, 255, 0, 255])))
        .expect("filter clip layer");
    assert_eq!(
        filter_layer.color_grade,
        Some(effective_grade(Some(&filter), &ColorAdjustments::default()))
    );
}
