//! A pool card dragged onto the drawn Timeline lands on the lane and at the tick
//! it is released over.
//!
//! The geometry that names a lane and a tick for a pointer is pinned in
//! `lanes_anchor_tests`; nothing ran the drop that writes it. Here a card is
//! pressed, carried and released three times on the drawn shell: onto the second
//! audio lane, onto the first one a tick past a cut, and onto the strip under the
//! last lane a tick past a clip's start. A drop that loses the lane or the tick
//! the drag named, that skips the magnet, or that adds its lane at the wrong
//! edge each lands the clip somewhere else.
//!
//! Not covered: a drop with Alt that replaces a clip, a drop from the Source
//! tab, and a catalog item dropped on a lane.

use aria_models::{ClipId, TrackKind};
use gpui::{Bounds, Entity, Modifiers, MouseButton, Pixels, Point, VisualTestContext, point, px};

use crate::app::AppState;
use crate::types::{ActivePanel, EditMode, UiTrackKind};

/// Ticks in a second of the drawn sequence.
fn second(state: &Entity<AppState>, visual: &mut VisualTestContext) -> i64 {
    visual.read(|app| state.read(app).store.sequence.ticks_per_second().round() as i64)
}

/// Pixels per tick of the drawn timeline.
fn tick_px(state: &Entity<AppState>, visual: &mut VisualTestContext) -> f32 {
    visual.read(|app| {
        let state = state.read(app);
        crate::transport::TIMELINE_BASE_PPS * state.zoom / state.store.sequence.ticks_per_second()
    })
}

#[track_caller]
fn bounds(visual: &mut VisualTestContext, selector: &str) -> Bounds<Pixels> {
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    visual
        .debug_bounds(crate::interned::intern(selector))
        .unwrap_or_else(|| panic!("the shell draws no {selector}"))
}

/// Press the first pool card, carry it to `to` and let go there.
fn drag_card_to(visual: &mut VisualTestContext, to: Point<Pixels>) {
    let card = bounds(visual, "pool-item-0").center();
    visual.simulate_mouse_move(card, None, Modifiers::none());
    visual.simulate_mouse_down(card, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
    // Past the drag threshold, so the card is lifted before it travels.
    let lifted = point(card.x + px(12.0), card.y + px(12.0));
    for at in [lifted, to] {
        visual.simulate_mouse_move(at, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
    }
    visual.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
    visual.run_until_parked();
}

/// Every clip on the sequence as its lane's badge, its start and its end, by
/// start and then by lane.
fn placed(state: &Entity<AppState>, visual: &mut VisualTestContext) -> Vec<(String, i64, i64)> {
    visual.read(|app| {
        let state = state.read(app);
        let mut placed: Vec<(String, i64, i64)> = state
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
            .map(|clip| {
                let label = state
                    .clip_lane_label(ClipId::from_raw(clip.id))
                    .expect("a placed clip is on a lane");
                let badge = label.split_whitespace().next().unwrap_or_default();
                (
                    badge.to_string(),
                    clip.timeline_start_ticks,
                    clip.timeline_end_ticks,
                )
            })
            .collect();
        placed.sort_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0)));
        placed
    })
}

/// The pointer at the height of the element drawn under `target` (a lane's
/// header or a drop strip), a quarter of a tick past `tick` so it reads as
/// that tick.
fn over(
    state: &Entity<AppState>,
    visual: &mut VisualTestContext,
    target: &str,
    tick: i64,
) -> Point<Pixels> {
    let canvas = bounds(visual, "timeline-audio-canvas");
    let y = bounds(visual, target).center().y;
    let tick_px = tick_px(state, visual);
    let x = canvas.left() + px((tick as f32 + 0.25) * tick_px);
    assert!(
        x < canvas.right(),
        "tick {tick} is past the drawn audio side {canvas:?} at {tick_px} px a tick"
    );
    point(x, y)
}

/// **A pool card dropped on a lane, or on the strip under the last lane, lands
/// on that lane at the tick under the pointer, taken by a magnet in reach.**
#[gpui::test]
fn a_pool_card_lands_on_the_lane_and_tick_it_is_released_over(cx: &mut gpui::TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "pool-card-drop");
    visual.update(|window, app| {
        app.set_reduce_motion(true);
        state.update(app, |s, _| {
            s.import_media(crate::app::fixtures::asset("fixture-sine-48k-2s.wav"))
                .expect("import the sine");
            let audio = s
                .store
                .sequence
                .tracks
                .iter()
                .filter(|lane| lane.kind == UiTrackKind::Audio)
                .count();
            for n in audio..2 {
                s.add_track(TrackKind::Audio, format!("A{}", n + 1))
                    .expect("an audio lane");
            }
            // Overwrite, so a later drop moves none of the clips already
            // placed and each lands where it is released.
            s.edit_mode = EditMode::Overwrite;
            s.snap_enabled = true;
            s.focus_pane(ActivePanel::Media);
        });
        window.refresh();
    });
    visual.run_until_parked();
    let second = second(&state, visual);

    let to = over(&state, visual, "track-header-A2", second);
    drag_card_to(visual, to);
    let first = placed(&state, visual);
    assert_eq!(
        first
            .iter()
            .map(|(lane, start, _)| (lane.as_str(), *start))
            .collect::<Vec<_>>(),
        [("A2", second)],
        "a card let go over A2 one second in"
    );
    let (_, start, end) = first[0].clone();
    assert!(
        end - start > 2,
        "the sine is {} ticks long, too short to aim inside",
        end - start
    );

    // One tick past the clip's end on A1: the cut takes it.
    let to = over(&state, visual, "track-header-A1", end + 1);
    drag_card_to(visual, to);
    let length = end - start;
    assert_eq!(
        placed(&state, visual),
        [
            ("A2".to_string(), start, end),
            ("A1".to_string(), end, end + length)
        ],
        "a card let go a tick past a cut"
    );

    // Under the last lane, a tick past the first clip's start: a new lane at
    // the bottom of the audio side, and the clip's start takes the drop.
    let to = over(&state, visual, "lane-edge-bottom", start + 1);
    drag_card_to(visual, to);
    assert_eq!(
        placed(&state, visual),
        [
            ("A2".to_string(), start, end),
            ("A3".to_string(), start, end),
            ("A1".to_string(), end, end + length)
        ],
        "a card let go on the strip under the last lane"
    );
}
