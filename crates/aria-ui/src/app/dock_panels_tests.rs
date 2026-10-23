//! Contracts for the History, Markers, Scopes, and Color dock panels.
//!
//! The class closed here: a panel that is registered and cannot be reached,
//! drawn, or acted on, and scopes measured for a panel nobody can see. What is
//! asserted:
//!
//! - Each panel opens from its Window row, draws its surface, and closes from
//!   the same row; a row whose panel has a binding draws that binding's chord.
//! - Ctrl+Shift+H and Shift+7 bring History and Scopes forward.
//! - The scopes measure while the Scopes tab is in front of its frame or the
//!   strip is open, and stop measuring and drop the reading once neither is.
//! - A click on a History row moves the project to that step.
//! - The Color panel draws for a selected clip, and Reset grade returns the
//!   grade to neutral.
//!
//! Not covered: where the Scopes panel's cards land as the frame narrows,
//! which needs pixels.

use std::path::PathBuf;
use std::sync::Arc;

use aria_core::RgbaImage;
use aria_models::{ClipId, ClipParam, LookParam};

use super::premiere_tests::with_clip;
use super::scope_view::{ScopeBuffers, ScopeTextures, measure};
use crate::chrome::menu_items::{items_of, run_item};
use crate::chrome::menu_window::PANEL_PREFIX;
use crate::dock::{PANEL_COLOR, PANEL_HISTORY, PANEL_MARKERS, PANEL_SCOPES, PanelId};
use crate::types::AppMenu;

/// Each panel, the surface it draws, and the binding drawn beside its Window
/// row.
const PANELS: &[(PanelId, &str, Option<&str>)] = &[
    (PANEL_HISTORY, "history-panel", Some("key-history")),
    (PANEL_MARKERS, "markers-panel", None),
    (PANEL_SCOPES, "scopes-panel", Some("key-scopes")),
    (PANEL_COLOR, "color-panel", None),
];

/// A frame of one colour, straight alpha.
fn frame(rgba: [u8; 4]) -> Arc<RgbaImage> {
    let pixels = rgba.iter().copied().cycle().take(16 * 16 * 4).collect();
    Arc::new(RgbaImage::new(16, 16, pixels))
}

/// A reading of `frame` for `tick`, as the worker makes one.
fn reading(frame: &RgbaImage, tick: i64) -> ScopeTextures {
    measure(
        frame,
        tick,
        aria_analysis::WaveformTrace::Luma,
        &mut ScopeBuffers::default(),
    )
    .expect("a well-formed frame measures")
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

#[gpui::test]
fn each_panel_docks_from_its_window_row_and_draws(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "dock-panels");
    for &(panel, drawn, key) in PANELS {
        let row = format!("{PANEL_PREFIX}{panel}");
        let (shown, wanted) = visual.read(|app| {
            let state = state.read(app);
            let shown = items_of(AppMenu::Window, state)
                .into_iter()
                .find(|item| item.id == row)
                .map(|item| item.chord(state));
            (shown, key.and_then(|key| state.chord_label(key)))
        });
        let shown = shown.unwrap_or_else(|| panic!("Window has no row for {panel}"));
        assert_eq!(shown, wanted, "the chord beside Window > {panel}");
        assert_eq!(
            shown.is_some(),
            key.is_some(),
            "Window > {panel} draws a chord it has no binding for, or none for its binding"
        );

        let press = |visual: &mut gpui::VisualTestContext| {
            let found = visual.update(|_, app| {
                state.update(app, |state, cx| run_item(AppMenu::Window, &row, state, cx))
            });
            assert!(found, "Window has no row for {panel}");
            visual.run_until_parked();
            visual.read(|app| state.read(app).dock_tree.is_open(panel))
        };
        if visual.read(|app| state.read(app).dock_tree.is_open(panel)) {
            assert!(!press(visual), "{panel} did not close");
        }
        assert!(press(visual), "{panel} did not open");
        assert!(
            visual.debug_bounds(drawn).is_some(),
            "{panel} is open and not drawn"
        );
        assert!(!press(visual), "{panel} did not close");
        assert!(
            visual.debug_bounds(drawn).is_none(),
            "{panel} is closed and still drawn"
        );
    }
}

