//! Contracts for the layer-style and LUT surface.
//!
//! `SetClipLayerStyles` carries all four blocks, so the class closed here is
//! "adding or editing one style block drops the other three". The style list
//! comes from `app::clip_styles::STYLES` and the numeric rows from the property
//! catalog, both at run time, so a new style or a new style property fails until
//! it is covered.

use super::premiere_tests::with_clip;
use crate::app::clip_params::ParamRow;
use crate::app::clip_styles::{STYLES, style_enabled};

/// Every generated row of every style block, with the style it belongs to.
fn style_rows(state: &crate::app::AppState) -> Vec<(&'static str, ParamRow)> {
    STYLES
        .iter()
        .flat_map(|(style, _)| {
            state
                .param_rows(style)
                .into_iter()
                .map(move |row| (*style, row))
        })
        .collect()
}

#[test]
fn every_style_block_toggles_and_the_others_survive() {
    let (mut state, _) = with_clip();
    assert!(!STYLES.is_empty());

    for (id, _) in STYLES {
        state
            .toggle_selected_style(id)
            .unwrap_or_else(|err| panic!("enable {id}: {err:?}"));
        let styles = state.selected_ui_clip().expect("clip").styles;
        assert!(style_enabled(&styles, id), "{id} did not turn on");
    }

    // All four on at once: enabling the last must not have dropped the first.
    let styles = state.selected_ui_clip().expect("clip").styles;
    for (id, _) in STYLES {
        assert!(
            style_enabled(&styles, id),
            "{id} was dropped by a later style edit"
        );
    }

    // Toggling one off leaves the rest alone.
    state.toggle_selected_style("glow").expect("glow off");
    let styles = state.selected_ui_clip().expect("clip").styles;
    assert!(!style_enabled(&styles, "glow"));
    for (id, _) in STYLES.iter().filter(|(id, _)| *id != "glow") {
        assert!(style_enabled(&styles, id), "{id} went off with glow");
    }

    state.clear_selected_styles().expect("clear");
    let styles = state.selected_ui_clip().expect("clip").styles;
    for (id, _) in STYLES {
        assert!(!style_enabled(&styles, id), "{id} survived the clear");
    }
}

#[test]
fn every_style_row_moves_alone_and_clamps() {
    let (mut state, _) = with_clip();
    for (id, _) in STYLES {
        state.toggle_selected_style(id).expect("enable");
    }

    let rows = style_rows(&state);
    assert!(!rows.is_empty(), "four enabled blocks offered no rows");
    // A target every row's range contains, so one sweep covers them all.
    let targets: Vec<_> = rows
        .iter()
        .map(|(style, row)| (*style, row.clone(), row.range.clamp(3.0)))
        .collect();

    for (style, row, target) in &targets {
        state
            .set_param_scalar(row.param, row.component, *target)
            .unwrap_or_else(|err| panic!("set {style}/{}: {err:?}", row.id));
        let now = state.param_scalar(row.param, row.component).expect("read");
        assert!(
            (now - target).abs() < 1e-3,
            "{} did not take its value ({now} vs {target})",
            row.id
        );
    }

    for (_, row, target) in &targets {
        let now = state.param_scalar(row.param, row.component).expect("read");
        assert!(
            (now - target).abs() < 1e-3,
            "{} was reset by a later row edit",
            row.id
        );
    }

    for (_, row, _) in &targets {
        state
            .set_param_scalar(row.param, row.component, 1e6)
            .expect("high");
        let high = state.param_scalar(row.param, row.component).expect("read");
        if let Some(max) = row.range.max {
            assert!(high <= max + 1e-3, "{} exceeded {max}", row.id);
        }
        assert!(high.is_finite(), "{} took a non-finite value", row.id);

        state
            .set_param_scalar(row.param, row.component, -1e6)
            .expect("low");
        let low = state.param_scalar(row.param, row.component).expect("read");
        if let Some(min) = row.range.min {
            assert!(low >= min - 1e-3, "{} fell below {min}", row.id);
        }

        state
            .set_param_scalar(row.param, row.component, f32::NAN)
            .expect("nan");
        let after = state.param_scalar(row.param, row.component).expect("read");
        assert!(after.is_finite(), "{} took a NaN", row.id);
        assert!(
            (after - low).abs() < 1e-6,
            "{} moved on a NaN instead of ignoring it",
            row.id
        );
    }
}

