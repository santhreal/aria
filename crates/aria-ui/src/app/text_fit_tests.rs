//! A line of text fits the box it is drawn in.
//!
//! The class closed here: a cell as wide as its text. A destination path wider
//! than the export card ran past the card's edge, and a frame holding more
//! tabs than its width pushed its frame menu out of the frame, where the
//! pointer does not reach it.
//!
//! The export sheet is drawn with a destination longer than the card, and
//! every value row on it draws through the one cell this proves. Every docked
//! frame is drawn with more tabs than fit, and each keeps its menu, the tabs
//! it draws, each drawn tab's close button, and the overflow control that
//! lists the rest inside it.
//!
//! Not covered: the glyphs, which the test text system does not raster, where
//! the ellipsis lands, and text outside the export sheet and the tab bar.

use std::path::PathBuf;

use gpui::{Bounds, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::{AppState, DialogKind};
use crate::dock::{FrameId, Zone};
use crate::test_support::{bounds_inside, drawn_shell};

#[gpui::test]
fn a_destination_longer_than_the_export_card_stays_inside_it(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "fit-export");
    let mut project = PathBuf::from("/");
    for _ in 0..12 {
        project.push("a-folder-name-longer-than-most");
    }
    project.push("project.aria");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.project_path = Some(project);
            state.open_dialog(DialogKind::Export);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the export sheet draws a card");
    let value = visual
        .debug_bounds("export-row-Destination")
        .expect("the export sheet draws its destination");
    assert!(
        bounds_inside(value, card),
        "the destination {value:?} runs past the card {card:?}"
    );
    assert!(
        value.size.width > px(0.),
        "the destination drew no width: {value:?}"
    );
}

#[gpui::test]
fn a_frame_holding_more_tabs_than_fit_keeps_its_menu_and_its_tabs(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "fit-tabs");
    let window = Bounds::new(
        point(px(0.), px(0.)),
        visual.update(|window, _| window.viewport_size()),
    );
    // Every panel in the first frame, then the last one split off to its right:
    // the first frame holds every tab but one in part of the window's width.
    let crowded = first_frame(&state, visual);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            let panels = state.registered_panel_ids();
            for &panel in &panels {
                state.dock_tree.dock(panel, crowded, Zone::Center);
            }
            let last = panels
                .last()
                .copied()
                .expect("the ui plugin registers panels");
            state.dock_tree.dock(last, crowded, Zone::Right);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let tabs = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frame(crowded)
            .map_or(0, |frame| frame.tabs.len())
    });
    assert!(tabs >= 12, "the crowded frame holds {tabs} tabs");

    let drawn = drawn_frames(&state, visual);
    assert!(
        drawn.len() >= 2,
        "the split drew {} frames: {drawn:?}",
        drawn.len()
    );
    let mut offenders = Vec::new();
    for (id, frame) in drawn {
        if !bounds_inside(frame, window) {
            offenders.push(format!(
                "{id:?}: the frame {frame:?} is not inside the window"
            ));
        }
        offenders.extend(misplaced(
            visual,
            format!("dock-frame-menu-{}", id.0),
            frame,
            &format!("{id:?}: the frame menu"),
        ));
        let panels = visual.read(|app| {
            state
                .read(app)
                .dock_tree
                .frame(id)
                .map(|held| held.tabs.clone())
                .unwrap_or_default()
        });
        // A tab the row has no room for is listed by the overflow control,
        // which `shell/tab_overflow_tests.rs` opens and reads.
        let mut hidden = false;
        for panel in panels {
            let tab = format!("{id:?}: the {panel} tab");
            let Some(bounds) = visual.debug_bounds(format!("dock-tab-{panel}").leak()) else {
                hidden = true;
                continue;
            };
            offenders.extend(misplaced(visual, format!("dock-tab-{panel}"), frame, &tab));
            offenders.extend(misplaced(
                visual,
                format!("dock-tab-close-{panel}"),
                bounds,
                &format!("{tab}'s close button"),
            ));
        }
        if hidden {
            offenders.extend(misplaced(
                visual,
                format!("dock-tab-more-button-{}", id.0),
                frame,
                &format!("{id:?}: the overflow control"),
            ));
        }
    }
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// What is wrong with the element drawn under `selector` against `outer`: not
/// drawn, or not inside it.
fn misplaced(
    visual: &mut VisualTestContext,
    selector: String,
    outer: Bounds<Pixels>,
    what: &str,
) -> Option<String> {
    match visual.debug_bounds(selector.leak()) {
        None => Some(format!("{what} was not drawn")),
        Some(bounds) if !bounds_inside(bounds, outer) => {
            Some(format!("{what} {bounds:?} is not inside {outer:?}"))
        }
        Some(_) => None,
    }
}

/// The docked frames the shell painted that hold a tab, by their painted
/// bounds.
fn drawn_frames(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
) -> Vec<(FrameId, Bounds<Pixels>)> {
    visual.read(|app| {
        let state = state.read(app);
        let rects = state.dock_frame_rects.lock();
        let mut drawn: Vec<(FrameId, Bounds<Pixels>)> = rects
            .iter()
            .filter(|(id, _)| {
                state
                    .dock_tree
                    .frame(**id)
                    .is_some_and(|frame| !frame.tabs.is_empty())
            })
            .map(|(id, &[x, y, w, h])| (*id, Bounds::new(point(px(x), px(y)), size(px(w), px(h)))))
            .collect();
        drawn.sort_by_key(|(id, _)| *id);
        drawn
    })
}

fn first_frame(state: &gpui::Entity<AppState>, visual: &mut VisualTestContext) -> FrameId {
    drawn_frames(state, visual)
        .first()
        .map(|(id, _)| *id)
        .expect("the shell draws a docked frame with a tab")
}
