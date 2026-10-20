//! The grade slider census: every surface the row list feeds must answer for
//! every row, derived from the table at run time.
//!
//! # Why
//!
//! An adjustment used to be declared six times — a struct field with its two
//! serde attributes, a `Default` line, a slider table row, a parameter axis
//! arm, a range group, a catalog row — and nothing tied them together, so a
//! slider could exist in the model with no catalog row, no range, or a write
//! path pointing at the neighbouring field. These tests read
//! [`ColorAdjustments::SLIDERS`] and demand that each surface answers for the
//! row, so a new row is covered the moment it is added and a row that reaches
//! only half the surfaces fails here.
//!
//! What they do not catch: whether the renderer reads the field. The shader
//! packs the grade into fixed vec4 lanes and is not derived from this list.

use crate::clip::{ClipParam, LookParam, ParamValue, clip_param_catalog, param_spec};
use crate::param::Param;
use crate::time::{RationalTime, TimeRange};
use crate::{ColorAdjustments, MediaSource, Project, Rational, TrackKind};

const R24: Rational = Rational::FPS_24;

/// A one-clip project on a video lane, which is what a grade needs.
fn graded_clip() -> (Project, crate::ids::ClipId) {
    let mut project = Project::new("grade", R24);
    let media = project.add_media(MediaSource::new(
        "/aria/grade-census.mp4",
        1920,
        1080,
        R24,
        480,
        false,
    ));
    let track = project.add_track(TrackKind::Video, "V1");
    let clip = project
        .add_clip(
            track,
            media,
            TimeRange::at_rate(0, 96, R24),
            RationalTime::new(0, R24),
        )
        .expect("place the clip");
    (project, clip)
}

#[test]
fn every_slider_has_a_catalog_row_that_agrees_with_its_shape() {
    let catalog = clip_param_catalog();
    for slider in ColorAdjustments::SLIDERS {
        let row = catalog
            .iter()
            .find(|row| row.id == slider.id)
            .unwrap_or_else(|| panic!("slider {} has no catalog row", slider.name));
        assert_eq!(row.group, "grade", "{} is in the wrong group", slider.name);
        assert_eq!(row.label, slider.label, "{} label drift", slider.name);
        assert_eq!(
            (row.range.min, row.range.max),
            (Some(slider.min), Some(slider.max)),
            "{} draws a different range than it validates",
            slider.name
        );
    }
    let grade_rows = catalog.iter().filter(|row| row.group == "grade").count();
    assert_eq!(
        grade_rows,
        ColorAdjustments::SLIDERS.len(),
        "the grade group and the slider table are different sizes"
    );
}

#[test]
fn every_slider_is_addressed_by_its_own_axis() {
    // A write through the axis must land on the field the row names, and on no
    // other field: the classic failure is a row wired to its neighbour.
    let (mut project, clip) = graded_clip();
    for (at, slider) in ColorAdjustments::SLIDERS.iter().enumerate() {
        let axis = axis_of(slider.name);
        let target = midpoint(slider.min, slider.max);
        project
            .set_param_constant(
                clip,
                ClipParam::Look { param: axis },
                ParamValue::Scalar(target),
            )
            .unwrap_or_else(|e| panic!("write {}: {e}", slider.name));
        let adjust = &project.clip(clip).expect("the clip").adjust;
        assert_eq!(
            (slider.get)(adjust).constant(),
            Some(target),
            "{} did not take the write",
            slider.name
        );
        for (other, elsewhere) in ColorAdjustments::SLIDERS.iter().enumerate() {
            if other <= at {
                continue;
            }
            assert!(
                elsewhere.at_neutral(adjust),
                "writing {} also moved {}",
                slider.name,
                elsewhere.name
            );
        }
    }
}

