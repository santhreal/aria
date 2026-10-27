//! Escape during a drag the shell carries for a timeline element puts back
//! what its press found, and ends the drag: a move after Escape and the
//! release after it change nothing.
//!
//! The shell carries a drag begun on the ruler, the lane groups or the graph
//! with a hold, and a hold is made with what Escape puts back, so a drag that
//! gains a hold gains an Escape. The class closed here: a held drag whose
//! Escape put nothing back, or left the drag held so its release still landed
//! it. A ruler scrub, a clip moved, a clip's edge trimmed, a key dragged on the
//! graph and a band drawn on a lane are each pressed, carried, escaped,
//! carried again and let go, and what the press found reads the same after
//! Escape, the move and the release.
//!
//! The divider's Escape is in `held_drag_tests`, the zoom scroll bar's in
//! `zoom_bar_tests`, the work area bar's in `work_area_tests`, the outline
//! border's in `outline_column_tests`, and a band on a property row's in
//! `property_rows_tests`.
//!
//! Not covered: Escape on a fade, a slip, a transition, a key on a property
//! row, a key on a mask's path and a clip's line, which the same canvas cancel
//! drops and no test here drives.

use std::fmt::Debug;

use gpui::{Pixels, Point, VisualTestContext, point, px};

use crate::app::held_drag_tests::{
    bounds, carry, graph_key, opacity_keys, picture_clip, press, release, tick_px,
};
use crate::app::viewer_row_tests::{change, shell_with_take};

/// Press at `at`, carry to `to` and press Escape, then carry to `on` and let
/// go there. `found` reads what the press found: the carry changes it, and
/// Escape puts it back for good.
#[track_caller]
fn escaped<T: PartialEq + Debug>(
    visual: &mut VisualTestContext,
    (at, to, on): (Point<Pixels>, Point<Pixels>, Point<Pixels>),
    found: impl Fn(&mut VisualTestContext) -> T,
    name: &str,
) {
    let before = found(visual);
    press(visual, at);
    carry(visual, to);
    assert_ne!(
        found(visual),
        before,
        "{name}: the drag changed nothing to put back"
    );
    visual.simulate_keystrokes("escape");
    visual.run_until_parked();
    assert_eq!(found(visual), before, "{name}: Escape");
    carry(visual, on);
    assert_eq!(found(visual), before, "{name}: a move after Escape");
    release(visual, on);
    assert_eq!(found(visual), before, "{name}: the release after Escape");
}

#[gpui::test]
fn escape_puts_the_playhead_back_where_a_ruler_scrub_found_it(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "escape-ruler-scrub");
    let ruler = bounds(visual, "timeline-ruler");
    let tick_px = tick_px(&state, visual);
    let duration = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    change(visual, &state, |state| state.set_playhead(duration / 2));
    // A quarter of a tick past the tick, so a press reads as that tick whether
    // the ruler rounds or floors.
    let x = |tick: i64| ruler.left() + px((tick as f32 + 0.25) * tick_px);
    let y = ruler.center().y;
    escaped(
        visual,
        (
            point(x(duration / 4), y),
            point(x(duration * 3 / 4), y + px(60.0)),
            point(x(duration / 8), y),
        ),
        |visual| visual.read(|app| state.read(app).playhead.value),
        "a ruler scrub",
    );
}

#[gpui::test]
fn escape_puts_back_a_clip_moved_and_a_clip_trimmed(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "escape-clip-drags");
    let (clip, start, end) = picture_clip(&state, visual);
    let canvas = bounds(visual, "timeline-video-canvas");
    let lane = bounds(visual, "track-header-V1");
    let ruler = bounds(visual, "timeline-ruler");
    let tick_px = tick_px(&state, visual);
    // Where the clip is, the edit it is at, and what the drag holds.
    let found = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let state = state.read(app);
            let span = state
                .store
                .sequence
                .tracks
                .iter()
                .flat_map(|lane| lane.clips.iter())
                .find(|c| c.id == clip)
                .map(|c| (c.track_id, c.timeline_start_ticks, c.timeline_end_ticks));
            (
                span,
                state.revision,
                state.trim_feedback.is_some(),
                state.held_move.is_some(),
                state.held_edit.is_some(),
                state.drag_project.is_some(),
            )
        })
    };
    // The name strip: clear of the line.
    let y = lane.top() + px(10.0);
    // Mid-bar: the body, clear of the edges and the fade grips.
    let body = point(canvas.left() + px((start + end) as f32 * 0.5 * tick_px), y);
    escaped(
        visual,
        (
            body,
            point(body.x + px(24.0 * tick_px), ruler.center().y),
            point(body.x + px(40.0 * tick_px), y),
        ),
        &found,
        "a clip moved",
    );
    // Inside the tail edge's grip.
    let tail = point(canvas.left() + px(end as f32 * tick_px) - px(3.0), y);
    escaped(
        visual,
        (
            tail,
            point(tail.x - px(40.0), y),
            point(tail.x - px(80.0), y),
        ),
        &found,
        "a clip's tail trimmed",
    );
}

#[gpui::test]
fn escape_puts_back_a_key_dragged_on_the_graph(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "escape-graph-key");
    let (clip, key, _) = graph_key(&state, visual);
    escaped(
        visual,
        (
            key,
            point(key.x, key.y - px(30.0)),
            point(key.x, key.y - px(50.0)),
        ),
        |visual| {
            let held = visual.read(|app| {
                state
                    .read(app)
                    .with_prop_rows(|rows| rows.graph_drag.is_some())
            });
            (opacity_keys(&state, visual, clip), held)
        },
        "a graph key",
    );
}

#[gpui::test]
fn escape_drops_a_band_drawn_on_a_lane(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "escape-band");
    let (_, start, end) = picture_clip(&state, visual);
    let canvas = bounds(visual, "timeline-video-canvas");
    let lane = bounds(visual, "track-header-V1");
    let tick_px = tick_px(&state, visual);
    // Past the clip's tail, where the lane is empty.
    let at = point(
        canvas.left() + px(end as f32 * tick_px) + px(40.0),
        lane.top() + px(10.0),
    );
    assert!(at.x < canvas.right(), "the lane is empty past the take");
    // A click on the empty lane deselects, so the band has a clip to select.
    press(visual, at);
    release(visual, at);
    let mid = canvas.left() + px((start + end) as f32 * 0.5 * tick_px);
    escaped(
        visual,
        (at, point(mid, at.y), point(canvas.left(), at.y)),
        |visual| {
            visual.read(|app| {
                let state = state.read(app);
                (state.marquee.is_some(), state.selection.clone())
            })
        },
        "a band",
    );
}