/// Ctrl+Shift+H and Shift+7 bring their panels forward, opening each when
/// closed, and a second press leaves it in front.
#[test]
fn ctrl_shift_h_and_shift_7_bring_history_and_scopes_forward() {
    for (id, panel) in [("key-history", PANEL_HISTORY), ("key-scopes", PANEL_SCOPES)] {
        let (mut state, _) = with_clip();
        let binding = crate::keys::bindings::SHIPPED_BINDINGS
            .iter()
            .find(|binding| binding.id == id)
            .unwrap_or_else(|| panic!("{id} is bound"));
        let crate::keys::KeyBody::Plain(run) = binding.run else {
            panic!("{id} runs on the state alone");
        };
        if state.dock_tree.is_open(panel) {
            state.toggle_dock_panel(panel);
        }
        run(&mut state);
        assert!(
            state.dock_tree.is_active(panel),
            "{id} left {panel} closed or behind"
        );
        run(&mut state);
        assert!(
            state.dock_tree.is_active(panel),
            "a second {id} put {panel} away"
        );
    }
}

/// The Scopes tab in front measures; sent behind another tab of its frame, it
/// stops the worker measuring and drops the reading, textures included.
#[test]
fn the_scopes_tab_measures_only_while_it_is_in_front() {
    let (mut state, _) = with_clip();
    let picture = frame([10, 20, 30, 255]);
    state.last_frame = Some(Arc::clone(&picture));
    state.shown_tick = Some(4);
    if state.dock_tree.is_open(PANEL_SCOPES) {
        state.toggle_dock_panel(PANEL_SCOPES);
    }
    assert!(!state.scopes_wanted());
    assert!(
        !state.adopt_scopes(Some(reading(&picture, 4))),
        "a reading was taken with no scope on screen"
    );

    state.focus_dock_panel(PANEL_SCOPES);
    assert!(state.scopes_wanted());
    assert!(
        !state.preview_worker.set_scopes(true),
        "the Scopes tab in front left the worker not measuring"
    );
    let shown = reading(&picture, 4);
    let waveform = Arc::clone(&shown.waveform);
    assert!(state.adopt_scopes(Some(shown)));
    assert_eq!(state.scope_summary(), "Frame 00:00:00:04");

    let neighbour = state
        .dock_tree
        .find(PANEL_SCOPES)
        .and_then(|id| state.dock_tree.frame(id))
        .and_then(|frame| frame.tabs.iter().copied().find(|tab| *tab != PANEL_SCOPES))
        .expect("the Scopes tab shares its frame");
    state.focus_dock_panel(neighbour);
    assert!(!state.scopes_wanted());
    assert!(
        !state.preview_worker.set_scopes(false),
        "the worker kept measuring for a tab behind another"
    );
    assert!(
        state.scope_textures.is_none(),
        "a Scopes tab sent behind kept its reading"
    );
    assert!(
        state
            .retired_images
            .iter()
            .any(|image| Arc::ptr_eq(image, &waveform)),
        "the dropped reading's textures were not released"
    );
    assert_eq!(state.scope_summary(), "Scopes off");
}