#[test]
fn a_grade_axis_refuses_a_value_its_row_does_not_allow() {
    let (mut project, clip) = graded_clip();
    for slider in ColorAdjustments::SLIDERS {
        let param = ClipParam::Look {
            param: axis_of(slider.name),
        };
        for edge in [slider.min, slider.max] {
            assert!(
                project
                    .set_param_constant(clip, param, ParamValue::Scalar(edge))
                    .is_ok(),
                "{} refused its own bound {edge}",
                slider.name
            );
        }
        for outside in [slider.min - 0.5, slider.max + 0.5, f32::NAN] {
            assert!(
                project
                    .set_param_constant(clip, param, ParamValue::Scalar(outside))
                    .is_err(),
                "{} accepted {outside}, which is outside [{}, {}]",
                slider.name,
                slider.min,
                slider.max
            );
        }
    }
}

#[test]
fn default_puts_every_slider_where_its_row_rests() {
    let (project, clip_id) = graded_clip();
    let clip = project.clip(clip_id).expect("the clip");
    let adjust = ColorAdjustments::default();
    assert_eq!(ColorAdjustments::DEFAULT, adjust);
    assert!(adjust.is_neutral(), "a fresh grade is not neutral");
    for slider in ColorAdjustments::SLIDERS {
        assert_eq!(
            (slider.get)(&adjust).constant(),
            Some(slider.neutral),
            "{} does not default to its neutral",
            slider.name
        );
        assert_eq!(
            ClipParam::Look { param: slider.axis }.default_value(clip),
            Some(ParamValue::Scalar(slider.neutral)),
            "{} default_value does not match its neutral",
            slider.name
        );
    }
    assert_eq!(
        serde_json::to_string(&adjust).expect("serialize"),
        "{}",
        "a neutral grade must not reach a save"
    );
}

#[test]
fn a_save_carries_exactly_the_sliders_that_moved() {
    for slider in ColorAdjustments::SLIDERS {
        let mut adjust = ColorAdjustments::default();
        let moved = midpoint(slider.min, slider.max);
        *(slider.get_mut)(&mut adjust) = Param::Constant(moved);
        let json: serde_json::Value =
            serde_json::from_str(&serde_json::to_string(&adjust).expect("serialize"))
                .expect("valid json");
        let object = json.as_object().expect("an object");
        assert_eq!(
            object.len(),
            1,
            "moving {} wrote {} keys",
            slider.name,
            object.len()
        );
        assert_eq!(
            object.get(slider.name).and_then(serde_json::Value::as_f64),
            Some(f64::from(moved)),
            "{} is saved under another name",
            slider.name
        );
        let back: ColorAdjustments =
            serde_json::from_value(json).expect("a grade reads its own save");
        assert_eq!(back, adjust, "{} did not round-trip", slider.name);
    }
}

#[test]
fn a_save_written_without_a_slider_reads_it_at_neutral() {
    // The shape of an older project: the keys this build knows are absent, and
    // a key it does not know is present.
    let older: ColorAdjustments =
        serde_json::from_str(r#"{"brightness":0.5,"sparkle":0.9}"#).expect("an older save");
    assert_eq!(older.brightness.constant(), Some(0.5));
    for slider in ColorAdjustments::SLIDERS {
        if slider.name == "brightness" {
            continue;
        }
        assert!(
            slider.at_neutral(&older),
            "{} did not read back at neutral",
            slider.name
        );
    }
    let twice = serde_json::from_str::<ColorAdjustments>(r#"{"contrast":0.1,"contrast":0.2}"#);
    assert!(twice.is_err(), "a doubled slider must be refused");
}

/// The axis a slider is addressed by, looked up the way a surface would.
fn axis_of(name: &str) -> LookParam {
    let found = clip_param_catalog()
        .iter()
        .find(|row| {
            row.group == "grade"
                && ColorAdjustments::SLIDERS
                    .iter()
                    .any(|s| s.name == name && s.id == row.id)
        })
        .unwrap_or_else(|| panic!("no catalog row for {name}"));
    match found.param {
        ClipParam::Look { param } => {
            assert!(
                param_spec(found.param).is_some(),
                "{name} has no static row"
            );
            param
        }
        other => panic!("{name} is addressed as {other:?}, not a look parameter"),
    }
}

/// A value inside the row's range that is not its neutral, so a write is
/// observable.
fn midpoint(min: f32, max: f32) -> f32 {
    let mid = (min + max) / 2.0;
    if mid == 0.0 { max / 2.0 } else { mid }
}
