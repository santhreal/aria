//! What a view change does to the timeline's zoom and horizontal offset.
//!
//! Four contracts live here. The follow moves the view only when the playhead
//! is about to leave it, so a view scrolled to on purpose survives playback
//! that stays inside it. A seek that lands off screen pages the view to the
//! playhead under every follow setting, and one that lands on screen, or a
//! drag in the ruler, leaves the view where it is. The offset carries how it
//! should be drawn: the timeline eases a view the editor chose and never eases
//! one that is moving under a hand, so every writer records which it is. And
//! the zoom the panel draws is the zoom the state holds, written in one place
//! and clamped there.

use std::sync::Arc;

use crate::transport;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::timeline_display::Follow;

/// The width the state fits and follows against, derived from the same chrome
/// the shell lays out with. A test that guessed a width would keep passing while
/// the follow measured a different one.
fn viewport(state: &AppState) -> f32 {
    let sidebar = if state.ui_settings.sidebar_collapsed {
        0.0
    } else {
        state.ui_settings.sidebar_width
    };
    let inspector = if state.ui_settings.inspector_collapsed {
        0.0
    } else {
        state.ui_settings.inspector_width
    };
    transport::editor_timeline_viewport_px(
        state.last_window_w,
        sidebar,
        inspector,
        state.ui_settings.outline_width,
    )
}

/// Where the playhead sits in pixels, the way the lanes place it.
fn playhead_px(state: &AppState) -> f32 {
    let fps = state.store.sequence.ticks_per_second();
    (state.playhead.value.max(0) as f32 / fps) * transport::TIMELINE_BASE_PPS * state.zoom
}

/// Ten minutes of sequence in a window wide enough to have a viewport, so a
/// playhead can be put outside the visible span without being clamped back.
fn ten_minutes() -> AppState {
    let (mut state, _clip) = with_clip();
    state.last_window_w = 1440.0;
    state.set_zoom(1.0, false);
    let fps = state.store.sequence.frame_rate.nominal_fps();
    state.store_mut().sequence.duration_ticks = fps * 600;
    state.scroll_timeline_to(0.0, false);
    state
}

#[test]
fn the_view_follows_a_playhead_that_has_run_past_it() {
    let mut state = ten_minutes();
    let fps = state.store.sequence.frame_rate.nominal_fps();
    state.place_playhead(fps * 300);

    assert!(
        state.follow_playhead(),
        "a playhead five minutes off screen"
    );

    let width = viewport(&state);
    let at = playhead_px(&state);
    assert!(state.scroll_x > 0.0);
    assert!(
        at > state.scroll_x && at < state.scroll_x + width,
        "playhead at {at} outside the followed window {}..{}",
        state.scroll_x,
        state.scroll_x + width
    );
    // It rests near the left, not centred: what plays next is what has to be
    // visible.
    assert!(at - state.scroll_x < width * 0.5);
    assert!(state.scroll_eased, "a view the editor chose arrives eased");
    assert_eq!(state.store.scroll_x, state.scroll_x);
}

#[test]
fn the_view_stays_where_it_is_while_it_still_shows_the_playhead() {
    let mut state = ten_minutes();
    let fps = state.store.sequence.frame_rate.nominal_fps();
    state.place_playhead(fps * 2);

    assert!(!state.follow_playhead(), "two seconds in is on screen");
    assert_eq!(state.scroll_x, 0.0);
    assert!(!state.scroll_eased);
}

#[test]
fn following_backwards_stops_at_the_start_of_the_sequence() {
    let mut state = ten_minutes();
    state.scroll_timeline_to(5_000.0, false);
    state.place_playhead(0);

    assert!(
        state.follow_playhead(),
        "the start is off screen to the left"
    );
    assert_eq!(state.scroll_x, 0.0, "there is nothing before tick zero");
    assert_eq!(state.store.scroll_x, 0.0);
}

/// Every follow setting. A setting added later fails to compile here until
/// what a seek does under it is decided.
fn every_follow() -> [Follow; 2] {
    let all = [Follow::Page, Follow::Smooth];
    for follow in all {
        match follow {
            Follow::Page | Follow::Smooth => {}
        }
    }
    all
}

/// The tick `px` pixels into the timeline at the state's zoom, rounded down.
fn tick_at(state: &AppState, px: f32) -> i64 {
    let pps = transport::TIMELINE_BASE_PPS * state.zoom;
    (px / pps * state.store.sequence.ticks_per_second()) as i64
}

/// The writers a seek moves the playhead through: a key, a menu row, a jump or
/// a Viewer button, and a scrub on the Viewer's strip or a property row's
/// strip.
const SEEKS: [(&str, fn(&mut AppState, i64)); 2] = [
    ("a seek", AppState::set_playhead),
    ("a strip scrub", AppState::scrub_playhead),
];

