//! `resolve_with` cases.

use super::*;

#[test]
fn clip_without_styles_resolves_styles_none() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 1);
    assert!(scene.layers[0].styles.is_none());
}

#[test]
fn resolve_with_substitutes_styles_for_one_clip() {
    let mut project = Project::new("p", FPS_24);
    let media = project.add_media(video(1920, 1080));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project.add_clip(track, media, tr(0, 100), rt(0)).unwrap();
    project
        .set_layer_styles(
            clip,
            LayerStyles {
                shadow: Some(LayerShadow {
                    blur: Param::Constant(8.0),
                    ..LayerShadow::default()
                }),
                ..Default::default()
            },
        )
        .unwrap();

    let live = LayerStyles {
        shadow: Some(LayerShadow {
            blur: Param::Constant(24.0),
            ..LayerShadow::default()
        }),
        ..Default::default()
    };
    let overrides = ResolveOverrides {
        transform: None,
        generator: None,
        look: None,
        styles: Some((clip, &live)),
        animation: None,
        crop: None,
    };
    let scene = resolve_with(&project, rt(5), overrides).unwrap();
    approx(
        scene.layers[0]
            .styles
            .as_ref()
            .unwrap()
            .shadow
            .as_ref()
            .unwrap()
            .blur,
        24.0,
    );

    // Plain resolve still sees only committed state.
    let scene = resolve(&project, rt(5)).unwrap();
    approx(
        scene.layers[0]
            .styles
            .as_ref()
            .unwrap()
            .shadow
            .as_ref()
            .unwrap()
            .blur,
        8.0,
    );
}

#[test]
fn adjustment_lane_styles_stay_none_on_canvas_pass() {
    let mut project = Project::new("p", FPS_24);
    let base = project.add_track(TrackKind::Sticker, "S1");
    project
        .add_generated(
            base,
            Generator::SolidColor {
                rgba: [255, 0, 0, 255],
            },
            tr(0, 100),
        )
        .unwrap();
    let adjustment = project.add_track(TrackKind::Adjustment, "A1");
    let bar = project
        .add_generated(adjustment, Generator::Adjustment, tr(0, 100))
        .unwrap();
    project
        .set_clip_adjustments(
            bar,
            ColorAdjustments {
                saturation: (-1.0).into(),
                ..ColorAdjustments::default()
            },
        )
        .unwrap();
    project
        .set_layer_styles(
            bar,
            LayerStyles {
                shadow: Some(LayerShadow::default()),
                ..Default::default()
            },
        )
        .unwrap();

    let scene = resolve(&project, rt(5)).unwrap();
    assert_eq!(scene.layers.len(), 2);
    assert_eq!(scene.layers[1].source, LayerSource::CanvasPass);
    assert!(scene.layers[1].styles.is_none());
}

/// A catalog hover previews an animation the project does not carry. The class closed
/// here is "a live override is honoured for the transform but silently dropped for the
/// animation slots", which would make hovering look like a dead row.
#[test]
fn resolve_with_substitutes_animation_slots_for_one_clip() {
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

    // Enumerated from the catalog so a renamed or removed preset fails here rather
    // than leaving the override untested.
    let entrance = aria_models::animation_catalog()
        .iter()
        .find(|spec| spec.slot == aria_models::AnimationSlot::In && !spec.text_only)
        .expect("the catalog offers an entrance for a media clip");

    let committed = resolve(&project, rt(1)).unwrap();
    let baseline = &committed.layers[0];

    let animation = aria_models::AnimationRef::new(&entrance.id);
    let anims = aria_models::LookAnimations::of(project.timeline().clip(clip).unwrap())
        .with_slot(aria_models::AnimationSlot::In, Some(&animation));
    let scene = resolve_with(
        &project,
        rt(1),
        ResolveOverrides {
            transform: None,
            generator: None,
            look: None,
            styles: None,
            animation: Some((clip, anims)),
            crop: None,
        },
    )
    .unwrap();
    let previewed = &scene.layers[0];
    assert!(
        previewed.opacity != baseline.opacity
            || previewed.center != baseline.center
            || previewed.rotation != baseline.rotation
            || previewed.size != baseline.size,
        "{} previewed as no visible change at all",
        entrance.id
    );

    // The override is keyed on one clip, and the project is untouched by it.
    let other = project
        .add_generated(
            track,
            Generator::SolidColor {
                rgba: [0, 0, 255, 255],
            },
            tr(200, 300),
        )
        .unwrap();
    let animation = aria_models::AnimationRef::new(&entrance.id);
    let anims = aria_models::LookAnimations::of(project.timeline().clip(other).unwrap())
        .with_slot(aria_models::AnimationSlot::In, Some(&animation));
    let elsewhere = resolve_with(
        &project,
        rt(1),
        ResolveOverrides {
            transform: None,
            generator: None,
            look: None,
            styles: None,
            animation: Some((other, anims)),
            crop: None,
        },
    )
    .unwrap();
    assert_eq!(elsewhere.layers[0].opacity, baseline.opacity);
    assert_eq!(elsewhere.layers[0].center, baseline.center);
    assert_eq!(
        resolve(&project, rt(1)).unwrap().layers[0].opacity,
        baseline.opacity
    );
}
