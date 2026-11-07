//! The refusal of a keyframe removal that finds no keyframe.
//!
//! A script removing a keyframe that is not there used to read "no Opacity
//! keyframe at tick 15": the property's Rust name and a clip-local tick, where
//! the caller passed a row id and seconds on the timeline. Every row
//! `clip_params_for` offers on every dressed clip shape, a mask row and an
//! effect parameter included, must state its own row id and the time into the
//! clip, so a new row is held to the copy without an entry here. Does not
//! check the refusal a row the clip does not offer gets; the erase states that.

use super::*;
use crate::time::RationalTime;

/// Every dressed clip shape as a project, with an effect on the media clip so
/// effect parameters have rows.
fn projects() -> Vec<(crate::Project, crate::ClipId)> {
    let mut projects: Vec<_> = [
        fully_dressed_clip(),
        dressed_text_clip(),
        dressed_shape_clip(),
    ]
    .into_iter()
    .map(generated_project)
    .collect();
    let (mut media, id) = dressed_media_project();
    media.add_effect(id, "gaussian_blur").expect("effect");
    projects.push((media, id));
    projects
}

#[test]
fn a_removal_with_no_keyframe_states_the_row_id_and_seconds_into_the_clip() {
    let at = RationalTime::new(12, FPS_24);
    let (mut reached, mut dynamic) = (0, 0);
    for (mut project, id) in projects() {
        let clip = project.require_clip(id).expect("clip").clone();
        for row in clip_params_for(&clip) {
            dynamic += usize::from(param_spec(row.param).is_none());
            let refusal = project
                .remove_param_keyframe(id, row.param, at)
                .expect_err("no keyframe sits at 0.5 s");
            assert_eq!(
                refusal.to_string(),
                format!(
                    "invalid parameter: {} has no keyframe 0.500 s into the clip",
                    row.id
                ),
                "the refusal for row {}",
                row.id
            );
            reached += 1;
        }
    }
    assert!(reached > 50, "the census reached {reached} rows");
    assert!(dynamic > 0, "the census reached no mask or effect row");
}

#[test]
fn a_keyframe_position_off_the_clip_states_the_span_in_seconds() {
    for (mut project, id) in projects() {
        let refusal = project
            .remove_param_keyframe(id, ClipParam::Opacity, RationalTime::new(60, FPS_24))
            .expect_err("2.5 s is past the clip");
        assert_eq!(
            refusal.to_string(),
            "invalid parameter: keyframe position 2.500 s is outside the clip, which runs \
             0.000 s to 2.000 s"
        );
    }
}

/// The time is measured from the clip's start, not the timeline's.
#[test]
fn a_removal_on_a_clip_that_starts_late_states_the_time_into_the_clip() {
    let mut late = dressed_shape_clip();
    late.timeline = tr(24, 48);
    let (mut project, id) = generated_project(late);
    let refusal = project
        .remove_param_keyframe(id, ClipParam::Opacity, RationalTime::new(36, FPS_24))
        .expect_err("no keyframe sits 0.5 s into the clip");
    assert_eq!(
        refusal.to_string(),
        "invalid parameter: opacity has no keyframe 0.500 s into the clip"
    );
    let refusal = project
        .remove_param_keyframe(id, ClipParam::Opacity, RationalTime::new(12, FPS_24))
        .expect_err("0.5 s is before the clip");
    assert_eq!(
        refusal.to_string(),
        "invalid parameter: keyframe position 0.500 s is outside the clip, which runs \
         1.000 s to 3.000 s"
    );
}