#[test]
fn a_seek_off_screen_pages_the_view_to_it_under_every_follow() {
    for follow in every_follow() {
        for (entry, seek) in SEEKS {
            let mut state = ten_minutes();
            state.ui_settings.timeline_display.follow = follow;
            let width = viewport(&state);
            let fps = state.store.sequence.frame_rate.nominal_fps();
            // The first frame past the right edge, five minutes in, then back
            // before the left edge.
            let ticks = [
                ("a frame past the view", tick_at(&state, width + 1.0) + 1),
                ("five minutes in", fps * 300),
                ("two seconds in", fps * 2),
            ];
            for (what, tick) in ticks {
                let before = state.scroll_x;
                seek(&mut state, tick);
                let at = playhead_px(&state);
                assert!(
                    state.scroll_x != before
                        && at >= state.scroll_x
                        && at - state.scroll_x < width * 0.5,
                    "{follow:?}: {entry} {what} left the playhead at {at} px of a view at \
                     {}..{}",
                    state.scroll_x,
                    state.scroll_x + width
                );
                assert!(
                    state.scroll_eased,
                    "{follow:?}: {entry} {what} paged without a glide"
                );
                assert_eq!(state.store.scroll_x, state.scroll_x);
            }
        }
    }
}

#[test]
fn a_seek_on_screen_and_a_ruler_drag_leave_the_view_where_it_is() {
    for follow in every_follow() {
        let mut state = ten_minutes();
        state.ui_settings.timeline_display.follow = follow;
        let width = viewport(&state);
        let fps = state.store.sequence.frame_rate.nominal_fps();
        // The last frame in the view stands inside the margin a page follow
        // keeps clear of, and far from the center a smooth follow holds: on
        // screen all the same.
        let ticks = [
            ("two seconds in", fps * 2),
            (
                "to the last frame in the view",
                tick_at(&state, width - 1.0),
            ),
        ];
        for (entry, seek) in SEEKS {
            for (what, tick) in ticks {
                seek(&mut state, tick);
                assert_eq!(
                    state.scroll_x, 0.0,
                    "{follow:?}: {entry} {what} moved the view"
                );
            }
        }
        state.scrub_playhead_on_ruler(fps * 300);
        assert_eq!(
            state.scroll_x, 0.0,
            "{follow:?}: a ruler drag past the view moved it"
        );
        assert!(!state.scroll_eased);
    }
}

#[test]
fn a_view_moved_by_hand_is_not_eased_even_after_one_that_was() {
    let mut state = ten_minutes();
    state.zoom_fit_sequence();
    assert!(state.scroll_eased, "a fit is a view the editor chose");

    state.scroll_timeline_to(400.0, false);
    assert_eq!(state.scroll_x, 400.0);
    assert_eq!(state.store.scroll_x, 400.0);
    assert!(
        !state.scroll_eased,
        "a wheel notch has to land under the hand that moved it"
    );
}

/// The scale carries the same decision the offset does, and for the same reason:
/// a wheel notch is anchored to the pointer, so the frame it lands on has to be
/// drawn at the scale that anchoring was computed for. Everything else is the
/// editor changing the scale, and it travels.
///
/// Not covered: the drawn value itself, which belongs to the glide store and is
/// tested there.
#[test]
fn a_scale_the_editor_chose_is_eased_and_one_a_pointer_anchored_is_not() {
    let mut state = ten_minutes();
    state.zoom_fit_sequence();
    assert!(state.zoom_eased, "a fit is a scale the editor chose");

    let anchored = state.zoom * transport::ZOOM_WHEEL_STEP;
    state.set_zoom(anchored, false);
    assert_eq!(state.zoom, transport::clamp_zoom(anchored));
    assert_eq!(
        state.store.zoom, state.zoom,
        "the panel draws the same scale"
    );
    assert!(
        !state.zoom_eased,
        "a pointer-anchored notch has to land at the scale it was anchored to"
    );

    state.set_zoom(state.zoom * transport::ZOOM_STEP, true);
    assert!(state.zoom_eased, "a keyed step travels there");
}

#[test]
fn the_scroll_offset_is_never_negative() {
    let mut state = ten_minutes();
    state.scroll_timeline_to(-120.0, true);
    assert_eq!(state.scroll_x, 0.0);
    assert_eq!(state.store.scroll_x, 0.0);
}

/// A throw is the only scroll that overshoots, and it stops being one the moment
/// anything else moves the view. Without the second half, a fit or a wheel notch
/// arriving after a flick would ride the spring and bounce for no reason.
#[test]
fn only_a_released_pan_settles_on_the_spring() {
    let mut state = ten_minutes();

    state.fling_timeline_to(800.0);
    assert_eq!(state.scroll_x, 800.0);
    assert_eq!(state.store.scroll_x, 800.0);
    assert!(state.scroll_fling, "a thrown pan settles from its momentum");
    assert!(state.scroll_eased, "a throw travels rather than jumps");

    state.scroll_timeline_to(200.0, true);
    assert!(
        !state.scroll_fling,
        "a fit arriving after a throw is not a throw"
    );

    state.fling_timeline_to(-40.0);
    assert_eq!(state.scroll_x, 0.0, "a throw is clamped like every scroll");
    assert!(state.scroll_fling);

    state.scroll_timeline_to(120.0, false);
    assert!(!state.scroll_fling, "a hand on a wheel ends the settle");
}

