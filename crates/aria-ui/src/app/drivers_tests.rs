//! Property drivers from the inspector: Alt+click on a stopwatch, the source
//! box, the pick whip, and what a driven row shows.
//!
//! Closes the class "the inspector writes, shows, or edits a driver other than
//! the one the model holds": each gesture is one undo step, a driven row reads
//! the driver's result and its source, a failing driver reads the keyframed
//! value and its error, a refused write keeps the draft open, and the pick
//! whip inserts a reference that reads the picked property. Evaluation itself
//! is tested in aria-render and aria-plugin. Does not cover the Info panel.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipParam};
use gpui::{Modifiers, TestAppContext};

use crate::app::AppState;
use crate::app::clip_params::ParamRow;
use crate::app::premiere_tests::with_clip;
use crate::keys::fields;
use crate::test_support::drawn_shell;

const ROTATION: ClipParam = ClipParam::Rotation;

fn expressions(state: &AppState) {
    state
        .ctx
        .mount(&aria_plugin::ExpressionsPlugin)
        .expect("mount Expressions");
}

fn row(state: &AppState, param: ClipParam) -> ParamRow {
    let group = aria_models::param_spec(param)
        .expect("a catalog row")
        .group
        .to_string();
    state
        .param_rows(&group)
        .into_iter()
        .find(|row| row.param == param)
        .unwrap_or_else(|| panic!("the {group} group offers no row for {param:?}"))
}

fn retype(state: &mut AppState, text: &str) {
    while !state.driver_draft_text().is_empty() {
        state.driver_draft_pop();
    }
    if !text.is_empty() {
        assert!(fields::insert(state, text), "the driver box has focus");
    }
}

#[test]
fn alt_click_adds_a_driver_the_row_shows_and_a_second_removes_it() {
    let (mut state, clip) = with_clip();
    expressions(&state);
    let keyed = row(&state, ROTATION).value;

    state.toggle_driver(ROTATION).expect("add a driver");
    let draft = state.driver_draft().cloned().expect("the source opens");
    assert_eq!(
        (
            draft.clip,
            draft.param,
            draft.language.as_str(),
            draft.text.as_str(),
            draft.picking
        ),
        (clip, ROTATION, "aria.expressions", "value", false)
    );
    assert_eq!(
        fields::focused(&state).map(|field| field.id),
        Some("aria.field.driver")
    );
    let added = row(&state, ROTATION);
    assert_eq!(added.driver.as_deref(), Some("value"));
    assert_eq!(added.fault, None);
    assert_eq!(added.value, keyed, "the template leaves the value alone");

    retype(&mut state, "time * 30 + 5");
    state.commit_driver_draft().expect("write the driver");
    assert!(state.driver_draft().is_none());
    state.set_playhead(12);
    let seconds = 12.0 / state.playhead.rate.as_f64();
    let driven = row(&state, ROTATION);
    assert_eq!(driven.driver.as_deref(), Some("time * 30 + 5"));
    assert!(
        (f64::from(driven.value) - (seconds * 30.0 + 5.0)).abs() < 1e-3,
        "the row reads the driver's result: {}",
        driven.value
    );

    // The commit and the add are one undo step each.
    assert!(state.undo());
    assert_eq!(row(&state, ROTATION).driver.as_deref(), Some("value"));
    assert!(state.redo());

    state.toggle_driver(ROTATION).expect("remove the driver");
    let removed = row(&state, ROTATION);
    assert_eq!(removed.driver, None);
    assert_eq!(removed.value, keyed);
    assert!(state.undo());
    assert_eq!(
        row(&state, ROTATION).driver.as_deref(),
        Some("time * 30 + 5")
    );
}

#[test]
fn a_failing_driver_reads_the_keyframed_value_and_reports_why() {
    let (mut state, _) = with_clip();
    expressions(&state);
    let keyed = row(&state, ROTATION).value;
    state.toggle_driver(ROTATION).expect("add a driver");
    retype(&mut state, "nothing + 1");
    state
        .commit_driver_draft()
        .expect("code that fails is kept until it is fixed");
    let failed = row(&state, ROTATION);
    assert_eq!(failed.driver.as_deref(), Some("nothing + 1"));
    assert_eq!(failed.value, keyed);
    assert!(
        failed
            .fault
            .as_deref()
            .is_some_and(|fault| fault.contains("nothing is not defined")),
        "{:?}",
        failed.fault
    );

    state.open_driver_draft(ROTATION);
    retype(&mut state, "value + 1");
    state.commit_driver_draft().expect("fix the driver");
    let fixed = row(&state, ROTATION);
    assert_eq!(fixed.fault, None);
    assert_eq!(fixed.value, keyed + 1.0);
}

#[test]
fn a_cycle_is_refused_and_the_draft_stays_open() {
    let (mut state, _) = with_clip();
    expressions(&state);
    state.toggle_driver(ROTATION).expect("add a driver");
    retype(&mut state, "rotation + 1");
    let refused = state
        .commit_driver_draft()
        .expect_err("a driver reading its own property is a cycle");
    assert!(
        refused
            .to_string()
            .contains("this driver would read its own value back"),
        "{refused}"
    );
    assert_eq!(state.driver_draft_text(), "rotation + 1");
    assert_eq!(row(&state, ROTATION).driver.as_deref(), Some("value"));

    // Esc closes the draft without writing it.
    state.escape_driver_draft();
    assert!(state.driver_draft().is_none());
    assert_eq!(row(&state, ROTATION).driver.as_deref(), Some("value"));
}

