//! The graph editor's value axis prints in the units its property's row prints.
//!
//! The class closed here: an axis label printing the stored number while the
//! row beside it prints the shown one, so an opacity's axis read `1.1` over a
//! row reading `100%`, and a gain's read a multiplier over a row in decibels.
//! The rows are read from each fixture clip's projection when the test runs,
//! so a property added to the catalog, an effect's parameter or a lane's
//! audio properties are checked without an edit here.
//!
//! In Value mode each label must be what the row prints for the label's
//! stored value. In Speed mode it is that number's rate, with `/s` after it; a
//! gain's rate prints as a multiplier's, in [`Units::SPEED`]. Curves whose rows
//! print in different units share one axis, which then prints stored numbers.
//!
//! Not covered: where on the axis a label is drawn, which
//! `the_graph_axis_stands_on_the_curve_ys` reads, and a colour or rectangle
//! property, which the graph does not plot.

use aria_models::{ClipId, ClipParam, ParamKind, ParamValue};

use super::GraphMode;
use crate::app::AppState;
use crate::app::clip_keys::key_params_of;
use crate::app::fixtures::{a_title, blurred_clip, keyed_mask_path, with_audio};
use crate::app::param_units::{Scale, Units};
use crate::app::property_rows::format_param_value;

/// A clip holding every row kind the fixtures can dress: a still with an
/// effect and a mask, a title, and a clip on an audio lane.
fn fixtures() -> Vec<(AppState, ClipId)> {
    let (mut masked, still) = blurred_clip();
    keyed_mask_path(&mut masked);
    let title = a_title();
    let titled = title.selected_clip().expect("the title is selected");
    vec![(masked, still), (title, titled), with_audio()]
}

/// The labels the graph draws for `params` of `clip` in `mode`, and the
/// stored values they stand at, top first.
fn axis(
    state: &mut AppState,
    clip: ClipId,
    params: &[ClipParam],
    mode: GraphMode,
) -> [(String, f32); 3] {
    for (at, &param) in params.iter().enumerate() {
        state.select_timeline_property(clip, param, at > 0);
    }
    state.set_timeline_graph_mode(mode);
    let model = state.build_timeline_graph_model(24.0, 120.0, 0.0, 160.0);
    let drawn: Vec<ClipParam> = model.curves.iter().map(|curve| curve.param).collect();
    assert_eq!(drawn, params, "the graph draws the chosen properties");
    let mid = (model.min_y + model.max_y) * 0.5;
    [
        (model.top_label.to_string(), model.max_y),
        (model.mid_label.to_string(), mid),
        (model.bottom_label.to_string(), model.min_y),
    ]
}

#[test]
fn the_value_axis_prints_in_the_units_of_its_rows() {
    let mut checked = 0;
    for (mut state, clip) in fixtures() {
        let (model, audio_lane) = state.clip_and_lane_kind(clip).expect("the fixture clip");
        let rows = key_params_of(&model, audio_lane);
        for row in rows.iter() {
            if !matches!(
                row.spec.kind,
                ParamKind::Scalar | ParamKind::Vec2 | ParamKind::Vec3
            ) {
                continue;
            }
            let param = row.param();
            let printed = |stored: f32| {
                format_param_value(&model, &row.spec, Some(ParamValue::Scalar(stored)))
            };
            for (label, stored) in axis(&mut state, clip, &[param], GraphMode::Value) {
                assert_eq!(
                    label,
                    printed(stored).0.as_ref(),
                    "{}: the Value axis at {stored}",
                    row.label
                );
            }
            for (label, rate) in axis(&mut state, clip, &[param], GraphMode::Speed) {
                let number = printed(rate).2.expect("a plotted row prints a number");
                let mut expected = match number.scale {
                    Scale::Linear(_) => printed(rate).0.to_string(),
                    Scale::Gain => Units::SPEED.print(rate),
                };
                expected.push_str("/s");
                assert_eq!(label, expected, "{}: the Speed axis at {rate}", row.label);
            }
            checked += 1;
        }
    }
    // The fixtures dress the transform, an effect, a mask, a title and a
    // lane's audio, so far fewer rows means a fixture lost its properties.
    assert!(checked >= 20, "only {checked} rows were graphed");
}

/// Opacity prints a percent and Rotation degrees: one axis cannot print both,
/// so it prints the stored numbers. Position and Anchor Point both print a
/// percent to a tenth, so their shared axis prints it.
#[test]
fn curves_in_different_units_share_an_axis_of_stored_numbers() {
    let (mut state, clip) = blurred_clip();
    let mixed = [ClipParam::Opacity, ClipParam::Rotation];
    for (label, stored) in axis(&mut state, clip, &mixed, GraphMode::Value) {
        assert_eq!(label, format!("{stored:.1}"), "the mixed axis at {stored}");
    }
    for (label, rate) in axis(&mut state, clip, &mixed, GraphMode::Speed) {
        assert_eq!(label, format!("{rate:.1}/s"), "the mixed axis at {rate}");
    }
    let shared = [ClipParam::Position, ClipParam::AnchorPoint];
    let units = Units::of(ClipParam::Position, &ClipParam::Position.range());
    for (label, stored) in axis(&mut state, clip, &shared, GraphMode::Value) {
        assert_eq!(label, units.print(stored), "the shared axis at {stored}");
    }
}
