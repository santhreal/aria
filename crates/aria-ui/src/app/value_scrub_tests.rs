//! Contracts for scrubbable values.
//!
//! The class closed here is "a value gesture lands somewhere other than where
//! the pointer or the keyboard put it": a drag that compounds every move onto
//! the last one, a drag that leaves one undo step per pixel, an Escape that
//! leaves the dragged value behind, a typed value written as a delta from the
//! wrong origin, and a field that commits text stating no number. Both shapes
//! of write a row hands the gesture are driven through every case: one that
//! adds to the clip's current value, and one that adds to the value the row
//! was drawn with.
//!
//! The last case drives a drawn shell: past the slop the shell locks the
//! pointer, a locked pointer's relative travel still moves the value, and
//! letting go or Escape gives the pointer back.
//!
//! What it does not catch: where a panel draws its readouts, and a platform
//! that refuses the lock, where travel stops at the screen edge.

use std::cell::Cell;
use std::rc::Rc;

use aria_models::ClipParam;

use super::AppState;
use super::premiere_tests::with_clip;
use super::value_scrub::{ScrubTarget, ValueKind, Write};
use crate::motion::MotionKey;

/// The two shapes of write a row hands a readout.
#[derive(Debug, Clone, Copy)]
enum Shape {
    /// Adds the delta to the value the model holds when the write runs.
    Incremental,
    /// Adds the delta to the value the row was drawn with.
    Captured,
}

const SHAPES: [Shape; 2] = [Shape::Incremental, Shape::Captured];

fn opacity(state: &AppState) -> f32 {
    state
        .param_scalar(ClipParam::Opacity, None)
        .expect("opacity")
}

/// An opacity readout as a row draws it at the current value.
fn opacity_target(state: &AppState, name: &str, shape: Shape) -> ScrubTarget {
    let drawn = opacity(state);
    let write: Write = match shape {
        Shape::Incremental => Rc::new(|state: &mut AppState, delta| {
            state.nudge_param(ClipParam::Opacity, None, delta)
        }),
        Shape::Captured => Rc::new(move |state: &mut AppState, delta| {
            state.set_param_scalar(ClipParam::Opacity, None, drawn + delta)
        }),
    };
    ScrubTarget {
        key: MotionKey::new("value-scrub-test", name),
        value: drawn,
        unit: 0.01,
        kind: ValueKind::Number { decimals: 2 },
        write,
    }
}

fn start(at: f32) -> AppState {
    let (mut state, _) = with_clip();
    state
        .set_param_scalar(ClipParam::Opacity, None, at)
        .expect("start value");
    state
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1e-4
}

#[test]
fn a_press_inside_the_slop_writes_nothing() {
    for shape in SHAPES {
        let mut state = start(0.5);
        let depth = state.history_depth();
        state.press_value(opacity_target(&state, "a", shape));
        assert!(!state.drag_value(2.0, false, false), "{shape:?}");
        assert!(close(opacity(&state), 0.5), "{shape:?}");
        assert_eq!(state.history_depth(), depth, "{shape:?}");
    }
}

/// Two pixels are one unit, every move is measured from the press, and the
/// whole drag is one undo step that undo takes back to the start.
#[test]
fn a_drag_writes_its_travel_from_the_press_as_one_undo_step() {
    for shape in SHAPES {
        let mut state = start(0.5);
        let depth = state.history_depth();
        state.press_value(opacity_target(&state, "a", shape));
        assert!(state.drag_value(20.0, false, false), "{shape:?}");
        assert!(
            close(opacity(&state), 0.6),
            "{shape:?}: 20 px should be 10 units, read {}",
            opacity(&state)
        );
        assert!(state.drag_value(-10.0, false, false), "{shape:?}");
        assert!(
            close(opacity(&state), 0.55),
            "{shape:?}: moving back compounded onto the last move, read {}",
            opacity(&state)
        );
        assert!(state.release_value(), "{shape:?}");
        assert!(
            !state.value_entry_open(),
            "{shape:?}: a drag opened a field"
        );
        assert_eq!(state.history_depth(), depth + 1, "{shape:?}");
        assert!(state.undo(), "{shape:?}");
        assert!(close(opacity(&state), 0.5), "{shape:?}");
    }
}

#[test]
fn shift_moves_ten_units_and_ctrl_a_tenth() {
    for shape in SHAPES {
        let mut state = start(0.2);
        state.press_value(opacity_target(&state, "a", shape));
        state.drag_value(10.0, true, false);
        assert!(
            close(opacity(&state), 0.7),
            "{shape:?}: Shift read {}",
            opacity(&state)
        );
        state.drag_value(20.0, false, true);
        assert!(
            close(opacity(&state), 0.71),
            "{shape:?}: Ctrl read {}",
            opacity(&state)
        );
        state.release_value();
    }
}

