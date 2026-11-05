//! The rail's selected row holds its plate under the pointer.
//!
//! WHY: the rail blended its selected plate toward the hover fill, which is
//! weaker, so the pane in view dimmed under the pointer and read like any
//! other row. Closes, for every pane of [`HomeRail::ALL`] selected in turn and
//! every pane row hovered: a selected row that changes fill under the pointer,
//! and a row that is not selected that draws no hover fill. Not covered: the
//! New and Open rows, which are never selected and share the same fill.

use gpui::{
    Background, Bounds, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px, size,
};

use crate::test_support::drawn_shell_at;
use crate::theme::Theme;
use crate::types::HomeRail;

/// The fill painted over exactly the bounds of `selector`, if any.
fn fill_of(visual: &mut VisualTestContext, selector: &'static str) -> Option<Background> {
    let row = visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("Home draws {selector}"));
    visual.update(|window, _| {
        let scale = window.scale_factor();
        window
            .painted_quads()
            .iter()
            .find(|quad| {
                let b = quad.bounds;
                let bounds: Bounds<Pixels> = Bounds::new(
                    point(px(b.origin.x.0 / scale), px(b.origin.y.0 / scale)),
                    size(px(b.size.width.0 / scale), px(b.size.height.0 / scale)),
                );
                (bounds.origin.x - row.origin.x).abs() < px(0.5)
                    && (bounds.origin.y - row.origin.y).abs() < px(0.5)
                    && (bounds.size.width - row.size.width).abs() < px(0.5)
                    && (bounds.size.height - row.size.height).abs() < px(0.5)
            })
            .map(|quad| quad.background)
    })
}

/// A new pane stops this match compiling until it joins the test.
fn selector(tab: HomeRail) -> &'static str {
    match tab {
        HomeRail::Recents => "home-rail-Recents",
        HomeRail::Samples => "home-rail-Samples",
        HomeRail::Learn => "home-rail-Learn",
    }
}

#[gpui::test]
fn the_selected_rail_row_holds_its_plate_under_the_pointer(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(point(px(0.), px(0.)), size(px(1280.), px(800.)));
    let (state, mut visual) = drawn_shell_at(cx, "home-rail-fill", at);
    let visual = &mut visual;
    visual.update(|_, app| app.set_reduce_motion(true));
    let (plate, hover) = visual.read(|app| {
        let theme = Theme::of(app);
        (theme.glass_selected_bg(), theme.element_hover)
    });
    for selected in HomeRail::ALL {
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.go_home();
                state.set_home_rail(selected);
                cx.notify();
            })
        });
        visual.run_until_parked();
        for hovered in HomeRail::ALL {
            let row = visual
                .debug_bounds(selector(hovered))
                .unwrap_or_else(|| panic!("Home draws the {} row", hovered.label()));
            visual.simulate_mouse_move(row.center(), None, Modifiers::none());
            visual.run_until_parked();
            let want = if hovered == selected { plate } else { hover };
            assert_eq!(
                fill_of(visual, selector(hovered)),
                Some(Background::from(want)),
                "with {} selected, the hovered {} row draws {want:?}",
                selected.label(),
                hovered.label()
            );
        }
    }
}
