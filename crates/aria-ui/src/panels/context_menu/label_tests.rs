//! The clear swatch of a label palette draws one cross, centered in its box and
//! inside its outline, in every menu that draws the palette.
//!
//! The class closed here: a glyph in a square control laid out at the start of
//! the control's line box instead of on its center, as the clear swatch's `×`
//! text was, which sat low and to the left of its outline at a tone that read as
//! a stray mark rather than a control. The timeline `grid_tests::dress` builds
//! is opened in a drawn shell and every target it offers a menu on is opened;
//! each menu that draws the palette must draw its clear swatch, and the swatch
//! must hold one icon whose center is the swatch's and whose box is inside the
//! 1 px outline. Both palettes, the clip menu's and the pool menu's, must be
//! measured.
//!
//! Not covered: the cross's tone, which neither a frame's text runs nor its
//! recorded boxes hold.

use gpui::{Bounds, Pixels, TestAppContext, px};

use super::grid_tests::dress;
use crate::components::drawn::{self, Drawn};
use crate::test_support::drawn_shell;
use crate::types::ContextTarget;

const EPS: f32 = 0.51;

/// The outline the clear swatch draws around its cross.
const OUTLINE: f32 = 1.0;

/// The debug selector of the clear swatch the menu on `target` draws, or
/// `None` for a menu without a label palette.
fn clear_swatch(target: &ContextTarget) -> Option<&'static str> {
    match target {
        ContextTarget::Clip { .. } => Some("ctx-label-clip-clear"),
        ContextTarget::PoolItem { .. } => Some("ctx-label-media-clear"),
        _ => None,
    }
}

#[gpui::test]
fn the_clear_swatch_draws_one_cross_centered_inside_its_outline(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "ctx-label-clear");
    visual.update(|_, app| app.set_reduce_motion(true));
    let targets = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let targets = dress(state);
            cx.notify();
            targets
        })
    });
    visual.run_until_parked();
    let mut faults = Vec::new();
    let mut measured = Vec::new();
    for target in targets {
        let Some(selector) = clear_swatch(&target) else {
            continue;
        };
        visual.update(|_, app| {
            state.update(app, |state, cx| {
                state.close_context();
                state.open_context(target.clone(), 240.0, 40.0);
                cx.notify();
            })
        });
        visual.run_until_parked();
        drawn::start();
        visual.update(|_, app| state.update(app, |_, cx| cx.notify()));
        visual.run_until_parked();
        let images = drawn::take();
        let Some(swatch) = visual.debug_bounds(selector) else {
            faults.push(format!("the menu on {target:?} drew no clear swatch"));
            continue;
        };
        measured.push(selector);
        let crosses: Vec<Bounds<Pixels>> = images
            .iter()
            .filter(|(kind, at)| *kind == Drawn::Image && swatch.contains(&at.center()))
            .map(|(_, at)| *at)
            .collect();
        let [cross] = crosses.as_slice() else {
            faults.push(format!(
                "{target:?}: the clear swatch drew {} icons, not one",
                crosses.len()
            ));
            continue;
        };
        let (center, middle) = (cross.center(), swatch.center());
        if (center.x - middle.x).abs() > px(EPS) || (center.y - middle.y).abs() > px(EPS) {
            faults.push(format!(
                "{target:?}: the cross is centered at {center:?}, the swatch at {middle:?}"
            ));
        }
        let inset = px(OUTLINE - EPS);
        if cross.left() < swatch.left() + inset
            || cross.top() < swatch.top() + inset
            || cross.right() > swatch.right() - inset
            || cross.bottom() > swatch.bottom() - inset
        {
            faults.push(format!(
                "{target:?}: the cross spans {cross:?}, past the outline of {swatch:?}"
            ));
        }
    }
    assert!(faults.is_empty(), "{}", faults.join("\n"));
    assert_eq!(
        measured,
        ["ctx-label-clip-clear", "ctx-label-media-clear"],
        "the clip and pool menus each draw a palette, so both clear swatches were measured"
    );
}
