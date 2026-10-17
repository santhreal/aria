//! Contracts for a held step button in the Viewer's control row.
//!
//! The class closed here: a repeat that outlives its press, and a repeat that
//! runs at a rate other than the frame rate of the tab in front. Asserted on
//! Program and on a Source tab, with both step buttons:
//!
//! - A click steps one frame and nothing follows it.
//! - A hold steps once on the press, once when the pause ends, and once per
//!   frame of the tab in front after that, and not before each is due.
//! - A release ends the repeat: the clock running on moves nothing.
//! - A hold that reaches either end of the tab ends itself while the button is
//!   still down.
//!
//! Not covered: a release the window never receives, as when focus leaves
//! mid-hold. The end of the tab bounds that loop.

use std::time::Duration;

use aria_models::Rational;
use gpui::{Modifiers, MouseButton};

use super::REPEAT_DELAY;
use crate::app::AppState;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::interned::intern;
use crate::types::ActivePanel;

/// Less than any frame interval the hold can run at.
const EPSILON: Duration = Duration::from_millis(1);

/// The frame the transport keys move.
fn frame_here(state: &AppState) -> i64 {
    match state.sources.focused() {
        Some(view) if state.source_has_keys() => view.tick,
        _ => state.playhead.value,
    }
}

/// One frame of the focused Source tab's take, read from the pool.
fn take_interval(state: &AppState) -> Duration {
    let media = state.sources.focused().expect("a Source tab").media;
    let item = state.store.media.iter().find(|item| item.id == media);
    let rate = item.expect("the take is in the pool").frame_rate;
    Duration::from_secs_f64(f64::from(rate.den) / f64::from(rate.num))
}

#[gpui::test]
fn a_held_step_repeats_once_per_frame_of_the_tab_in_front_until_released(
    cx: &mut gpui::TestAppContext,
) {
    let executor = cx.executor();
    let (state, visual, media) = shell_with_take(cx, "step-hold-repeat");
    // A sequence rate other than the take's, so a repeat on the wrong tab's
    // rate is caught on both sides.
    change(visual, &state, |state| {
        state
            .set_sequence_rate(Rational::FPS_60)
            .expect("set the sequence rate");
    });
    let program = visual.read(|app| state.read(app).playhead.rate.seconds_per_unit());
    let program = Duration::from_secs_f64(program);
    for side in ["Program", "Source"] {
        let interval = if side == "Program" {
            program
        } else {
            change(visual, &state, |state| {
                assert!(state.open_source(media.raw()), "the Source tab opens");
            });
            let take = visual.read(|app| take_interval(state.read(app)));
            assert!(
                take.abs_diff(program) > EPSILON * 4,
                "the take and the sequence run at one rate"
            );
            take
        };
        for (button, sign) in [("next-frame", 1), ("prev-frame", -1)] {
            change(visual, &state, |state| {
                state.set_playhead(30);
                if let Some(view) = state.sources.focused_mut() {
                    view.tick = 30;
                }
                state.active_panel = ActivePanel::Timeline;
            });
            let at = |visual: &mut gpui::VisualTestContext| {
                visual.read(|app| frame_here(state.read(app)))
            };
            let point = visual
                .debug_bounds(intern(button))
                .unwrap_or_else(|| panic!("the Viewer draws no {button}"))
                .center();

            visual.simulate_click(point, Modifiers::none());
            visual.run_until_parked();
            executor.advance_clock(REPEAT_DELAY * 2);
            visual.run_until_parked();
            assert_eq!(
                at(visual),
                30 + sign,
                "{side} {button}: a click stepped more than once"
            );

            visual.simulate_mouse_down(point, MouseButton::Left, Modifiers::none());
            visual.run_until_parked();
            let mut want = 30 + 2 * sign;
            assert_eq!(at(visual), want, "{side} {button}: the press did not step");
            // Each repeat is due a fixed wait after the one before fires, so
            // the clock is read against the running deadline, not against
            // where the previous check left it.
            let dues = std::iter::once(REPEAT_DELAY).chain(std::iter::repeat_n(interval, 5));
            let mut now = Duration::ZERO;
            let mut deadline = Duration::ZERO;
            for (repeat, due) in dues.enumerate() {
                deadline += due;
                executor.advance_clock(deadline - EPSILON - now);
                visual.run_until_parked();
                assert_eq!(
                    at(visual),
                    want,
                    "{side} {button}: repeat {repeat} came early"
                );
                executor.advance_clock(EPSILON * 2);
                now = deadline + EPSILON;
                visual.run_until_parked();
                want += sign;
                assert_eq!(
                    at(visual),
                    want,
                    "{side} {button}: repeat {repeat} did not come"
                );
            }

            visual.simulate_mouse_up(point, MouseButton::Left, Modifiers::none());
            visual.run_until_parked();
            assert!(
                !visual.read(|app| state.read(app).step_held()),
                "{side} {button}: the release left the hold live"
            );
            executor.advance_clock(interval * 10);
            visual.run_until_parked();
            assert_eq!(
                at(visual),
                want,
                "{side} {button}: a step came after the release"
            );
        }
    }
}

#[gpui::test]
fn a_hold_that_reaches_the_end_of_the_sequence_ends_while_the_button_is_down(
    cx: &mut gpui::TestAppContext,
) {
    let executor = cx.executor();
    let (state, visual, _) = shell_with_take(cx, "step-hold-end");
    let end = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    for (button, from, stop) in [("next-frame", end - 1, end), ("prev-frame", 1, 0)] {
        change(visual, &state, |state| state.set_playhead(from));
        let point = visual
            .debug_bounds(intern(button))
            .unwrap_or_else(|| panic!("the Viewer draws no {button}"))
            .center();
        visual.simulate_mouse_down(point, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        executor.advance_clock(REPEAT_DELAY + EPSILON);
        visual.run_until_parked();
        let (at, held) = visual.read(|app| {
            let state = state.read(app);
            (state.playhead.value, state.step_held())
        });
        assert_eq!(at, stop, "{button} did not stop at the end");
        assert!(
            !held,
            "{button}: the hold ran on at the end with the button down"
        );
        visual.simulate_mouse_up(point, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
    }
}

#[test]
fn the_repeat_rate_stays_inside_one_to_240_frames_a_second() {
    let mut state = AppState::new(
        aria_plugin::PluginContext::new(),
        crate::scratch_pool::scratch("step-hold-rate"),
    )
    .expect("state");
    for (rate, fps) in [
        (Rational::new(0, 0), 1.0),
        (Rational::new(-24, 1), 1.0),
        (Rational::new(1, 2), 1.0),
        (Rational::new(24, 1), 24.0),
        (Rational::new(30000, 1001), 30000.0 / 1001.0),
        (Rational::new(1000, 1), 240.0),
    ] {
        state.playhead.rate = rate;
        assert_eq!(
            state.frame_interval_here(),
            Duration::from_secs_f64(1.0 / fps),
            "a {rate:?} sequence repeats at the wrong rate"
        );
    }
}
