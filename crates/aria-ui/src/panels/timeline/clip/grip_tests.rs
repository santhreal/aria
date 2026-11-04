//! A clip bar draws its fade handles while the pointer is on the bar, and the
//! handle of a fade that is set at all times.
//!
//! Closes the class "every bar draws two handle plates at rest": a handle
//! drawn on a bar the pointer is not on, a handle left drawn after the
//! pointer moves onto another bar under the same cursor or off the lanes, and
//! a set fade whose handle hides with the pointer away. Not covered: a locked
//! bar or one under 28 px, which draws no handle with the pointer on it
//! either, and the pointer leaving the window, which the test platform does
//! not send.

use aria_models::{ClipId, MediaId};
use gpui::{Bounds, Entity, Modifiers, Pixels, TestAppContext, VisualTestContext, point, px};

use super::super::{FADE_GRIP, TRIM};
use super::tests::{painted, placed, texture, v1_lane};
use crate::app::AppState;
use crate::transport::TIMELINE_BASE_PPS;

/// The left edge of every fade handle drawn on `lane`, left to right.
fn handles(visual: &mut VisualTestContext, lane: Bounds<Pixels>) -> Vec<f32> {
    let mut lefts: Vec<f32> = painted(visual, lane)
        .into_iter()
        .filter(|quad| {
            (f32::from(quad.bounds.size.width) - FADE_GRIP).abs() < 0.01
                && (f32::from(quad.bounds.size.height) - FADE_GRIP).abs() < 0.01
        })
        .map(|quad| f32::from(quad.bounds.left()))
        .collect();
    lefts.sort_by(f32::total_cmp);
    lefts
}

/// The window x span of `clip`'s bar, and the width of one tick.
pub(super) fn card(
    visual: &mut VisualTestContext,
    state: &Entity<AppState>,
    clip: ClipId,
) -> (f32, f32, f32) {
    let canvas = visual
        .debug_bounds("timeline-video-canvas")
        .expect("the timeline draws its video lanes");
    visual.read(|app| {
        let state = state.read(app);
        let bar = state.ui_clip_ref(clip).expect("the clip is projected");
        let tick_px = TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second();
        let left = f32::from(canvas.origin.x) - state.scroll_x;
        (
            left + bar.timeline_start_ticks as f32 * tick_px,
            left + bar.timeline_end_ticks as f32 * tick_px,
            tick_px,
        )
    })
}

fn hover(visual: &mut VisualTestContext, x: f32, y: Pixels) {
    visual.simulate_mouse_move(point(px(x), y), None, Modifiers::none());
    visual.run_until_parked();
}

/// Asserts the handles drawn on `lane` stand at `want`, each within the
/// half pixel a snapped quad moves.
fn assert_handles(visual: &mut VisualTestContext, lane: Bounds<Pixels>, want: &[f32], when: &str) {
    let drawn = handles(visual, lane);
    assert!(
        drawn.len() == want.len() && drawn.iter().zip(want).all(|(a, b)| (a - b).abs() <= 0.5),
        "{when}: handles drawn at {drawn:?}, want {want:?}"
    );
}

#[gpui::test]
fn a_bar_shows_its_fade_handles_for_the_pointer_and_for_a_set_fade(cx: &mut TestAppContext) {
    let (state, visual, first) = placed(cx, "fade-handles", texture());
    let second = visual.update(|window, app| {
        let second = state.update(app, |s, _| {
            let bar = s.ui_clip_ref(first).expect("the placed clip");
            let media = bar.media_id.expect("a clip of the fixture");
            let cut = bar.timeline_end_ticks;
            s.drop_media_on_timeline(MediaId::from_raw(media), None, cut)
                .expect("place the fixture after the first");
            s.store
                .sequence
                .tracks
                .iter()
                .flat_map(|lane| lane.clips.iter())
                .find(|clip| clip.timeline_start_ticks == cut)
                .map(|clip| ClipId::from_raw(clip.id))
                .expect("the second clip starts at the first one's end")
        });
        window.refresh();
        second
    });
    visual.run_until_parked();
    let lane = v1_lane(visual);
    let canvas = visual
        .debug_bounds("timeline-video-canvas")
        .expect("the timeline draws its video lanes");
    let y = lane.center().y;
    let (a0, a1, tick_px) = card(visual, &state, first);
    let (b0, b1, _) = card(visual, &state, second);
    assert!(
        a0 >= f32::from(canvas.left()) && b1 <= f32::from(canvas.right()) && a1 - a0 >= 28.0,
        "both bars are on screen and wide enough for handles: {a0}..{a1}, {b0}..{b1} in \
         {canvas:?}"
    );
    let ends = |left: f32, right: f32| [left + TRIM, right - TRIM - FADE_GRIP];

    assert_handles(visual, lane, &[], "at rest");
    hover(visual, (a0 + a1) / 2.0, y);
    assert_handles(visual, lane, &ends(a0, a1), "the pointer on the first bar");
    hover(visual, (b0 + b1) / 2.0, y);
    assert_handles(visual, lane, &ends(b0, b1), "the pointer on the second bar");
    // Over the Viewer's picture, above its control row: nothing there redraws
    // the Timeline on its own.
    let viewer = visual
        .debug_bounds("viewer-row")
        .expect("the Viewer draws its row");
    hover(
        visual,
        f32::from(viewer.center().x),
        viewer.top() - px(60.0),
    );
    assert_handles(visual, lane, &[], "the pointer off the lanes");

    let fade = 12;
    visual.update(|window, app| {
        state.update(app, |s, _| {
            s.set_clip_fade_in(first.raw(), fade)
                .expect("the first bar takes a fade in");
        });
        window.refresh();
    });
    visual.run_until_parked();
    let faded = a0 + TRIM + fade as f32 * tick_px;
    assert_handles(visual, lane, &[faded], "a fade in set, the pointer away");
    hover(visual, (a0 + a1) / 2.0, y);
    assert_handles(
        visual,
        lane,
        &[faded, ends(a0, a1)[1]],
        "a fade in set, the pointer on its bar",
    );
}
