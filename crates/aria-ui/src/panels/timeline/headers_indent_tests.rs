//! The property outline under a lane starts on the lane header's text edge.
//!
//! The class closed here is an outline row indented by its own number rather
//! than by the header it hangs from: a group twirl that starts left or right of
//! the badge label above it, and property rows that do not step one spacing
//! unit in from their group. The edge is measured from the drawn badge label,
//! so a change to the header's padding, patch slot, or gap that the outline
//! does not follow goes red.
//!
//! Not caught: the Path row of a mask, which shares the property rows' edge
//! constant but is not drawn in this fixture, and the outline's right side.

use std::path::PathBuf;

use gpui::{Pixels, px};

use crate::theme::Theme;

const EPS: f32 = 0.51;

fn near(a: Pixels, b: Pixels) -> bool {
    (f32::from(a) - f32::from(b)).abs() <= EPS
}

#[gpui::test]
fn the_property_outline_starts_on_the_lane_header_text_edge(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let executor = cx.executor();
    let (state, visual) = crate::test_support::drawn_shell(cx, "property-indent");
    let still = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
    let track = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let media = state.import_media(still).expect("import the still");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place the still");
            let track = state.store.sequence.tracks[0].id;
            let clip = state.store.sequence.tracks[0].clips[0].id;
            state.toggle_clip_outline(clip);
            cx.notify();
            track
        })
    });
    // The rows twirl open over the lane beat: one frame starts the glide and
    // one past its end draws every row.
    for _ in 0..2 {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        executor.advance_clock(std::time::Duration::from_secs(1));
    }

    let badge = visual
        .debug_bounds(crate::interned::intern(&format!("track-badge-{track}")))
        .expect("the lane header draws its badge");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let label = runs
        .iter()
        .filter(|run| badge.contains(&run.bounds.center()))
        .map(|run| run.bounds.left())
        .reduce(|a, b| a.min(b))
        .expect("the badge draws its label");
    let arrow = visual
        .debug_bounds("prop-group-arrow")
        .expect("the outline draws a group twirl");
    let watch = visual
        .debug_bounds("prop-watch")
        .expect("the open Transform group draws a property row");
    assert!(
        near(arrow.left(), label),
        "the group twirl starts at {:?}, off the badge label's edge {label:?}",
        arrow.left()
    );
    assert!(
        near(watch.left(), label + px(Theme::SPACE_XS)),
        "a property row's stopwatch starts at {:?}, not one step in from the label's \
         edge {label:?}",
        watch.left()
    );
}