#[test]
fn the_pick_whip_reads_a_property_of_another_clip() {
    let (mut state, first) = with_clip();
    expressions(&state);
    let media = {
        let engine = state.engine.lock();
        engine
            .project()
            .timeline()
            .clip(first)
            .and_then(aria_models::Clip::media)
            .expect("the fixture clip has media")
    };
    let end = state.store.sequence.duration_ticks;
    state
        .drop_media_on_timeline(media, None, end)
        .expect("a second clip");
    let second = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .find(|clip| *clip != first)
        .expect("two clips");
    state
        .apply_command(Command::Edit(EditCommand::SetClipName {
            clip: second,
            name: Some("Logo".into()),
        }))
        .expect("name the second clip");
    state.select_clip(second);
    state
        .set_param_scalar(ROTATION, None, 12.0)
        .expect("rotate Logo");

    state.select_clip(first);
    state.toggle_driver(ROTATION).expect("add a driver");
    retype(&mut state, "");
    state.toggle_driver_pick();
    assert!(state.driver_picking());

    state.select_clip(second);
    state
        .pick_driver_input(ROTATION)
        .expect("pick Logo's rotation");
    assert_eq!(
        state.selected_clip(),
        Some(first),
        "the driven clip is selected again"
    );
    assert!(!state.driver_picking());
    assert_eq!(
        state.driver_draft_text(),
        "thisComp.layer(\"Logo\").transform.rotation"
    );

    // With another clip selected the draft is not drawn, so keys do not type
    // into it; selecting its clip again brings it back.
    state.select_clip(second);
    assert!(!state.driver_draft_open(), "keys reach the editor");
    state.select_clip(first);
    assert!(state.driver_draft_open());
    state.commit_driver_draft().expect("write the driver");
    assert_eq!(row(&state, ROTATION).value, 12.0);
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// The drawn inspector: Alt+click on the rotation stopwatch adds a driver
/// and draws its source under the row, typed keys edit it, and Enter writes
/// it.
#[gpui::test]
async fn alt_click_on_a_drawn_stopwatch_adds_a_driver_the_keys_edit(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "driver-drawn");
    let clip = visual.update(|window, app| {
        // A state change made outside the shell notifies nothing, so the window
        // is asked to draw again or it keeps the frame with no clip selected.
        window.refresh();
        state.update(app, |state, _| {
            expressions(state);
            let media = state.import_media(fixture()).expect("import the fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the clip");
            let clip = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|track| track.clips.iter())
                .map(|clip| ClipId::from_raw(clip.id))
                .next()
                .expect("a clip");
            state.select_clip(clip);
            state.dock_tree.open(crate::dock::PANEL_INSPECTOR);
            clip
        })
    });
    visual.run_until_parked();
    let dot = visual.debug_bounds("insp-key-Rotation").unwrap_or_else(|| {
        let drawn: Vec<_> = ["insp-key-Position", "insp-key-Scale", "insp-key-Opacity"]
            .into_iter()
            .map(|selector| (selector, visual.debug_bounds(selector)))
            .collect();
        let facts = visual.read(|app| {
            let state = state.read(app);
            format!(
                "route {:?}, selected {:?}, tab {:?}, inspector collapsed {}, width {}, \
                 transform rows {:?}",
                state.route,
                state.selected_ui_clip().map(|clip| (clip.id, clip.kind)),
                state.inspector_tab,
                state.ui_settings.inspector_collapsed,
                state.ui_settings.inspector_width,
                state
                    .param_rows("transform")
                    .iter()
                    .map(|row| row.param)
                    .collect::<Vec<_>>(),
            )
        });
        panic!("the inspector draws no rotation stopwatch: {facts}; other dots {drawn:?}")
    });
    visual.simulate_click(dot.center(), Modifiers::alt());
    visual.run_until_parked();
    let driven = visual.read(|app| {
        state
            .read(app)
            .selected_model()
            .and_then(|model| model.driver(ROTATION).cloned())
    });
    assert_eq!(
        driven.map(|code| code.source),
        Some("value".to_string()),
        "Alt+click attaches the template"
    );
    assert!(
        visual.debug_bounds("insp-driver-Rotation").is_some(),
        "the source is drawn under the row"
    );

    visual.update(|_, app| {
        state.update(app, |state, _| {
            while !state.driver_draft_text().is_empty() {
                state.driver_draft_pop();
            }
        })
    });
    visual.simulate_input("time * 2");
    visual.simulate_keystrokes("enter");
    visual.run_until_parked();
    let written = visual.read(|app| {
        let state = state.read(app);
        (
            state.driver_draft().is_none(),
            state
                .selected_model()
                .filter(|model| model.id == clip)
                .and_then(|model| model.driver(ROTATION).cloned())
                .map(|code| code.source),
        )
    });
    assert_eq!(written, (true, Some("time * 2".to_string())));
}