#[test]
fn style_edits_need_their_block_and_an_unknown_id_is_inert() {
    let (mut state, _) = with_clip();

    // No block yet: no rows and no colors exist to edit.
    assert!(
        style_rows(&state).is_empty(),
        "a clip with no style blocks offered style rows"
    );
    for (id, _) in STYLES {
        assert!(
            state.param_colors(id).is_empty(),
            "a clip with no {id} block offered its color"
        );
        assert!(!style_enabled(
            &state.selected_ui_clip().expect("clip").styles,
            id
        ));
    }

    state.toggle_selected_style("shadow").expect("shadow on");
    let before = state.selected_ui_clip().expect("clip").styles;
    state.toggle_selected_style("no-such-style").expect("inert");
    assert!(
        state.param_rows("no-such-style").is_empty()
            && state.param_colors("no-such-style").is_empty(),
        "an unknown group produced rows"
    );
    assert_eq!(state.selected_ui_clip().expect("clip").styles, before);
}

#[test]
fn a_style_colour_edit_keeps_that_block_and_its_rows() {
    let (mut state, _) = with_clip();
    state.toggle_selected_style("outline").expect("outline on");
    let width = state
        .param_rows("outline")
        .into_iter()
        .find(|row| row.id == "style-outline-width")
        .expect("the outline block offers a width row");
    state
        .set_param_scalar(width.param, width.component, 6.0)
        .expect("width");
    let color = state
        .param_colors("outline")
        .into_iter()
        .find(|row| row.id == "style-outline-color")
        .expect("the outline block offers a color row");
    state
        .set_param_color(color.param, [10, 20, 30, 200])
        .expect("color");

    let styles = state.selected_ui_clip().expect("clip").styles;
    assert_eq!(styles.outline, Some([10, 20, 30, 200]));
    assert!(
        (styles.outline_width - 6.0).abs() < 1e-3,
        "a color edit must keep the width"
    );
}

#[test]
fn a_lut_loads_keeps_its_intensity_across_a_reload_and_clears() {
    use aria_models::{ClipParam, LookParam};
    let intensity = ClipParam::Look {
        param: LookParam::LutIntensity,
    };
    let (mut state, _) = with_clip();
    assert!(state.selected_lut().is_none());

    // An empty path is refused by the model, so it must never be sent.
    state.set_selected_lut("   ").expect("empty path");
    assert!(state.selected_lut().is_none());

    state.set_selected_lut("/luts/warm.cube").expect("load");
    let lut = state.selected_lut().expect("lut");
    assert_eq!(lut.path, "/luts/warm.cube");
    assert_eq!(lut.name(), "warm.cube");

    for _ in 0..40 {
        state.nudge_param(intensity, None, -0.1).expect("down");
    }
    assert_eq!(
        state.selected_lut().expect("lut").intensity,
        0.0,
        "intensity must stop at 0"
    );
    for _ in 0..40 {
        state.nudge_param(intensity, None, 0.1).expect("up");
    }
    assert_eq!(
        state.selected_lut().expect("lut").intensity,
        1.0,
        "intensity must stop at 1"
    );

    state.nudge_param(intensity, None, -0.5).expect("half");
    let dialed = state.selected_lut().expect("lut").intensity;
    state.set_selected_lut("/luts/cool.cube").expect("reload");
    let lut = state.selected_lut().expect("lut");
    assert_eq!(lut.path, "/luts/cool.cube");
    assert!(
        (lut.intensity - dialed).abs() < 1e-3,
        "loading another LUT must keep the dialed intensity"
    );

    state.clear_selected_lut().expect("clear");
    assert!(state.selected_lut().is_none());

    // With no LUT loaded, there is no row, so the nudge is inert.
    state.nudge_param(intensity, None, 0.5).expect("inert");
    assert!(state.selected_lut().is_none());
}

#[test]
fn style_and_lut_commands_are_inert_with_no_selection() {
    let (mut state, _) = with_clip();
    state.clear_selection();
    state.toggle_selected_style("shadow").expect("inert");
    state.clear_selected_styles().expect("inert");
    state.set_selected_lut("/luts/warm.cube").expect("inert");
    state.clear_selected_lut().expect("inert");

    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .next()
        .expect("clip still there");
    assert!(clip.styles.shadow.is_none());
    assert!(clip.lut.is_none());
}
