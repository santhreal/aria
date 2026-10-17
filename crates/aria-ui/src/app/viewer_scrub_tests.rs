//! Contracts for the Viewer's scrubbable playhead field and its mini ruler.
//!
//! The class closed here: a Viewer drag that stops at the edge of the element
//! pressed, moves a tab other than the one in front, opens the field for
//! typing after it moved the playhead, or banks travel past the end of the
//! sequence. Asserted through the state for the arithmetic, and through the
//! drawn shell for the press, the pointer lock, the drag off the strip, and
//! the release.
//!
//! Not covered: the cursor shape over the field and the strip.

use gpui::{Modifiers, MouseButton, VisualTestContext, point, px};

use crate::app::AppState;
use crate::app::clock::ClockSite;
use crate::app::premiere_tests::with_clip;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::interned::intern;

/// A state with a clip on the timeline and the playhead on `at`.
fn parked(at: i64) -> AppState {
    let (mut state, _) = with_clip();
    assert!(
        state.store.sequence.duration_ticks > 100,
        "the clip is long enough to scrub across"
    );
    state.set_playhead(at);
    state
}

#[test]
fn a_drag_on_the_playhead_field_moves_a_frame_per_two_pixels() {
    let mut state = parked(40);
    state.press_clock(ClockSite::Viewer);

    assert!(
        !state.drag_viewer(2.0, 0.0, false, false),
        "travel inside the click slop moved the playhead"
    );
    assert_eq!(state.playhead.value, 40);

    // Past the slop the whole travel counts, the slop included: 12 px.
    assert!(state.drag_viewer(10.0, 0.0, false, false));
    assert_eq!(state.playhead.value, 46, "12 px is six frames");
    assert!(state.drag_viewer(-20.0, 0.0, false, false));
    assert_eq!(state.playhead.value, 36, "the way back is the same rate");
    assert!(state.drag_viewer(2.0, 0.0, true, false));
    assert_eq!(
        state.playhead.value, 46,
        "Shift moves ten frames per two pixels"
    );
    assert!(
        !state.drag_viewer(4.0, 0.0, false, true),
        "Ctrl moves a tenth of the rate, so 4 px is short of a frame"
    );
    assert!(state.drag_viewer(16.0, 0.0, false, true));
    assert_eq!(state.playhead.value, 47, "Ctrl: 20 px is one frame");

    assert!(state.release_viewer(), "the press was held");
    assert!(!state.clock_focused(), "a drag opened the field for typing");
    assert_eq!(state.viewer_scrub.press(), None);
    assert!(!state.release_viewer(), "a second release found a press");
}

#[test]
fn a_click_on_the_playhead_field_opens_it_and_a_press_while_open_starts_nothing() {
    let mut state = parked(40);
    state.press_clock(ClockSite::Viewer);
    state.drag_viewer(1.0, 0.0, false, false);
    assert!(state.release_viewer());
    assert!(state.clock_focused(), "a click did not open the field");
    assert_eq!(state.playhead.value, 40, "a click moved the playhead");

    state.press_clock(ClockSite::Viewer);
    assert_eq!(
        state.viewer_scrub.press(),
        None,
        "a press on the open field armed a drag over the caret"
    );
    assert!(!state.drag_viewer(50.0, 0.0, false, false));
    assert_eq!(state.playhead.value, 40);
}

/// Two fields type one draft, and the one drawn with it is the one pressed: a
/// press on the other drops the draft, so a value half typed in one never
/// appears in the other.
#[test]
fn a_click_opens_the_field_pressed_and_only_that_one() {
    let mut state = parked(40);
    state.press_clock(ClockSite::Timeline);
    assert!(state.release_viewer());
    assert!(
        state.clock_field(ClockSite::Timeline).0,
        "a click did not open the timeline field"
    );
    assert!(
        !state.clock_field(ClockSite::Viewer).0,
        "the Viewer's field opened with the timeline's"
    );

    state.clock_pop();
    state.press_clock(ClockSite::Viewer);
    assert!(
        !state.clock_focused(),
        "a press on the Viewer's field left the timeline's draft open"
    );
    assert!(state.release_viewer());
    let (open, text) = state.clock_field(ClockSite::Viewer);
    assert!(open, "a click on the Viewer's field did not open it");
    assert_eq!(
        text, state.clock_display,
        "the field opened on the dropped draft, not on the playhead"
    );
    assert!(!state.clock_field(ClockSite::Timeline).0);
}

