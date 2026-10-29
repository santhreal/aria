//! A draw of the timeline asks the filmstrip store for every cell it drew
//! without its exact frame, in every lane group.
//!
//! Closes the class "one lane group's ask takes back another's". An ask
//! replaces the one before it, and each lane group asked on its own, so the
//! audio group, drawn after the video group and with no strip in it, emptied
//! the lists the video group had just asked for. A landing then found nothing
//! queued and started no next cell: a strip filled one cell per frame, and a
//! cell drawn with no picture snapped only when it was the first one a draw
//! found. After a draw, the store holds every cell any group drew without its
//! exact frame, and every cell any group drew with no picture. Not covered: a
//! strip in the audio group, which draws none.

use super::lanes::built;
use crate::test_support::drawn_shell;

/// `ticks` in order, each once.
fn once_each(mut ticks: Vec<i64>) -> Vec<i64> {
    ticks.sort_unstable();
    ticks.dedup();
    ticks
}

#[gpui::test]
fn a_draw_asks_for_every_cell_its_lane_groups_drew_without_its_frame(
    cx: &mut gpui::TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "strip-asks");
    let still = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/texture.png");
    built::take_owed();
    // The update draws the window once it has settled and lands no cell: a
    // landing runs on the test's thread at its next drain.
    visual.update(|window, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            cx.notify();
        });
        window.refresh();
    });
    let (missing, blank) = built::take_owed();
    let (missing, blank) = (once_each(missing), once_each(blank));
    assert!(
        missing.len() > 1,
        "the draw drew {} strip cells without their frame: it measures no ask",
        missing.len()
    );
    assert_eq!(
        blank, missing,
        "the first draw of a strip drew a cell with a picture"
    );
    let (queued, snapping) = visual.read(|app| {
        let (queued, snapping) = state.read(app).filmstrips.asked();
        (queued.to_vec(), snapping.to_vec())
    });
    assert_eq!(
        queued, missing,
        "the store was not asked for every cell the draw drew without its frame"
    );
    assert_eq!(
        snapping, blank,
        "the store was not asked to snap every cell the draw drew with no picture"
    );
    visual.run_until_parked();
}
