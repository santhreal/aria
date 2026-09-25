//! What a running animation costs the rest of the window.
//!
//! The class these close: an animation that repaints everything. A frame has
//! two costs and this file covers both. The pulse clock owns the first — which
//! views a tick repaints, and whether the clock keeps ticking once nothing is
//! animating. The state poll owns the second — what an animating frame asks the
//! engine for, measured with a full sequence loaded so a per-frame projection
//! would show up as time.
//!
//! What they do not catch: how long gpui spends laying out the subtree it was
//! handed. `with_animation` asks for a window frame; nothing here can make that
//! frame cheaper than the elements in it.

use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui::{AppContext as _, TestAppContext};

use super::*;
use crate::app::AppState;
use crate::app::premiere_tests::with_sequence;
use crate::types::AppMenu;

/// An entity that draws and can be told to redraw. Two of them stand for two
/// panels: a lease is per view, and so is the repaint it drives.
struct Marker;

#[test]
fn a_tick_repaints_only_the_views_that_asked_for_it() {
    let cx = TestAppContext::single();
    cx.update(|cx| {
        let animating = cx.new(|_| Marker);
        let still = cx.new(|_| Marker);
        pulse_delta(&ZERON_PULSE, animating.entity_id(), cx);

        assert_eq!(
            pulse_tick(cx),
            Tick::Repainted(1),
            "a tick repainted a view nobody was animating"
        );
        // Held so the id cannot be reused by another entity mid-test.
        drop(still);
    });
}

#[test]
fn a_second_animation_joins_the_clock_instead_of_replacing_it() {
    let cx = TestAppContext::single();
    cx.update(|cx| {
        let first = cx.new(|_| Marker);
        let second = cx.new(|_| Marker);
        pulse_delta(&ZERON_PULSE, first.entity_id(), cx);
        assert_eq!(pulse_tick(cx), Tick::Repainted(1));

        pulse_delta(&ZERON_PULSE, second.entity_id(), cx);
        assert_eq!(
            pulse_tick(cx),
            Tick::Repainted(2),
            "the second lease displaced the first"
        );
    });
}

#[test]
fn the_clock_parks_when_the_last_lease_expires() {
    let cx = TestAppContext::single();
    let view = cx.update(|cx| {
        let view = cx.new(|_| Marker);
        pulse_delta(&ZERON_PULSE, view.entity_id(), cx);
        view
    });
    // A lease is renewed by drawing. Stop drawing and the clock has to stop:
    // otherwise an idle window pays a repaint every 33 ms forever.
    std::thread::sleep(PULSE_LEASE + Duration::from_millis(20));
    cx.update(|cx| {
        assert_eq!(
            pulse_tick(cx),
            Tick::Parked,
            "the clock kept ticking over an idle window"
        );
        // Parked means parked: the next tick has nothing to do either.
        assert_eq!(pulse_tick(cx), Tick::Parked);
        drop(view);
    });
}

#[test]
fn reduced_motion_takes_no_lease_at_all() {
    let cx = TestAppContext::single();
    cx.update(|cx| {
        cx.set_reduce_motion(true);
        let view = cx.new(|_| Marker);
        assert_eq!(
            pulse_delta(&ZERON_PULSE, view.entity_id(), cx),
            0.0,
            "a pulse still moved under reduced motion"
        );
        assert_eq!(
            pulse_tick(cx),
            Tick::Parked,
            "reduced motion still started the repaint clock"
        );
    });
}

/// The state work of one frame, as the fastest of `runs`: scheduler noise can
/// only make a sample worse, so the minimum is the honest cost.
fn frame_cost(state: &mut AppState, runs: usize) -> Duration {
    let mut best = Duration::MAX;
    for _ in 0..runs {
        let start = Instant::now();
        state.sync_from_engine();
        best = best.min(start.elapsed());
    }
    best
}

#[test]
fn an_animating_menu_frame_does_not_reproject_the_timeline() {
    let mut state = with_sequence(4, 60);
    state.open_menu = Some(AppMenu::File);
    let drawn = Arc::clone(&state.store);

    // A menu opens over 60 frames' worth of polls at most, and not one of them
    // changes anything the projection reads.
    let cost = frame_cost(&mut state, 60);
    assert!(
        Arc::ptr_eq(&drawn, &state.store),
        "opening a menu rebuilt the timeline projection"
    );
    // 480 clips project in ~400 µs, and a 60 Hz frame has 16 ms: a poll that
    // reprojected would spend a fortieth of the frame on a store nothing
    // changed. The memo path is a handful of comparisons. The ceiling is loose
    // on purpose — it fails on a projection, not on a slow machine.
    assert!(
        cost < Duration::from_micros(50),
        "an animating frame cost {cost:?} of state work"
    );
}

#[test]
fn frame_cost_does_not_follow_the_clip_count() {
    let mut small_state = with_sequence(4, 60);
    let small = frame_cost(&mut small_state, 200);
    let mut large_state = with_sequence(4, 240);
    let large = frame_cost(&mut large_state, 200);
    // Four times the clips. A projection per frame would show that; comparing a
    // revision, a playhead and a selection of one does not. The microsecond is
    // timer granularity, not slack: on a coarse clock the small sequence can
    // round to zero, and a single tick of growth is quantization, not a
    // projection (which costs hundreds of microseconds — see above).
    assert!(
        large < small * 4 + Duration::from_micros(1),
        "frame cost grew with the sequence: {small:?} at 480 clips, {large:?} at 1920"
    );
}
