//! The Scopes panel scrolls through its readings, and fades its edges only
//! toward more of them.
//!
//! The class closed here: the panel laid its readings one under another in a
//! frame that scrolled with no cue, so at the launch layout the vectorscope
//! ended cut in half at the panel's foot, with nothing to show the panel ran
//! on. With a reading in the smallest window, where the histogram runs past
//! the panel's foot, the panel fades its foot and not its top. The wheel
//! turned to the end moves the fade to the top.
//!
//! Not covered: the fade's pixels, which the test platform does not raster on
//! Linux.

use aria_analysis::WaveformTrace;
use aria_core::RgbaImage;
use gpui::{
    Bounds, EdgeFade, Modifiers, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext, TouchPhase,
    VisualTestContext, point, px, size,
};

use crate::app::scope_view::{ScopeBuffers, measure};
use crate::dock::PANEL_SCOPES;
use crate::edge_fade::SCROLL_FADE;
use crate::settings::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH};
use crate::test_support::drawn_shell_at;

/// Layout is in floats; an edge two boxes share lands within this.
const EPS: f32 = 0.51;

/// The fades of one full draw, after the follow-ups of the last change.
fn draw(visual: &mut VisualTestContext) -> Vec<EdgeFade> {
    visual.run_until_parked();
    crate::edge_fade::take_painted();
    visual.update(|window, _| window.refresh());
    crate::edge_fade::take_painted()
}

fn bounds(visual: &mut VisualTestContext, selector: &'static str) -> Bounds<Pixels> {
    visual
        .debug_bounds(selector)
        .unwrap_or_else(|| panic!("the shell drew no {selector}"))
}

/// The top and bottom bands of the fade painted over `panel`, or zeros when
/// none is.
fn panel_bands(fades: &[EdgeFade], panel: Bounds<Pixels>) -> (f32, f32) {
    fades
        .iter()
        .find(|fade| fade.bounds == panel)
        .map_or((0.0, 0.0), |fade| {
            (f32::from(fade.bands.top), f32::from(fade.bands.bottom))
        })
}

#[gpui::test]
fn the_scopes_panel_fades_its_foot_while_its_readings_continue_below(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let at = Bounds::new(
        point(px(0.), px(0.)),
        size(px(MIN_WINDOW_WIDTH), px(MIN_WINDOW_HEIGHT)),
    );
    let (state, mut visual) = drawn_shell_at(cx, "scopes-scroll", at);
    let visual = &mut visual;
    // The readings fade up; reduced motion lands them at once.
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_dock_panel(PANEL_SCOPES);
            let frame = RgbaImage::new(16, 16, vec![128; 16 * 16 * 4]);
            state.scope_textures = Some(
                measure(&frame, 0, WaveformTrace::Luma, &mut ScopeBuffers::default())
                    .expect("a well-formed frame measures"),
            );
            cx.notify();
        })
    });

    let fades = draw(visual);
    let panel = bounds(visual, "scopes-panel");
    let last = bounds(visual, "scopes-panel-histogram-card");
    assert!(
        f32::from(last.bottom()) > f32::from(panel.bottom()) + EPS,
        "the histogram {last:?} fits in the Scopes panel {panel:?} at {MIN_WINDOW_WIDTH} x \
         {MIN_WINDOW_HEIGHT}, so this window cannot show the readings continuing"
    );
    assert_eq!(
        panel_bands(&fades, panel),
        (0.0, SCROLL_FADE),
        "at the top of readings that continue below, the Scopes panel fades its foot only"
    );

    let position = panel.center();
    visual.simulate_mouse_move(position, None, Modifiers::none());
    visual.simulate_event(ScrollWheelEvent {
        position,
        delta: ScrollDelta::Lines(point(0.0, -400.0)),
        modifiers: Modifiers::none(),
        touch_phase: TouchPhase::Moved,
    });
    let fades = draw(visual);
    assert_eq!(
        panel_bands(&fades, panel),
        (SCROLL_FADE, 0.0),
        "at the end of readings that continue above, the Scopes panel fades its top only"
    );
}
