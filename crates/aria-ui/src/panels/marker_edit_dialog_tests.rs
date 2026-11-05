//! The Edit Marker dialog draws its colors as one line of swatches.
//!
//! The class closed here: color picks drawn as named boxes that wrapped at the
//! dialog's width, leaving the last color alone on a second line under the
//! rest. The row is measured with the dialog open over the real shell: it is
//! one swatch tall, so every color stands on the first line.
//!
//! Not covered: which swatch is ringed and the tip each one shows, which the
//! test platform does not raster.

use gpui::TestAppContext;

use crate::swatch::SIDE;
use crate::test_support::drawn_shell;

#[gpui::test]
fn the_marker_colors_stand_on_one_line(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "marker-edit-colors");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.add_marker_at_playhead().expect("a marker lands");
            let marker = state.store.sequence.markers[0].id;
            assert!(state.open_marker_edit(marker), "the marker's dialog opens");
            cx.notify();
        })
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());

    let row = visual
        .debug_bounds("marker-edit-colors")
        .expect("the Edit Marker dialog drew its colors");
    let height = f32::from(row.size.height);
    assert!(
        (SIDE..2.0 * SIDE).contains(&height),
        "the color row is {height} tall, not one swatch line of {SIDE}"
    );
}