/// One measurement of the body, read by the fit, the follow, and the panel's
/// culling and edge fades. The panel used to cull against a 2000px guess, which
/// meant a narrow window built rows it had no room for and a wide one faded an
/// edge with sequence still beyond it.
#[test]
fn the_state_measures_the_body_the_panel_draws() {
    let mut state = ten_minutes();
    let open = state.timeline_viewport_px();
    assert_eq!(open, viewport(&state));

    state.ui_settings.sidebar_collapsed = true;
    let collapsed = state.timeline_viewport_px();
    assert_eq!(collapsed, viewport(&state));
    assert!(
        collapsed > open,
        "a collapsed rail is width zero, so the timeline gets it"
    );

    // Culling reads this width, so it must never be nothing: a zero would build
    // no ticks, no markers, and no clips at all.
    state.last_window_w = 0.0;
    assert!(state.timeline_viewport_px() >= 160.0);
}

/// The zoom has one writer, and the two copies of it cannot disagree.
///
/// The panel draws from the projection's copy and every verb reads the state's.
/// Four call sites used to write both by hand, each spelling the range out
/// again, so a fifth would have been one line from drawing at a zoom nothing
/// else agreed with.
#[test]
fn the_zoom_the_panel_draws_is_the_zoom_the_state_holds() {
    let mut state = ten_minutes();
    for target in [0.0, 0.05, 0.5, 1.0, 12.0, 400.0] {
        state.set_zoom(target, false);
        assert_eq!(state.zoom, state.store.zoom, "the panel draws a stale zoom");
        assert!(state.zoom >= transport::ZOOM_MIN);
        assert!(state.zoom <= transport::ZOOM_MAX);
    }
    state.set_zoom(f32::MAX, false);
    assert_eq!(state.zoom, transport::ZOOM_MAX);
    state.set_zoom(-1.0, false);
    assert_eq!(state.zoom, transport::ZOOM_MIN);
}

/// The projection is shared with the panels, not copied for each of them.
///
/// Three panels need the store for the length of a draw and cannot hold a borrow
/// of the state across the listeners they attach, so each used to deep-copy every
/// track, clip, and keyframe once per frame. Sharing it is only safe because a
/// write takes it back: the frame still holding the old one keeps drawing what it
/// measured.
#[test]
fn a_panel_holds_the_projection_without_copying_it() {
    let mut state = ten_minutes();
    let frame = state.store.clone();
    assert!(
        Arc::ptr_eq(&frame, &state.store),
        "a panel's handle is the same projection, not a copy of it"
    );

    state.set_zoom(2.0, false);
    assert!(
        !Arc::ptr_eq(&frame, &state.store),
        "a write has to take the store back before it changes it"
    );
    assert_eq!(frame.zoom, 1.0, "the frame that was drawing is untouched");
    assert_eq!(state.store.zoom, 2.0);

    // Nothing is holding it now, so the next write reuses the allocation.
    let before = Arc::as_ptr(&state.store);
    state.set_zoom(3.0, false);
    assert_eq!(
        Arc::as_ptr(&state.store),
        before,
        "a write with no frame in flight must not allocate"
    );
}

#[test]
fn selection_span_reads_the_projected_selection_flag() {
    let (mut state, _) = with_clip();
    state.selection.clear();
    let clip = &mut std::sync::Arc::make_mut(&mut state.store).sequence.tracks[0].clips[0];
    clip.selected = true;
    let expected = (clip.timeline_start_ticks, clip.timeline_end_ticks);
    assert_eq!(state.selection_span(), Some(expected));
}

/// A fit frames the sequence's length in seconds at every rate the settings
/// dialog offers.
///
/// The class: a conversion that took the rate's numerator as the ticks in a
/// second, so a 23.976 sequence (24000/1001) measured a thousandth of its
/// length and fit at the widest zoom. The expectation is in seconds, so it does
/// not lean on the conversion under test.
#[test]
fn a_fit_frames_thirty_seconds_at_every_offered_rate() {
    let mut state = ten_minutes();
    let width = viewport(&state);
    let want = width / (30.0 * transport::TIMELINE_BASE_PPS);
    assert!(
        want > transport::ZOOM_MIN && want < transport::ZOOM_MAX,
        "thirty seconds fit inside the zoom range"
    );
    for rate in crate::surfaces::settings_pick::RATES {
        let sequence = &mut state.store_mut().sequence;
        sequence.frame_rate = rate;
        sequence.duration_ticks = (rate.as_f64() * 30.0).round() as i64;
        state.zoom_fit_sequence();
        assert!(
            (state.zoom - want).abs() < want * 0.01,
            "{rate:?}: thirty seconds fit at zoom {} instead of {want}",
            state.zoom
        );
    }
}
