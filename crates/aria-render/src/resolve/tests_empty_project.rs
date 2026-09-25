//! `empty_project` cases.

use super::*;

#[test]
fn empty_project_uses_default_canvas_and_has_no_layers() {
    let project = Project::new("p", FPS_24);
    let scene = resolve(&project, rt(0)).unwrap();
    assert_eq!((scene.width, scene.height), (1920, 1080));
    assert!(scene.layers.is_empty());
}

#[test]
fn single_video_clip_is_centered_and_aspect_fit() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!((scene.width, scene.height), (1920, 1080));
    assert_eq!(scene.layers.len(), 1);

    let layer = &scene.layers[0];
    approx2(layer.center, [960.0, 540.0]);
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [1920.0, 1080.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
    approx(layer.opacity, 1.0);
    match &layer.source {
        LayerSource::Media {
            media: m,
            source_time,
        } => {
            assert_eq!(*m, media);
            assert_eq!(source_time.value, 5);
        }
        other => panic!("expected media source, got {other:?}"),
    }
}

#[test]
fn clip_inactive_at_time_produces_no_layer() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, media, tr(0, 10), rt(0)).unwrap();

    let scene = resolve(&project, rt(50)).unwrap();
    assert!(scene.layers.is_empty());
}

#[test]
fn tracks_stack_bottom_to_top() {
    let mut project = Project::new("p", FPS_24);
    let bottom = project.add_track(TrackKind::Sticker, "S1");
    let top = project.add_track(TrackKind::Sticker, "S2");
    project
        .add_generated(
            bottom,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    project
        .add_generated(
            top,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(0, 100),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    assert_eq!(scene.layers[0].source, LayerSource::Solid([255, 0, 0, 255]));
    assert_eq!(scene.layers[1].source, LayerSource::Solid([0, 0, 255, 255]));
}

#[test]
fn text_generator_maps_style_and_defers_size() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Text, "T1");
    let style = ModelTextStyle {
        size: 90.0.into(),
        bold: true,
        italic: true,
        underline: true,
        fill: [255, 0, 0, 255].into(),
        letter_spacing: 12.0.into(),
        line_spacing: 1.5.into(),
        align_h: aria_models::TextAlignH::Right,
        align_v: aria_models::TextAlignV::Bottom,
        stroke: Some(aria_models::TextStroke {
            rgba: [0, 0, 0, 255].into(),
            width: 8.0.into(),
        }),
        background: Some(aria_models::TextBackground {
            rgba: [0, 0, 255, 200].into(),
            radius: 0.5.into(),
        }),
        shadow: Some(aria_models::TextShadow {
            rgba: [0, 0, 0, 230].into(),
            blur: 0.15.into(),
            distance: 10.0.into(),
        }),
        ..ModelTextStyle::default()
    };
    project
        .add_generated(
            track,
            Generator::Text {
                content: "Hi".into(),
                style,
            },
            tr(0, 100),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!((scene.width, scene.height), (1920, 1080));
    assert_eq!(scene.layers.len(), 1);
    let layer = &scene.layers[0];
    approx2(layer.center, [960.0, 540.0]);
    assert_eq!(layer.size, SizeSpec::BitmapScaled([1.0, 1.0]));
    match &layer.source {
        LayerSource::Text {
            content,
            style,
            animation,
        } => {
            assert!(animation.is_none());
            assert_eq!(content, "Hi");
            approx(style.font_size, 90.0);
            assert_eq!(style.color, [255, 0, 0, 255]);
            assert!(style.bold);
            assert!(style.italic);
            assert!(style.underline);
            approx(style.letter_spacing, 12.0);
            approx(style.line_height, 135.0);
            assert_eq!(style.align, aria_text::TextAlign::Right);
            assert_eq!(style.vertical_align, aria_text::TextVerticalAlign::Bottom);
            assert_eq!(style.max_width, Some(1920.0));
            let stroke = style.stroke.expect("stroke mapped");
            assert_eq!(stroke.rgba, [0, 0, 0, 255]);
            approx(stroke.width, 8.0);
            let bg = style.background.expect("background mapped");
            assert_eq!(bg.rgba, [0, 0, 255, 200]);
            approx(bg.radius, 0.5);
            let shadow = style.shadow.expect("shadow mapped");
            assert_eq!(shadow.rgba, [0, 0, 0, 230]);
            approx(shadow.blur, 0.15);
            approx(shadow.distance, 10.0);
        }
        other => panic!("expected text source, got {other:?}"),
    }
}

#[test]
fn text_wrap_toggle_removes_the_canvas_width_constraint() {
    let style = ModelTextStyle {
        wrap: false,
        ..ModelTextStyle::default()
    };
    assert_eq!(map_text_style(&style, 1920.0, 1080.0, 0).max_width, None);
}

#[test]
fn keyframed_text_background_samples_card_per_tick() {
    use aria_models::{ClipParam, Easing, ParamValue, TextParam};

    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "Card".into(),
                style: ModelTextStyle {
                    background: Some(aria_models::TextBackground::default()),
                    ..ModelTextStyle::default()
                },
            },
            tr(0, 100),
        )
        .unwrap();
    let color = ClipParam::Text {
        param: TextParam::BackgroundColor,
    };
    let radius = ClipParam::Text {
        param: TextParam::BackgroundRadius,
    };
    project
        .set_param_keyframe(
            clip,
            color,
            rt(0),
            ParamValue::Color([0, 0, 0, 255]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            color,
            rt(50),
            ParamValue::Color([200, 100, 0, 255]),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            radius,
            rt(0),
            ParamValue::Scalar(0.0),
            Easing::Linear,
            None,
        )
        .unwrap();
    project
        .set_param_keyframe(
            clip,
            radius,
            rt(50),
            ParamValue::Scalar(1.0),
            Easing::Linear,
            None,
        )
        .unwrap();

    let scene = resolve(&project, rt(25)).unwrap();
    match &scene.layers[0].source {
        LayerSource::Text { style, .. } => {
            let bg = style.background.expect("background");
            assert_eq!(bg.rgba, [100, 50, 0, 255], "card color lerps per channel");
            approx(bg.radius, 0.5);
        }
        other => panic!("expected text source, got {other:?}"),
    }
}

