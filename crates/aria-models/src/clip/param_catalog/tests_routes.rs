//! A row's route: that it finds a value, that it goes quiet when the block it
//! needs is absent, and that it reads the same field a command writes.

use super::*;

#[test]
fn every_row_samples_the_kind_it_declares() {
    let clips = all_dressed();
    for spec in clip_param_catalog() {
        let Some((clip, value)) = clips
            .iter()
            .find_map(|clip| spec.sample(clip, 0).map(|value| (clip, value)))
        else {
            panic!(
                "{} has no route on any dressed clip, so nothing can read it",
                spec.id
            );
        };
        let got = match value {
            ParamValue::Scalar(_) => ParamKind::Scalar,
            ParamValue::Vec2(_) => ParamKind::Vec2,
            ParamValue::Vec3(_) => ParamKind::Vec3,
            ParamValue::Color(_) => ParamKind::Color,
            ParamValue::Rect(_) => ParamKind::Rect,
        };
        assert_eq!(
            got, spec.kind,
            "{} declares {:?} but samples {got:?}",
            spec.id, spec.kind
        );
        assert!(
            spec.available(clip),
            "{} sampled a value but reports itself unavailable",
            spec.id
        );
    }
}

#[test]
fn a_row_whose_block_is_absent_is_unavailable_and_samples_nothing() {
    // A bare generated clip: no filter, LUT, mask, key, matte, styles, and not
    // a title or a shape. Every row that depends on a block must go quiet.
    let bare = Clip::generated(
        Generator::SolidColor {
            rgba: [0, 0, 0, 255],
        },
        tr(0, 48),
    );
    let mut blocked = 0;
    for spec in clip_param_catalog() {
        if spec.available(&bare) {
            assert!(
                spec.sample(&bare, 0).is_some(),
                "{} reports available but samples nothing",
                spec.id
            );
        } else {
            assert!(
                spec.sample(&bare, 0).is_none(),
                "{} reports unavailable but still samples a value",
                spec.id
            );
            assert!(
                spec.keyframes(&bare).is_empty(),
                "{} reports unavailable but still lists keyframes",
                spec.id
            );
            blocked += 1;
        }
    }
    assert!(
        blocked > 20,
        "a bare clip should gate most rows; only {blocked} were gated"
    );
}

#[test]
fn a_row_reports_the_keyframes_written_on_its_route() {
    // Written through the model's own write path, so the route the catalog reads
    // and the route a command writes have to be the same one.
    let mut project = crate::Project::new("p", FPS_24);
    let track = project.add_track(crate::TrackKind::Text, "T1");
    let clip = project
        .add_generated(
            track,
            Generator::Text {
                content: "Aria".into(),
                style: TextStyle::default(),
            },
            tr(0, 48),
        )
        .expect("clip");
    let spec = param_spec(ClipParam::Text {
        param: TextParam::Size,
    })
    .expect("text size row");

    project
        .set_param_keyframe(
            clip,
            spec.param,
            crate::RationalTime::new(0, FPS_24),
            ParamValue::Scalar(40.0),
            Easing::Linear,
            None,
        )
        .expect("first key");
    project
        .set_param_keyframe(
            clip,
            spec.param,
            crate::RationalTime::new(24, FPS_24),
            ParamValue::Scalar(80.0),
            Easing::EaseIn,
            None,
        )
        .expect("second key");

    let clip = project.timeline().clip(clip).expect("clip");
    assert_eq!(
        spec.keyframes(clip),
        vec![(0, Easing::Linear), (24, Easing::EaseIn)]
    );
    assert_eq!(spec.easing_at(clip, 24), Some(Easing::EaseIn));
    assert_eq!(spec.easing_at(clip, 12), None);
    // Sampling rides the same route, so the midpoint is the interpolation.
    assert_eq!(spec.sample(clip, 12), Some(ParamValue::Scalar(60.0)));
}
