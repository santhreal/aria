//! A [`super::picker`]'s open press, row press, outside press, and single-open
//! rule across the mixer, the Viewer, and the Project panel.
//!
//! WHY: the Viewer's zoom and resolution menus and the Project panel's sort
//! menu each held their own boolean or enum on `AppState`, and the mixer's
//! automation modes were drawn as five chips that wrapped onto two lines in a
//! strip's head. A list opened in one panel could stay open alongside a list in
//! another, and a change to outside-click or row-close behavior had to be made
//! in each copy. The classes closed here:
//!
//! - a press on a mixer strip's automation chip that does not open its five
//!   `AutomationMode::ALL` rows below the chip, a press on a row that does not
//!   set that track's or the master's automation mode in the model and close
//!   the list, or a press outside the chip and list that does not close the
//!   list with the mode unchanged;
//! - opening any `ListId` variant (`Viewer(Zoom)`, `Viewer(Resolution)`,
//!   `PoolSort`, `MixerAuto`) while another list is open without closing the
//!   earlier list.
//!
//! Not covered: the warning color on a writing automation mode's chip label,
//! which the test renderer does not read back from text runs.

use aria_models::{AutomationMode, TrackKind};
use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext};

use super::{ListId, row_selector};
use crate::app::AppState;
use crate::app::preview_view::ViewerMenu;
use crate::interned::intern;
use crate::motion::MotionKey;
use crate::resolution::PlaybackResolution;
use crate::test_support::drawn_shell;
use crate::types::MediaSort;

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the window draws no {selector}"))
}

fn click(visual: &mut VisualTestContext, selector: &str) {
    let at = bounds(visual, selector).center();
    visual.simulate_click(at, Modifiers::none());
    visual.run_until_parked();
}

fn auto_row(raw: u64, mode: AutomationMode) -> String {
    row_selector(
        MotionKey::new("mixer-auto", raw).with(mode.label()),
        mode.label(),
    )
}

fn strip_mode(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    raw: u64,
) -> AutomationMode {
    visual.read(|app| {
        let state = state.read(app);
        if raw == 0 {
            return state.master_chain().automation;
        }
        state
            .mixer_strips()
            .expect("the mix reads")
            .into_iter()
            .find(|strip| strip.track == raw)
            .unwrap_or_else(|| panic!("no mixer strip {raw}"))
            .automation
    })
}

fn open_list(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> Option<ListId> {
    visual.read(|app| state.read(app).open_list)
}

#[gpui::test]
fn the_mixer_automation_list_opens_chooses_every_mode_and_closes_on_an_outside_press(
    cx: &mut TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "pick-list-mixer-auto");
    visual.update(|_, app| app.set_reduce_motion(true));
    let lane = visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.apply_workspace("win-workspace-audio", cx));
            state
                .add_lane_of_kind(TrackKind::Audio, None)
                .expect("an audio lane")
                .raw()
        })
    });
    visual.run_until_parked();

    for raw in [lane, 0] {
        let chip_sel = format!("mixer-auto-{raw}");
        assert_eq!(open_list(&state, visual), None);
        for mode in AutomationMode::ALL {
            assert!(
                visual.debug_bounds(intern(&auto_row(raw, mode))).is_none(),
                "strip {raw} draws {mode:?} while its list is closed"
            );
        }

        for mode in [
            AutomationMode::Read,
            AutomationMode::Write,
            AutomationMode::Touch,
            AutomationMode::Latch,
            AutomationMode::Off,
        ] {
            click(visual, &chip_sel);
            assert_eq!(
                open_list(&state, visual),
                Some(ListId::MixerAuto(raw)),
                "pressing {chip_sel} opens its automation list"
            );
            let chip = bounds(visual, &chip_sel);
            for listed in AutomationMode::ALL {
                let row = bounds(visual, &auto_row(raw, listed));
                assert!(
                    row.top() >= chip.bottom(),
                    "strip {raw}'s {listed:?} row {row:?} does not open below its chip {chip:?}"
                );
            }
            click(visual, &auto_row(raw, mode));
            assert_eq!(
                open_list(&state, visual),
                None,
                "pressing strip {raw}'s {mode:?} row closes the list"
            );
            assert_eq!(
                strip_mode(&state, visual, raw),
                mode,
                "pressing strip {raw}'s {mode:?} row sets the automation mode in the model"
            );
            for listed in AutomationMode::ALL {
                assert!(
                    visual
                        .debug_bounds(intern(&auto_row(raw, listed)))
                        .is_none(),
                    "strip {raw} still draws {listed:?} after its row closed the list"
                );
            }
        }

        // Set a non-default mode, reopen the list, and press outside to close
        // without changing the mode.
        click(visual, &chip_sel);
        click(visual, &auto_row(raw, AutomationMode::Latch));
        assert_eq!(strip_mode(&state, visual, raw), AutomationMode::Latch);

        click(visual, &chip_sel);
        assert_eq!(open_list(&state, visual), Some(ListId::MixerAuto(raw)));
        click(visual, &format!("mixer-foot-{raw}"));
        assert_eq!(
            open_list(&state, visual),
            None,
            "an outside press closes strip {raw}'s automation list"
        );
        assert_eq!(
            strip_mode(&state, visual, raw),
            AutomationMode::Latch,
            "an outside press leaves strip {raw}'s automation mode unchanged"
        );

        // A second press on the chip itself toggles the open list closed.
        click(visual, &chip_sel);
        assert_eq!(open_list(&state, visual), Some(ListId::MixerAuto(raw)));
        click(visual, &chip_sel);
        assert_eq!(
            open_list(&state, visual),
            None,
            "a second press on strip {raw}'s chip closes its automation list"
        );
    }
}

