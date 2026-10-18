//! The frame menu stays inside the window and stands on the menu bar's grid.
//!
//! The class closed here: a frame menu drawn past the window's edge, with the
//! panels it offers to add cut off below the bottom, and a menu whose rows
//! are another height than the menu bar's. Every drawn frame's menu is opened
//! with its Add Panel flyout out, at the launch window and at the smallest
//! window, and checked against the window, against the 22 px row pitch, and
//! against its own menu: the flyout opens beside the menu, level with the Add
//! Panel row, offers every registered panel the frame does not hold, and stays
//! out while the pointer moves down its rows. A panel picked from it joins the
//! frame in front.
//!
//! Not covered: the frost and the fade, and a floating window's frame menu.

use gpui::{Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px, size};

use crate::app::AppState;
use crate::dock::FrameId;
use crate::interned::intern;
use crate::test_support::{bounds_inside, drawn_shell};
use crate::theme::Theme;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

fn bounds(visual: &mut VisualTestContext, selector: &'static str, what: &str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("{what}: nothing draws {selector}"))
}

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= EPS
}

/// The docked frames and the panels each one does not hold, in the order the
/// menu lists them.
fn frames(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
) -> Vec<(FrameId, Vec<&'static str>)> {
    visual.read(|app| {
        let state = state.read(app);
        let mut drawn: Vec<FrameId> = state.dock_frame_rects.lock().keys().copied().collect();
        drawn.sort_by_key(|frame| frame.0);
        drawn
            .into_iter()
            .filter_map(|frame| {
                let tabs = state.dock_tree.frame(frame)?.tabs.clone();
                let openable = state
                    .registered_panel_ids()
                    .into_iter()
                    .filter(|panel| !tabs.contains(panel))
                    .collect();
                Some((frame, openable))
            })
            .collect()
    })
}

fn check_every_frame_menu(
    state: &gpui::Entity<AppState>,
    visual: &mut VisualTestContext,
    when: &str,
) {
    let window = visual.update(|window, _| window.viewport_size());
    let window = Bounds::new(point(px(0.0), px(0.0)), window);
    let frames = frames(state, visual);
    assert!(
        frames.len() > 1,
        "{when}: the shell draws {} frames",
        frames.len()
    );
    for (frame, openable) in frames {
        let what = format!("{when}, frame {}", frame.0);
        let button = bounds(
            visual,
            intern(&format!("dock-frame-menu-{}", frame.0)),
            &what,
        );
        visual.simulate_click(button.center(), Modifiers::none());
        visual.run_until_parked();
        let card = bounds(visual, "dock-frame-menu-card", &what);
        let add = bounds(visual, "dock-menu-add", &what);
        visual.simulate_mouse_move(add.center(), None, Modifiers::none());
        visual.run_until_parked();
        let flyout = bounds(visual, "dock-menu-add-flyout", &what);

        for (name, drawn) in [("menu", card), ("flyout", flyout)] {
            assert!(
                bounds_inside(drawn, window),
                "{what}: the {name} {drawn:?} runs past the window {window:?}"
            );
        }
        let overlap = f32::from(card.right()).min(f32::from(flyout.right()))
            - f32::from(card.left()).max(f32::from(flyout.left()));
        assert!(
            overlap <= Theme::SPACE_XS + 1.0 + EPS,
            "{what}: the flyout {flyout:?} covers the menu {card:?}"
        );
        assert!(
            near(add.size.height, px(Theme::ROW_HEIGHT)),
            "{what}: Add Panel is {:?} tall, not one menu row",
            add.size.height
        );
        // Level with Add Panel, or moved up to the window's bottom margin and
        // never down.
        let level = add.top() - px(Theme::SPACE_XS + 1.0);
        assert!(
            near(flyout.top(), level)
                || (flyout.top() < level
                    && near(flyout.bottom(), window.bottom() - px(Theme::SPACE_SM))),
            "{what}: the flyout {flyout:?} is neither level with Add Panel {add:?} nor on \
             the window's bottom margin"
        );
        let mut top = flyout.top() + px(Theme::SPACE_XS + 1.0);
        for panel in &openable {
            let row = bounds(visual, intern(&format!("dock-menu-open-{panel}")), &what);
            assert!(
                near(row.size.height, px(Theme::ROW_HEIGHT)) && near(row.top(), top),
                "{what}: Add {panel} is {row:?}, off the 22 px pitch from {top:?}"
            );
            assert!(
                bounds_inside(row, flyout),
                "{what}: Add {panel} {row:?} runs out of the flyout {flyout:?}"
            );
            top += px(Theme::ROW_HEIGHT);
        }
        assert!(
            near(top + px(Theme::SPACE_XS + 1.0), flyout.bottom()),
            "{what}: the flyout {flyout:?} holds more than the panels the frame lacks"
        );
        // The pointer crosses into the flyout and down its rows; the flyout
        // stays out under every one, so every one can be picked.
        for panel in &openable {
            let row = bounds(visual, intern(&format!("dock-menu-open-{panel}")), &what);
            visual.simulate_mouse_move(row.center(), None, Modifiers::none());
            visual.run_until_parked();
            assert!(
                visual.debug_bounds("dock-menu-add-flyout").is_some(),
                "{what}: the pointer on Add {panel} put the flyout away"
            );
        }

        visual.simulate_click(button.center(), Modifiers::none());
        visual.run_until_parked();
        assert!(
            visual.debug_bounds("dock-frame-menu-card").is_none(),
            "{what}: a second press on the frame's menu button left the menu open"
        );
    }
}