#[test]
fn escape_during_a_drag_restores_the_value_and_the_history() {
    for shape in SHAPES {
        let mut state = start(0.5);
        let depth = state.history_depth();
        state.press_value(opacity_target(&state, "a", shape));
        state.drag_value(60.0, false, false);
        assert!(close(opacity(&state), 0.8), "{shape:?}");
        assert!(state.cancel_value_drag(), "{shape:?}");
        assert!(close(opacity(&state), 0.5), "{shape:?}");
        assert_eq!(state.history_depth(), depth, "{shape:?}");
        assert!(
            !state.release_value(),
            "{shape:?}: the press outlived Escape"
        );
    }
}

/// A click opens the value selected, so the first key replaces it, and Enter
/// writes what was typed as one undo step.
#[test]
fn a_click_opens_the_value_as_text_and_enter_writes_it() {
    for shape in SHAPES {
        let mut state = start(0.5);
        let depth = state.history_depth();
        state.press_value(opacity_target(&state, "a", shape));
        assert!(state.release_value(), "{shape:?}");
        assert!(state.value_entry_open(), "{shape:?}");
        assert_eq!(state.value_entry_text(), "0.50", "{shape:?}");
        for ch in "0.25".chars() {
            state.value_entry_push(ch);
        }
        assert_eq!(state.value_entry_text(), "0.25", "{shape:?}");
        assert!(state.commit_value_entry(), "{shape:?}");
        assert!(!state.value_entry_open(), "{shape:?}");
        assert!(
            close(opacity(&state), 0.25),
            "{shape:?}: read {}",
            opacity(&state)
        );
        assert_eq!(state.history_depth(), depth + 1, "{shape:?}");
    }
}

#[test]
fn escape_closes_the_field_without_writing() {
    for shape in SHAPES {
        let mut state = start(0.5);
        let depth = state.history_depth();
        state.press_value(opacity_target(&state, "a", shape));
        state.release_value();
        state.value_entry_push('1');
        state.cancel_value_entry();
        assert!(!state.value_entry_open(), "{shape:?}");
        assert!(close(opacity(&state), 0.5), "{shape:?}");
        assert_eq!(state.history_depth(), depth, "{shape:?}");
    }
}

/// Text that states no number is reported and kept, so the operator can fix
/// it; letters never reach the draft at all.
#[test]
fn text_stating_no_number_keeps_the_field_and_reports_it() {
    let mut state = start(0.5);
    state.press_value(opacity_target(&state, "a", Shape::Incremental));
    state.release_value();
    for ch in "x-.".chars() {
        state.value_entry_push(ch);
    }
    assert_eq!(state.value_entry_text(), "-.");
    assert!(!state.commit_value_entry());
    assert!(state.value_entry_open());
    assert!(
        state
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("-. is not a number")),
        "read {:?}",
        state.last_error
    );
    assert!(close(opacity(&state), 0.5));
}

#[test]
fn backspace_on_the_selected_draft_clears_it() {
    let mut state = start(0.5);
    state.press_value(opacity_target(&state, "a", Shape::Incremental));
    state.release_value();
    state.value_entry_pop();
    assert_eq!(state.value_entry_text(), "");
    state.value_entry_push('1');
    state.value_entry_push('2');
    state.value_entry_pop();
    assert_eq!(state.value_entry_text(), "1");
}

/// Tab commits and opens the readout painted after the open one, Shift+Tab the
/// one painted before it, and the last readout wraps to the first.
#[test]
fn tab_commits_and_opens_the_neighbouring_readout() {
    let mut state = start(0.5);
    let first = opacity_target(&state, "first", Shape::Incremental);
    let second = opacity_target(&state, "second", Shape::Incremental);
    let third = opacity_target(&state, "third", Shape::Incremental);

    state.press_value(second.clone());
    state.release_value();
    for target in [&first, &second, &third] {
        state.value_painted(target);
    }
    for ch in "0.4".chars() {
        state.value_entry_push(ch);
    }
    state.tab_value_entry(false);
    assert!(close(opacity(&state), 0.4), "read {}", opacity(&state));
    let open = state.value_scrubs().entry_for(third.key);
    assert!(open.is_some(), "Tab did not open the next readout");

    for target in [&first, &second, &third] {
        state.value_painted(target);
    }
    state.tab_value_entry(true);
    assert!(state.value_scrubs().entry_for(second.key).is_some());

    // The last readout's next is the first painted in the following frame.
    state.cancel_value_entry();
    state.press_value(third.clone());
    state.release_value();
    for target in [&first, &second, &third, &first] {
        state.value_painted(target);
    }
    state.tab_value_entry(false);
    assert!(state.value_scrubs().entry_for(first.key).is_some());
}