#[gpui::test]
fn opening_any_pick_list_closes_the_one_that_was_open(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "pick-list-single-open");
    visual.update(|_, app| app.set_reduce_motion(true));
    let lane = visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.apply_workspace("win-workspace-audio", cx));
            state
                .add_lane_of_kind(TrackKind::Audio, None)
                .expect("an audio lane")
                .raw()
        })
    });
    visual.run_until_parked();

    let lane_row = auto_row(lane, AutomationMode::Read);
    let master_row = auto_row(0, AutomationMode::Read);
    let zoom_row = row_selector(("preview-zoom-level", 0usize), "Fit");
    let res_row = row_selector(
        ("preview-resolution-playback", 0usize),
        PlaybackResolution::ALL[0].label(),
    );
    let sort_row = row_selector(("pool-sort-row", 0usize), MediaSort::ALL[0].label());

    click(visual, &format!("mixer-auto-{lane}"));
    assert_eq!(open_list(&state, visual), Some(ListId::MixerAuto(lane)));
    assert!(visual.debug_bounds(intern(&lane_row)).is_some());

    // Opening the master strip's automation list closes the lane strip's list.
    click(visual, "mixer-auto-0");
    assert_eq!(open_list(&state, visual), Some(ListId::MixerAuto(0)));
    assert!(
        visual.debug_bounds(intern(&lane_row)).is_none(),
        "opening mixer-auto-0 closes mixer-auto-{lane}"
    );
    assert!(visual.debug_bounds(intern(&master_row)).is_some());

    // Toggling another list on AppState while a list is open closes the open
    // list and opens the new one in one step.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.toggle_list(ListId::MixerAuto(lane));
            cx.notify();
        });
    });
    visual.run_until_parked();
    assert_eq!(open_list(&state, visual), Some(ListId::MixerAuto(lane)));
    assert!(
        visual.debug_bounds(intern(&master_row)).is_none(),
        "toggle_list(mixer-auto-{lane}) closes mixer-auto-0"
    );
    assert!(visual.debug_bounds(intern(&lane_row)).is_some());

    click(visual, "mixer-auto-0");
    assert_eq!(open_list(&state, visual), Some(ListId::MixerAuto(0)));
    // Opening the Viewer zoom list while a mixer list is open closes the mixer
    // list.
    click(visual, "preview-zoom");
    assert_eq!(
        open_list(&state, visual),
        Some(ListId::Viewer(ViewerMenu::Zoom))
    );
    assert!(
        visual.debug_bounds(intern(&master_row)).is_none(),
        "opening preview-zoom closes mixer-auto-0"
    );
    assert!(visual.debug_bounds(intern(&zoom_row)).is_some());

    // Opening the Viewer resolution list closes the zoom list.
    click(visual, "preview-resolution");
    assert_eq!(
        open_list(&state, visual),
        Some(ListId::Viewer(ViewerMenu::Resolution))
    );
    assert!(
        visual.debug_bounds(intern(&zoom_row)).is_none(),
        "opening preview-resolution closes preview-zoom"
    );
    assert!(visual.debug_bounds(intern(&res_row)).is_some());

    // Opening the Viewer zoom list while the resolution list is open closes
    // the resolution list.
    click(visual, "preview-zoom");
    assert_eq!(
        open_list(&state, visual),
        Some(ListId::Viewer(ViewerMenu::Zoom))
    );
    assert!(
        visual.debug_bounds(intern(&res_row)).is_none(),
        "opening preview-zoom closes preview-resolution"
    );
    assert!(visual.debug_bounds(intern(&zoom_row)).is_some());

    click(visual, "preview-resolution");
    assert_eq!(
        open_list(&state, visual),
        Some(ListId::Viewer(ViewerMenu::Resolution))
    );

    // In the Editing workspace, opening the Project pool sort list closes the
    // Viewer resolution list, and pressing each sort row sets media_sort and
    // closes the list.
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            assert!(state.apply_workspace("win-workspace-editing", cx));
        })
    });
    visual.run_until_parked();
    click(visual, "pool-sort");
    assert_eq!(open_list(&state, visual), Some(ListId::PoolSort));
    assert!(
        visual.debug_bounds(intern(&res_row)).is_none(),
        "opening pool-sort closes preview-resolution"
    );
    assert!(visual.debug_bounds(intern(&sort_row)).is_some());

    for (index, order) in MediaSort::ALL.iter().copied().enumerate() {
        if open_list(&state, visual) != Some(ListId::PoolSort) {
            click(visual, "pool-sort");
        }
        let sel = row_selector(("pool-sort-row", index), order.label());
        click(visual, &sel);
        assert_eq!(
            visual.read(|app| {
                let state = state.read(app);
                (state.media_sort, state.open_list)
            }),
            (order, None),
            "pressing pool sort row {} sets {:?} and closes the list",
            order.label(),
            order
        );
    }
}