/// The strip and the Scopes tab show one reading: hiding either keeps it while
/// the other is on screen, and hiding both drops it.
#[test]
fn the_strip_and_the_tab_share_one_reading() {
    let (mut state, _) = with_clip();
    let picture = frame([200, 100, 50, 255]);
    state.last_frame = Some(Arc::clone(&picture));
    state.shown_tick = Some(2);
    state.focus_dock_panel(PANEL_SCOPES);
    state.toggle_scopes();
    assert!(state.scopes_open);
    assert!(state.adopt_scopes(Some(reading(&picture, 2))));

    let neighbour = state
        .dock_tree
        .find(PANEL_SCOPES)
        .and_then(|id| state.dock_tree.frame(id))
        .and_then(|frame| frame.tabs.iter().copied().find(|tab| *tab != PANEL_SCOPES))
        .expect("the Scopes tab shares its frame");
    state.focus_dock_panel(neighbour);
    assert!(
        state.scope_textures.is_some(),
        "the open strip lost its reading when the tab went behind"
    );

    state.focus_dock_panel(PANEL_SCOPES);
    state.toggle_scopes();
    assert!(
        state.scope_textures.is_some(),
        "the Scopes tab lost its reading when the strip closed"
    );

    state.focus_dock_panel(neighbour);
    assert!(
        state.scope_textures.is_none(),
        "the reading outlived both scopes"
    );
}

/// The first clip of the open sequence.
fn first_clip(state: &super::AppState) -> ClipId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .next()
        .expect("one clip")
}

/// A click on an older History row undoes to it, and a click on the newer row
/// redoes to that, with the edit's effect following the depth.
#[gpui::test]
fn a_click_on_a_history_row_moves_the_project_to_that_step(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "history-panel");
    let before = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let before = state.history_depth();
            crate::surfaces::markers_pick::apply(
                state,
                crate::surfaces::markers_pick::Pick::AddAtPlayhead,
            )
            .expect("a marker at the playhead");
            state.focus_dock_panel(PANEL_HISTORY);
            cx.notify();
            before
        })
    });
    visual.run_until_parked();
    let read = |visual: &mut gpui::VisualTestContext| {
        visual.read(|app| {
            let state = state.read(app);
            (state.history_depth(), state.marker_count())
        })
    };
    assert_eq!(read(visual), (before + 1, 1));

    for (depth, markers) in [(before, 0), (before + 1, 1)] {
        let selector = crate::interned::intern(&format!("history-row-{depth}"));
        let row = visual
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("History draws no row for step {depth}"));
        visual.simulate_click(row.center(), gpui::Modifiers::none());
        visual.run_until_parked();
        assert_eq!(
            read(visual),
            (depth, markers),
            "a click on step {depth} left the project elsewhere"
        );
    }
}

/// With no clip selected the Color panel offers nothing to reset; with a
/// graded clip selected it draws Reset grade, and a click returns the grade to
/// neutral.
#[gpui::test]
fn the_color_panel_resets_the_selected_clips_grade(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "color-panel");
    let exposure = ClipParam::Look {
        param: LookParam::AdjustExposure,
    };
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_dock_panel(PANEL_COLOR);
            cx.notify();
        })
    });
    visual.run_until_parked();
    assert!(
        visual.debug_bounds("color-panel").is_some(),
        "the Color panel is in front and not drawn"
    );
    assert!(
        visual.debug_bounds("color-grade-clear").is_none(),
        "Reset grade is offered with no clip selected"
    );

    let neutral = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(fixture()).expect("import fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place clip");
            let clip = first_clip(state);
            state.select_clip(clip);
            let neutral = state.param_scalar(exposure, None);
            state.nudge_param(exposure, None, 0.5).expect("grade");
            cx.notify();
            neutral
        })
    });
    visual.run_until_parked();
    let graded = visual.read(|app| state.read(app).param_scalar(exposure, None));
    assert!(neutral.is_some(), "exposure is not a property of the clip");
    assert_ne!(graded, neutral, "the nudge did not grade the clip");

    let reset = visual
        .debug_bounds("color-grade-clear")
        .expect("Reset grade is not drawn for a selected clip");
    visual.simulate_click(reset.center(), gpui::Modifiers::none());
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).param_scalar(exposure, None)),
        neutral,
        "Reset grade left the grade on"
    );
}