#[test]
fn escape_puts_the_playhead_back_and_opens_nothing() {
    let mut state = parked(40);
    state.press_clock(ClockSite::Viewer);
    assert!(state.drag_viewer(30.0, 0.0, false, false));
    assert_ne!(state.playhead.value, 40);

    assert!(state.cancel_viewer_drag(), "the press was held");
    assert_eq!(state.playhead.value, 40, "Escape kept the scrubbed frame");
    assert!(!state.clock_focused(), "Escape opened the field");
    assert_eq!(state.viewer_scrub.press(), None);
    assert!(!state.cancel_viewer_drag(), "a second Escape found a press");
}

#[test]
fn travel_past_either_end_of_the_sequence_is_not_banked() {
    let mut state = parked(2);
    let end = state.store.sequence.duration_ticks;
    state.press_clock(ClockSite::Viewer);
    state.drag_viewer(-100.0, 0.0, false, false);
    assert_eq!(
        state.playhead.value, 0,
        "the playhead went before the start"
    );
    assert!(state.drag_viewer(4.0, 0.0, false, false));
    assert_eq!(
        state.playhead.value, 2,
        "the way back waited out banked travel"
    );

    state.drag_viewer(100_000.0, 0.0, false, false);
    assert_eq!(state.playhead.value, end, "the playhead went past the end");
    assert!(state.drag_viewer(-4.0, 0.0, false, false));
    assert_eq!(state.playhead.value, end - 2);
    state.release_viewer();
}

#[test]
fn the_ruler_spans_the_sequence_from_its_left_edge_to_its_right() {
    let mut state = parked(40);
    let end = state.store.sequence.duration_ticks;
    assert!(
        !state.press_viewer_ruler(150.0),
        "a ruler that was never painted moved the playhead"
    );
    assert_eq!(state.playhead.value, 40);
    state.release_viewer();

    state.record_viewer_ruler(100.0, 200.0);
    for (x, want) in [
        (100.0, 0),
        (300.0, end),
        (200.0, (end as f32 * 0.5).round() as i64),
        (-500.0, 0),
        (5_000.0, end),
    ] {
        state.press_viewer_ruler(x);
        assert_eq!(state.playhead.value, want, "a press at x = {x}");
        state.release_viewer();
    }

    state.set_playhead(40);
    state.press_viewer_ruler(100.0);
    assert!(state.drag_viewer(0.0, 300.0, false, false));
    assert_eq!(state.playhead.value, end, "the drag follows the pointer");
    assert!(state.cancel_viewer_drag());
    assert_eq!(state.playhead.value, 40, "Escape kept the scrubbed frame");

    state.press_viewer_ruler(300.0);
    assert!(state.release_viewer());
    assert!(
        !state.clock_focused(),
        "a press on the ruler opened the field"
    );
}

fn center(visual: &mut VisualTestContext, selector: &str) -> gpui::Point<gpui::Pixels> {
    visual
        .debug_bounds(intern(selector))
        .unwrap_or_else(|| panic!("the Viewer draws no {selector}"))
        .center()
}

fn locked(visual: &mut VisualTestContext) -> bool {
    visual.update(|window, _| window.is_pointer_locked())
}

/// The drawn fields, the Viewer's and the timeline header's: a drag locks the
/// pointer and reads relative travel, a release gives the pointer back and
/// leaves the field closed, and a click opens the field clicked and no other.
#[gpui::test]
fn the_drawn_playhead_fields_scrub_on_a_drag_and_open_on_a_click(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "viewer-scrub-field");
    let at = |visual: &mut VisualTestContext| visual.read(|app| state.read(app).playhead.value);
    for (selector, site, other) in [
        ("playhead-tc", ClockSite::Viewer, ClockSite::Timeline),
        ("timeline-tc", ClockSite::Timeline, ClockSite::Viewer),
    ] {
        drag_and_click(visual, &state, &at, selector, site, other);
    }
}