/// Open an opacity readout as text, the way a click on it does. The paste
/// sweep focuses the value field through this.
pub(crate) fn open_probe_entry(state: &mut AppState) {
    let target = opacity_target(state, "probe", Shape::Incremental);
    state.press_value(target);
    state.release_value();
}

#[test]
fn a_frame_count_reads_and_types_as_timecode() {
    let kind = ValueKind::Frames { num: 24, den: 1 };
    assert_eq!(kind.format(48.0), "00:00:02:00");
    assert_eq!(kind.parse("00:00:01:12"), Some(36.0));
    assert_eq!(kind.parse("12"), Some(12.0));
    assert_eq!(kind.parse("soon"), None);
    let number = ValueKind::Number { decimals: 1 };
    assert_eq!(number.format(2.25), "2.2");
    assert_eq!(number.parse(" -3.5 "), Some(-3.5));
    assert_eq!(number.parse("inf"), None);
}

/// A frame count never takes a fractional write, whatever the rate.
#[test]
fn a_frame_count_drags_by_whole_frames() {
    let mut state = start(0.5);
    let seen = Rc::new(std::cell::RefCell::new(Vec::new()));
    let log = Rc::clone(&seen);
    let target = ScrubTarget {
        key: MotionKey::new("value-scrub-test", "frames"),
        value: 10.0,
        unit: 1.0,
        kind: ValueKind::Frames { num: 24, den: 1 },
        write: Rc::new(move |_: &mut AppState, delta| {
            log.borrow_mut().push(delta);
            Ok(())
        }),
    };
    state.press_value(target);
    for x in [104.0, 105.0, 107.0, 109.0] {
        state.drag_value(x, false, true);
    }
    state.drag_value(140.0, false, false);
    state.release_value();
    let seen = seen.borrow();
    assert!(!seen.is_empty());
    assert!(
        seen.iter().all(|delta| delta.fract() == 0.0),
        "fractional frame writes: {seen:?}"
    );
}

/// A readout whose write keeps the delta it was handed, so a drawn shell needs
/// no clip to drag.
fn recording_target(wrote: &Rc<Cell<f32>>) -> ScrubTarget {
    let sink = Rc::clone(wrote);
    ScrubTarget {
        key: MotionKey::new("value-scrub-test", "lock"),
        value: 0.0,
        unit: 1.0,
        kind: ValueKind::Number { decimals: 0 },
        write: Rc::new(move |_: &mut AppState, delta| {
            sink.set(delta);
            Ok(())
        }),
    }
}

/// Past the slop the shell locks the pointer and reads relative travel, which
/// moves the value while the locked position holds still. Letting go and
/// Escape both give the pointer back.
#[gpui::test]
fn a_drag_locks_the_pointer_and_reads_relative_travel(cx: &mut gpui::TestAppContext) {
    use gpui::{Modifiers, MouseButton, VisualTestContext, point, px};

    fn step(visual: &mut VisualTestContext, dx: f32) {
        visual.simulate_locked_mouse_move(
            point(px(dx), px(0.)),
            MouseButton::Left,
            Modifiers::none(),
        );
    }
    fn locked(visual: &mut VisualTestContext) -> bool {
        visual.update(|window, _| window.is_pointer_locked())
    }

    let (state, visual) = crate::test_support::drawn_shell(cx, "value-scrub-lock");
    visual.run_until_parked();
    for end in ["release", "escape"] {
        let wrote = Rc::new(Cell::new(0.0));
        let target = recording_target(&wrote);
        visual.update(|_, app| state.update(app, |state, _| state.press_value(target)));
        step(visual, 2.0);
        assert!(
            !locked(visual),
            "{end}: a press inside the slop locked the pointer"
        );
        assert_eq!(wrote.get(), 0.0, "{end}: a press inside the slop wrote");
        step(visual, 18.0);
        assert!(locked(visual), "{end}: the drag left the pointer free");
        assert_eq!(wrote.get(), 10.0, "{end}: 20 px of travel is 10 units");
        let held = visual.update(|window, _| window.mouse_position());
        step(visual, 20.0);
        assert_eq!(
            visual.update(|window, _| window.mouse_position()),
            held,
            "{end}: the locked pointer moved"
        );
        assert_eq!(
            wrote.get(),
            20.0,
            "{end}: a locked pointer's travel was lost"
        );
        if end == "release" {
            visual.simulate_mouse_up(held, MouseButton::Left, Modifiers::none());
        } else {
            visual.simulate_keystrokes("escape");
        }
        visual.run_until_parked();
        assert!(!locked(visual), "{end}: the pointer stayed locked");
        assert!(
            !visual.read(|app| state.read(app).value_scrubs().pressed()),
            "{end}: the press outlived the gesture"
        );
    }
}