#[test]
fn solid_generator_fills_canvas() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [10, 20, 30, 255],
            },
            tr(0, 100),
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let layer = &scene.layers[0];
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [1920.0, 1080.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
}

#[test]
fn sticker_generator_resolves_to_a_reference_scaled_layer() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(track, Generator::sticker("heart"), tr(0, 100))
        .unwrap();

    let scene = resolve(&project, rt(12)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    let layer = &scene.layers[0];
    approx2(layer.center, [960.0, 540.0]);
    // The heart asset is 256×256; intrinsic pixels are reference pixels,
    // which map 1:1 on the 1080p canvas.
    match layer.size {
        SizeSpec::Fixed(size) => approx2(size, [256.0, 256.0]),
        other => panic!("expected fixed size, got {other:?}"),
    }
    match &layer.source {
        LayerSource::Sticker { asset, local_time } => {
            assert_eq!(asset, "heart");
            approx(*local_time as f32, 0.5); // tick 12 at 24 fps
        }
        other => panic!("expected sticker source, got {other:?}"),
    }
}

#[test]
fn sticker_local_time_is_clip_relative() {
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(track, Generator::sticker("star_spin"), tr(24, 100))
        .unwrap();

    let scene = resolve(&project, rt(36)).unwrap();
    match &scene.layers[0].source {
        LayerSource::Sticker { local_time, .. } => approx(*local_time as f32, 0.5),
        other => panic!("expected sticker source, got {other:?}"),
    }
}

#[test]
fn empty_or_unknown_sticker_asset_places_nothing() {
    // The legacy payload-less sticker (empty asset) loads but draws
    // nothing, exactly like before the catalog existed.
    let mut project = Project::new("p", FPS_24);
    let track = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(track, Generator::sticker(""), tr(0, 100))
        .unwrap();
    assert!(resolve(&project, rt(5)).unwrap().layers.is_empty());

    // Unknown ids can't enter via commands (validation), but the
    // resolver stays graceful if one ever reaches it.
    let layer = resolve_generator(
        &Generator::sticker("not-a-sticker"),
        [960.0, 540.0],
        [0.5, 0.5],
        0.0,
        1.0,
        [0.0, 0.0, 1.0, 1.0],
        None,
        None,
        1920.0,
        1080.0,
        Scale2::ONE,
        0,
        0.0,
        Vec::new(),
    );
    assert!(layer.is_none());
}

#[test]
fn crop_and_horizontal_flip_set_uv() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_clip_crop(
            clip,
            CropRect {
                x: 0.25,
                y: 0.1,
                w: 0.5,
                h: 0.8,
            },
            true,
            false,
            None,
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    let uv = scene.layers[0].uv;
    // base [0.25, 0.1, 0.75, 0.9]; flip_h swaps the u axis.
    approx(uv[0], 0.75);
    approx(uv[1], 0.1);
    approx(uv[2], 0.25);
    approx(uv[3], 0.9);
}

/// The crop override replaces a clip's crop and mirrors for one render, which is
/// what lets an on-canvas crop drag show the frame it is cutting into. It names one
/// clip: a second clip renders what the project holds.
#[test]
fn a_crop_override_replaces_one_clip_s_crop_for_one_render() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    let other = project.add_track(TrackKind::Video, "V2");
    let other_clip = project.add_clip(other, media, tr(0, 100), rt(0)).unwrap();
    let cropped = CropRect {
        x: 0.25,
        y: 0.1,
        w: 0.5,
        h: 0.8,
    };
    for id in [clip, other_clip] {
        project
            .set_clip_crop(id, cropped, true, false, None)
            .unwrap();
    }

    let overrides = ResolveOverrides {
        crop: Some((clip, CropRect::FULL, false, false)),
        ..ResolveOverrides::default()
    };
    let scene = resolve_with(&project, rt(5), overrides).unwrap();
    let overridden = scene
        .layers
        .iter()
        .find(|layer| layer.clip == Some(clip))
        .expect("the overridden clip");
    assert_eq!(
        overridden.uv,
        [0.0, 0.0, 1.0, 1.0],
        "the override must show the whole frame, mirrors included"
    );
    let untouched = scene
        .layers
        .iter()
        .find(|layer| layer.clip == Some(other_clip))
        .expect("the other clip");
    approx(untouched.uv[0], 0.75);
    approx(untouched.uv[2], 0.25);

    // Without the override both clips are back to the project's crop.
    let scene = resolve(&project, rt(5)).unwrap();
    for layer in &scene.layers {
        approx(layer.uv[0], 0.75);
        approx(layer.uv[2], 0.25);
    }
}