fn drag_and_click(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    at: &dyn Fn(&mut VisualTestContext) -> i64,
    selector: &str,
    site: ClockSite,
    other: ClockSite,
) {
    for end in ["release", "escape"] {
        change(visual, state, |state| state.set_playhead(30));
        let field = center(visual, selector);
        visual.simulate_mouse_down(field, MouseButton::Left, Modifiers::none());
        visual.simulate_mouse_move(
            field + point(px(2.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(
            !locked(visual),
            "{selector} {end}: a press inside the slop locked the pointer"
        );
        assert_eq!(
            at(visual),
            30,
            "{selector} {end}: a press inside the slop moved the playhead"
        );
        visual.simulate_mouse_move(
            field + point(px(20.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert!(
            locked(visual),
            "{selector} {end}: the drag left the pointer free"
        );
        assert_eq!(
            at(visual),
            40,
            "{selector} {end}: 20 px of travel is ten frames"
        );
        visual.simulate_locked_mouse_move(
            point(px(-10.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert_eq!(
            at(visual),
            35,
            "{selector} {end}: a locked pointer's travel was lost"
        );
        if end == "release" {
            visual.simulate_mouse_up(field, MouseButton::Left, Modifiers::none());
        } else {
            visual.simulate_keystrokes("escape");
        }
        visual.run_until_parked();
        let (playhead, typing, pressed) = visual.read(|app| {
            let state = state.read(app);
            (
                state.playhead.value,
                state.clock_focused(),
                state.viewer_scrub.press().is_some(),
            )
        });
        assert!(
            !locked(visual),
            "{selector} {end}: the pointer stayed locked"
        );
        assert!(!pressed, "{selector} {end}: the press outlived the gesture");
        assert!(
            !typing,
            "{selector} {end}: the drag opened the field for typing"
        );
        let want = if end == "release" { 35 } else { 30 };
        assert_eq!(
            playhead, want,
            "{selector} {end}: where the drag left the playhead"
        );
        if end == "escape" {
            // Escape ended the drag; the button is still down until here.
            visual.simulate_mouse_up(field, MouseButton::Left, Modifiers::none());
            visual.run_until_parked();
        }
    }

    let field = center(visual, selector);
    visual.simulate_click(field, Modifiers::none());
    visual.run_until_parked();
    let (open, other_open) = visual.read(|app| {
        let state = state.read(app);
        (state.clock_field(site).0, state.clock_field(other).0)
    });
    assert!(
        open,
        "{selector}: a click on the drawn field did not open it"
    );
    assert!(
        !other_open,
        "{selector}: a click opened the other field too"
    );
    change(visual, state, |state| state.focus_clock(false));
}

/// The drawn ruler moves the tab in front, and the drag goes on when the
/// pointer leaves the 12 px strip.
#[gpui::test]
fn the_drawn_ruler_moves_the_tab_in_front_and_keeps_scrubbing_off_the_strip(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, media) = shell_with_take(cx, "viewer-scrub-ruler");
    let end = visual.read(|app| state.read(app).store.sequence.duration_ticks);
    for side in ["Program", "Source"] {
        if side == "Source" {
            change(visual, &state, |state| {
                assert!(state.open_source(media.raw()), "the Source tab opens");
            });
        }
        change(visual, &state, |state| {
            state.set_playhead(30);
            if let Some(view) = state.sources.focused_mut() {
                view.tick = 30;
            }
        });
        let ruler = visual
            .debug_bounds(intern("viewer-ruler"))
            .unwrap_or_else(|| panic!("{side}: the Viewer draws no mini ruler"));
        let left = ruler.origin + point(px(0.5), ruler.size.height / 2.0);
        let right = left + point(ruler.size.width - px(1.0), px(0.0));
        let frame =
            |visual: &mut VisualTestContext| visual.read(|app| state.read(app).ruler_frame());
        let last = visual.read(|app| {
            let state = state.read(app);
            state
                .sources
                .focused()
                .map_or(end, |view| view.duration_ticks - 1)
        });

        visual.simulate_mouse_down(left, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        assert_eq!(frame(visual), 0, "{side}: a press at the left edge");
        // Off the strip, over the control row above its right edge.
        visual.simulate_mouse_move(
            right - point(px(0.0), px(20.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        assert_eq!(
            frame(visual),
            last,
            "{side}: the drag stopped at the strip's edge"
        );
        visual.simulate_mouse_up(right, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        let (playhead, pressed) = visual.read(|app| {
            let state = state.read(app);
            (state.playhead.value, state.viewer_scrub.press().is_some())
        });
        assert!(!pressed, "{side}: the press outlived the release");
        if side == "Source" {
            assert_eq!(playhead, 30, "the Source tab's ruler moved the sequence");
        }
    }
}