#[gpui::test]
fn every_frame_menu_stays_inside_the_window_on_the_menu_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "frame-menu");
    check_every_frame_menu(&state, visual, "launch");
    visual.simulate_resize(size(
        px(crate::settings::MIN_WINDOW_WIDTH),
        px(crate::settings::MIN_WINDOW_HEIGHT),
    ));
    visual.run_until_parked();
    check_every_frame_menu(&state, visual, "smallest window");
}

#[gpui::test]
fn a_panel_picked_from_the_add_panel_flyout_joins_the_frame_in_front(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "frame-menu-pick");
    let (frame, openable) = frames(&state, visual)
        .into_iter()
        .find(|(_, openable)| !openable.is_empty())
        .expect("a frame lacks a panel");
    let pick = *openable.last().expect("a panel to add");
    let button = bounds(
        visual,
        intern(&format!("dock-frame-menu-{}", frame.0)),
        "pick",
    );
    visual.simulate_click(button.center(), Modifiers::none());
    visual.run_until_parked();
    let add = bounds(visual, "dock-menu-add", "pick");
    visual.simulate_mouse_move(add.center(), None, Modifiers::none());
    visual.run_until_parked();
    // Along the Add Panel row into the flyout, then down to the last row.
    let first = bounds(
        visual,
        intern(&format!("dock-menu-open-{}", openable[0])),
        "pick",
    );
    visual.simulate_mouse_move(first.center(), None, Modifiers::none());
    visual.run_until_parked();
    let row = bounds(visual, intern(&format!("dock-menu-open-{pick}")), "pick");
    visual.simulate_mouse_move(row.center(), None, Modifiers::none());
    visual.run_until_parked();
    visual.simulate_click(row.center(), Modifiers::none());
    visual.run_until_parked();
    let held = visual.read(|app| {
        state
            .read(app)
            .dock_tree
            .frame(frame)
            .map(|f| (f.tabs.contains(&pick), f.tabs.get(f.active).copied()))
    });
    assert_eq!(
        held,
        Some((true, Some(pick))),
        "frame {}: picking Add {pick} left the frame without it in front",
        frame.0
    );
    assert!(
        visual.debug_bounds("dock-frame-menu-card").is_none(),
        "picking a panel left the frame menu open"
    );
}
