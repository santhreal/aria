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

#[gpui::test]
fn pressing_a_linked_clip_line_or_right_clicking_puts_that_clip_first_and_shift_toggles(
    cx: &mut TestAppContext,
) {
    use super::BarRows;
    use super::tests::lane_of;
    use gpui::MouseButton;

    let shot = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/fixture-640x360-24fps-2s-av.mp4");
    let (state, visual, _) = placed(cx, "linked-press", shot);
    let (v1_clip, a1_clip, v1_line_frac) = visual.read(|app| {
        let s = state.read(app);
        let v1 = s.store.sequence.tracks[0].clips[0].id;
        let a1 = s.store.sequence.tracks[1].clips[0].id;
        let line = s
            .ui_clip_ref(ClipId::from_raw(v1))
            .and_then(|clip| clip.line.as_ref())
            .expect("V1 clip has an opacity line");
        (
            ClipId::from_raw(v1),
            ClipId::from_raw(a1),
            line.fraction_at(0.0),
        )
    });

    let v1 = v1_lane(visual);
    let a1 = lane_of(visual, "timeline-audio-canvas", "track-header-A1");
    let (x0, x1, _) = card(visual, &state, v1_clip);
    let mid_x = px((x0 + x1) * 0.5);
    let a1_body = point(mid_x, a1.top() + px(6.0));
    let v1_line = point(
        mid_x,
        v1.top() + px(BarRows::of(f32::from(v1.size.height)).line_y(v1_line_frac)),
    );

    // Press A1's body: both halves stay selected with A1 first.
    visual.simulate_mouse_down(a1_body, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(a1_body, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    visual.read(|app| {
        let s = state.read(app);
        assert_eq!(s.selected_clip(), Some(a1_clip));
        assert_eq!(s.selected_model().map(|c| c.id), Some(a1_clip));
        assert_eq!(s.selection, vec![a1_clip, v1_clip]);
    });

    // Press V1's line: the set is kept and V1 moves to the front.
    visual.simulate_mouse_down(v1_line, MouseButton::Left, Modifiers::none());
    visual.simulate_mouse_up(v1_line, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    visual.read(|app| {
        let s = state.read(app);
        assert_eq!(s.selected_clip(), Some(v1_clip));
        assert_eq!(s.selected_model().map(|c| c.id), Some(v1_clip));
        assert_eq!(s.selection, vec![v1_clip, a1_clip]);
    });

    // Right-click A1's body, then V1's line: each puts the clicked half first
    // while keeping both halves selected.
    visual.simulate_mouse_down(a1_body, MouseButton::Right, Modifiers::none());
    visual.simulate_mouse_up(a1_body, MouseButton::Right, Modifiers::none());
    visual.run_until_parked();
    visual.read(|app| {
        let s = state.read(app);
        assert_eq!(s.selected_clip(), Some(a1_clip));
        assert_eq!(s.selected_model().map(|c| c.id), Some(a1_clip));
        assert_eq!(s.selection, vec![a1_clip, v1_clip]);
    });
    visual.update(|window, app| {
        state.update(app, |s, _| s.close_context());
        window.refresh();
    });
    visual.run_until_parked();

    visual.simulate_mouse_down(v1_line, MouseButton::Right, Modifiers::none());
    visual.simulate_mouse_up(v1_line, MouseButton::Right, Modifiers::none());
    visual.run_until_parked();
    visual.read(|app| {
        let s = state.read(app);
        assert_eq!(s.selected_clip(), Some(v1_clip));
        assert_eq!(s.selected_model().map(|c| c.id), Some(v1_clip));
        assert_eq!(s.selection, vec![v1_clip, a1_clip]);
    });
    visual.update(|window, app| {
        state.update(app, |s, _| s.close_context());
        window.refresh();
    });
    visual.run_until_parked();

    // Shift-clicking V1's line toggles the linked pair out of and back into the
    // selection, as shift-click on the clip body does while linked selection
    // is on; the pressed clip comes back first.
    let shift = Modifiers {
        shift: true,
        ..Modifiers::none()
    };
    visual.simulate_mouse_down(v1_line, MouseButton::Left, shift);
    visual.simulate_mouse_up(v1_line, MouseButton::Left, shift);
    visual.run_until_parked();
    visual.read(|app| {
        let s = state.read(app);
        assert!(
            s.linked_selection,
            "the pair toggles together only while linked"
        );
        assert_eq!(s.selection, Vec::<ClipId>::new());
    });

    visual.simulate_mouse_down(v1_line, MouseButton::Left, shift);
    visual.simulate_mouse_up(v1_line, MouseButton::Left, shift);
    visual.run_until_parked();
    visual.read(|app| {
        let s = state.read(app);
        assert_eq!(s.selection, vec![v1_clip, a1_clip]);
    });
}
