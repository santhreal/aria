//! Contracts for the colour wheels and the Color panel's sections.
//!
//! The class closed here is "a wheel gesture lands somewhere other than where
//! the pointer put it": a press read against a frame other than the wheel's
//! own, a drag that compounds or leaves one undo step per move, an Escape that
//! leaves the dragged balance behind, a press that moves the channels' level,
//! a rim that drives a channel past its reach, and a reset that takes four
//! undo steps. Every wheel is driven through every case.
//!
//! The drawn case goes through the shell: a section header opens the wheels, a
//! press on the drawn Lift wheel, a move off it, and the release or Escape are
//! routed by the shell as a user's would be. A release outside the window ends
//! the drag as one over it does.
//!
//! What it does not catch: whether the hue ring's colours match the direction
//! the grade moves on screen, which needs pixels.

use std::path::PathBuf;

use aria_models::{ClipId, ClipParam};
use gpui::{Modifiers, MouseButton, point, px};

use super::AppState;
use super::grade_controls::{
    COLOR_SECTIONS, WheelControl, pointer_to_wheel_offset, rgb_deltas_to_wheel_offset,
    wheel_offset_to_rgb_deltas,
};
use super::premiere_tests::with_clip;
use crate::dock::PANEL_COLOR;

/// A wheel drawn 72 px wide, its top-left corner away from the window's.
const FRAME: [f32; 4] = [400.0, 300.0, 72.0, 72.0];
const CENTER: [f32; 2] = [436.0, 336.0];
const RADIUS: f32 = 36.0;

fn at(dx: f32, dy: f32) -> [f32; 2] {
    [CENTER[0] + dx, CENTER[1] + dy]
}

fn channels(state: &AppState, control: WheelControl) -> [f32; 3] {
    control.rgb_params().map(|param| {
        state
            .param_scalar(ClipParam::Look { param }, None)
            .expect("a grade channel")
    })
}

fn set_channels(state: &mut AppState, control: WheelControl, values: [f32; 3]) {
    for (param, value) in control.rgb_params().into_iter().zip(values) {
        state
            .set_param_scalar(ClipParam::Look { param }, None, value)
            .expect("set a channel");
    }
}

/// How far the rim moves a channel, as the Color documentation states it: a
/// quarter for Lift, a half for Gamma and Gain.
fn stated_reach(control: WheelControl) -> f32 {
    match control {
        WheelControl::Lift => 0.25,
        WheelControl::Gamma | WheelControl::Gain => 0.5,
    }
}

/// The channels a wheel holding `offset` around `mean` writes.
fn expected(control: WheelControl, mean: f32, offset: [f32; 2]) -> [f32; 3] {
    wheel_offset_to_rgb_deltas(offset).map(|delta| mean + stated_reach(control) * delta)
}

fn near(got: [f32; 3], want: [f32; 3]) -> bool {
    got.iter()
        .zip(want)
        .all(|(got, want)| (got - want).abs() < 1e-4)
}

fn assert_channels(state: &AppState, control: WheelControl, want: [f32; 3], what: &str) {
    let got = channels(state, control);
    assert!(
        near(got, want),
        "{control:?}: {what}: read {got:?}, wanted {want:?}"
    );
}

#[test]
fn the_pointer_maps_into_the_unit_disc_and_holds_at_the_rim() {
    let center = [100.0, 100.0];
    let cases = [
        ([100.0, 100.0], [0.0, 0.0]),
        ([125.0, 100.0], [0.5, 0.0]),
        ([100.0, 150.0], [0.0, 1.0]),
        ([200.0, 100.0], [1.0, 0.0]),
        ([100.0, -900.0], [0.0, -1.0]),
    ];
    for (pointer, want) in cases {
        let got = pointer_to_wheel_offset(center, 50.0, pointer);
        assert!(
            (got[0] - want[0]).abs() < 1e-5 && (got[1] - want[1]).abs() < 1e-5,
            "{pointer:?} read {got:?}, wanted {want:?}"
        );
    }
    let diagonal = pointer_to_wheel_offset(center, 50.0, [300.0, 300.0]);
    let length = diagonal[0].hypot(diagonal[1]);
    assert!((length - 1.0).abs() < 1e-5, "past the rim read {length}");
    assert_eq!(
        pointer_to_wheel_offset(center, 0.0, [130.0, 100.0]),
        [0.0, 0.0],
        "a wheel that has not painted reads the centre"
    );
}

/// Red at 0°, green at 120°, blue at 240°; the deltas sum to zero and read
/// back as the offset they came from.
#[test]
fn offsets_and_channel_deltas_round_trip_and_sum_to_zero() {
    let offsets = [
        [0.0, 0.0],
        [1.0, 0.0],
        [0.0, 1.0],
        [-0.5, 0.866],
        [-0.5, -0.866],
        [0.3, -0.7],
    ];
    for offset in offsets {
        let deltas = wheel_offset_to_rgb_deltas(offset);
        assert!(
            deltas.iter().sum::<f32>().abs() < 1e-5,
            "{offset:?}: {deltas:?}"
        );
        let back = rgb_deltas_to_wheel_offset(deltas);
        assert!(
            (back[0] - offset[0]).abs() < 1e-3 && (back[1] - offset[1]).abs() < 1e-3,
            "{offset:?} read back as {back:?}"
        );
    }
    let green = wheel_offset_to_rgb_deltas([-0.5, 0.866_025_4]);
    assert!(
        (green[1] - 1.0).abs() < 1e-5,
        "120° is not green: {green:?}"
    );
}

/// A press at the centre of the wheel's own frame writes nothing, and one
/// half way to its right rim writes half the reach toward red. Read against
/// the window's corner instead, both presses land on the rim.
#[test]
fn a_press_reads_the_pointer_against_the_wheels_own_frame() {
    for control in WheelControl::ALL {
        let (mut state, _) = with_clip();
        let neutral = [control.neutral_master(); 3];
        let depth = state.history_depth();
        state.press_wheel(control, FRAME, at(0.0, 0.0));
        assert_channels(&state, control, neutral, "a press at the centre");
        assert!(state.release_wheel());
        assert_eq!(
            state.history_depth(),
            depth,
            "{control:?}: a press that changed nothing left an undo step"
        );

        assert!(state.press_wheel(control, FRAME, at(RADIUS * 0.5, 0.0)));
        assert_channels(
            &state,
            control,
            expected(control, control.neutral_master(), [0.5, 0.0]),
            "a press half way to the right rim",
        );
        let offset = control.current_offset(&state);
        assert!(
            (offset[0] - 0.5).abs() < 1e-4 && offset[1].abs() < 1e-4,
            "{control:?}: the puck reads {offset:?}"
        );
        assert!(state.release_wheel());
    }
}

/// Every move is measured from the press, the whole drag is one undo step,
/// and undo takes it back to where the press found the grade.
#[test]
fn a_drag_is_one_undo_step_from_the_press() {
    for control in WheelControl::ALL {
        let (mut state, _) = with_clip();
        let neutral = [control.neutral_master(); 3];
        let depth = state.history_depth();
        state.press_wheel(control, FRAME, at(RADIUS, 0.0));
        assert!(state.drag_wheel(at(0.0, RADIUS)));
        assert!(state.drag_wheel(at(-RADIUS * 0.4, RADIUS * 0.3)));
        assert!(
            !state.drag_wheel(at(-RADIUS * 0.4, RADIUS * 0.3)),
            "{control:?}: a move to the same place changed the grade"
        );
        assert_channels(
            &state,
            control,
            expected(control, control.neutral_master(), [-0.4, 0.3]),
            "the last move compounded onto the earlier ones",
        );
        assert_eq!(state.wheel_held(), Some(control));
        assert!(state.release_wheel());
        assert_eq!(state.wheel_held(), None, "{control:?}: the release held on");
        assert!(!state.release_wheel(), "{control:?}: nothing is held");
        assert_eq!(
            state.history_depth(),
            depth + 1,
            "{control:?}: the drag is not one undo step"
        );
        assert!(state.undo(), "{control:?}");
        assert_channels(&state, control, neutral, "undo after the drag");
    }
}

/// Escape restores the channels the press found, whatever they were, and
/// leaves no undo step.
#[test]
fn escape_puts_the_grade_back_where_the_press_found_it() {
    for control in WheelControl::ALL {
        let (mut state, _) = with_clip();
        let base = control.neutral_master();
        let before = [base + 0.1, base - 0.05, base + 0.02];
        set_channels(&mut state, control, before);
        let depth = state.history_depth();
        state.press_wheel(control, FRAME, at(RADIUS, 0.0));
        state.drag_wheel(at(0.0, -RADIUS));
        assert!(state.cancel_wheel_drag(), "{control:?}");
        assert!(!state.cancel_wheel_drag(), "{control:?}: nothing is held");
        assert_channels(&state, control, before, "Escape");
        assert_eq!(state.history_depth(), depth, "{control:?}");
    }
}

/// A wheel moves the balance and leaves the channels' mean where the press
/// found it, at any position, so a press never brightens or darkens.
#[test]
fn a_drag_keeps_the_level_the_press_found() {
    for control in WheelControl::ALL {
        let (mut state, _) = with_clip();
        let mean = control.neutral_master() + 0.2;
        set_channels(&mut state, control, [mean + 0.15, mean - 0.15, mean]);
        state.press_wheel(control, FRAME, at(0.0, 0.0));
        for pointer in [
            at(RADIUS, 0.0),
            at(-RADIUS * 0.7, RADIUS * 0.2),
            at(0.0, 0.0),
        ] {
            state.drag_wheel(pointer);
            let level = channels(&state, control).iter().sum::<f32>() / 3.0;
            assert!(
                (level - mean).abs() < 1e-4,
                "{control:?} at {pointer:?}: the level moved to {level}, from {mean}"
            );
        }
        assert_channels(&state, control, [mean; 3], "the centre around the level");
        assert!(state.release_wheel());
    }
}

/// Past the rim the offset holds at the rim: no channel moves further from
/// the level than the wheel's reach, and a gamma or gain channel at unity
/// never reaches zero.
#[test]
fn the_rim_bounds_every_channel_by_the_reach() {
    let far = [
        at(10_000.0, 0.0),
        at(-10_000.0, 10_000.0),
        at(0.0, -10_000.0),
    ];
    for control in WheelControl::ALL {
        let (mut state, _) = with_clip();
        let level = control.neutral_master();
        state.press_wheel(control, FRAME, at(0.0, 0.0));
        for pointer in far {
            state.drag_wheel(pointer);
            for value in channels(&state, control) {
                assert!(
                    (value - level).abs() <= stated_reach(control) + 1e-4,
                    "{control:?} at {pointer:?}: a channel read {value}"
                );
                if control != WheelControl::Lift {
                    assert!(value >= 0.5 - 1e-4, "{control:?}: a channel read {value}");
                }
            }
            let offset = control.current_offset(&state);
            assert!(
                (offset[0].hypot(offset[1]) - 1.0).abs() < 1e-3,
                "{control:?} at {pointer:?}: the puck left the rim, {offset:?}"
            );
        }
        assert!(state.release_wheel());
    }
}

/// Reset puts the channels and the master back at neutral as one undo step,
/// and a reset at neutral leaves none.
#[test]
fn reset_returns_the_wheel_to_neutral_as_one_undo_step() {
    for control in WheelControl::ALL {
        let (mut state, _) = with_clip();
        let base = control.neutral_master();
        let graded = [base + 0.1, base - 0.1, base + 0.05];
        set_channels(&mut state, control, graded);
        let master = ClipParam::Look {
            param: control.master_param(),
        };
        state
            .set_param_scalar(master, None, base + 0.3)
            .expect("set the master");
        let depth = state.history_depth();
        control.reset(&mut state).expect("reset");
        assert_channels(&state, control, [base; 3], "after reset");
        assert!((control.current_master(&state) - base).abs() < 1e-4);
        assert_eq!(state.history_depth(), depth + 1, "{control:?}");
        control.reset(&mut state).expect("a second reset");
        assert_eq!(
            state.history_depth(),
            depth + 1,
            "{control:?}: a reset at neutral left an undo step"
        );
        assert!(state.undo());
        assert_channels(&state, control, graded, "undo after reset");
        assert!((control.current_master(&state) - (base + 0.3)).abs() < 1e-4);
    }
}

/// Basic Correction starts open and every other section closed; a toggle
/// opens a closed section and closes an open one, and touches no other. A
/// section past the open-state capacity has no state and moves none.
#[test]
fn a_section_toggle_flips_that_section_alone() {
    let (mut state, _) = with_clip();
    let open = |state: &AppState| -> Vec<bool> {
        (0..COLOR_SECTIONS)
            .map(|index| state.color_section_open(index))
            .collect()
    };
    let mut want: Vec<bool> = (0..COLOR_SECTIONS).map(|index| index == 0).collect();
    assert_eq!(open(&state), want);
    for index in 0..COLOR_SECTIONS {
        state.toggle_color_section(index);
        want[index] = !want[index];
        assert_eq!(open(&state), want, "toggling section {index}");
    }
    for past in [COLOR_SECTIONS, usize::MAX] {
        state.toggle_color_section(past);
        assert_eq!(open(&state), want, "section {past} moved one");
        assert!(!state.color_section_open(past), "section {past} reads open");
    }
}

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn first_clip(state: &AppState) -> ClipId {
    state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .next()
        .expect("one clip")
}

/// The Color panel draws the wheels once their section is open. A press on
/// the drawn Lift wheel writes the offset under the pointer, a move off the
/// wheel keeps dragging it, and the release, over the window or outside it,
/// keeps one undo step while Escape keeps none.
#[gpui::test]
fn a_press_on_the_drawn_wheel_drags_through_the_shell(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "grade-wheel");
    let lift = WheelControl::Lift;
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.focus_dock_panel(PANEL_COLOR);
            let media = state.import_media(fixture()).expect("import fixture");
            state
                .drop_media_on_timeline(media, None, 0)
                .expect("place clip");
            let clip = first_clip(state);
            state.select_clip(clip);
            cx.notify();
        })
    });
    crate::test_support::refresh_panels(visual);
    assert!(
        visual.debug_bounds("color-wheel-lift").is_none(),
        "the wheels drew with their section closed"
    );
    let click_header = |visual: &mut gpui::VisualTestContext, index: usize| -> bool {
        let selector = crate::interned::intern(&format!("color-section-{index}"));
        let Some(header) = visual.debug_bounds(selector) else {
            return false;
        };
        visual.simulate_click(header.center(), Modifiers::none());
        crate::test_support::refresh_panels(visual);
        true
    };
    // Basic Correction closes, so the wheels open near the top of the panel.
    // Each later header opens its section until the wheels draw; a section
    // without them closes again.
    assert!(click_header(visual, 0), "section 0 has no header");
    let wheels = (1..COLOR_SECTIONS)
        .find(|&index| {
            assert!(click_header(visual, index), "no section draws the wheels");
            let drawn = visual.debug_bounds("color-wheel-lift").is_some();
            if !drawn {
                click_header(visual, index);
            }
            drawn
        })
        .expect("no section draws the wheels");
    assert!(
        visual.read(|app| state.read(app).color_section_open(wheels)),
        "a click on the header left the wheels closed"
    );
    let wheel = visual
        .debug_bounds("color-wheel-lift")
        .expect("the Lift wheel is not drawn with its section open");
    let center = wheel.center();
    let radius = f32::from(wheel.size.width) * 0.5;
    let read =
        |visual: &mut gpui::VisualTestContext| visual.read(|app| channels(state.read(app), lift));
    let depth = visual.read(|app| state.read(app).history_depth());
    let neutral = [lift.neutral_master(); 3];

    for end in ["release", "outside", "escape"] {
        let half = point(center.x + px(radius * 0.5), center.y);
        visual.simulate_mouse_down(half, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        let want = expected(lift, lift.neutral_master(), [0.5, 0.0]);
        let got = read(visual);
        assert!(
            near(got, want),
            "{end}: a press half way right read {got:?}, wanted {want:?}"
        );
        // Below the wheel, past its rim: the drag follows the pointer off it.
        let below = point(center.x, center.y + px(radius * 2.0));
        visual.simulate_mouse_move(below, MouseButton::Left, Modifiers::none());
        visual.run_until_parked();
        let want = expected(lift, lift.neutral_master(), [0.0, 1.0]);
        let got = read(visual);
        assert!(
            near(got, want),
            "{end}: a move off the wheel read {got:?}, wanted {want:?}"
        );
        match end {
            "release" => visual.simulate_mouse_up(below, MouseButton::Left, Modifiers::none()),
            // Left of the window: the release lands on nothing the shell drew.
            "outside" => visual.simulate_mouse_up(
                point(px(-10.), below.y),
                MouseButton::Left,
                Modifiers::none(),
            ),
            _ => visual.simulate_keystrokes("escape"),
        }
        visual.run_until_parked();
        assert_eq!(
            visual.read(|app| state.read(app).wheel_held()),
            None,
            "{end}: the press outlived the gesture"
        );
        let after = visual.read(|app| state.read(app).history_depth());
        if end != "escape" {
            assert_eq!(after, depth + 1, "{end}: the drag is not one undo step");
            assert!(near(read(visual), want), "the release lost the drag");
            visual.update(|_, app| {
                state.update(app, |state, cx| {
                    assert!(state.undo());
                    cx.notify();
                })
            });
            visual.run_until_parked();
        } else {
            assert_eq!(after, depth, "Escape left an undo step");
        }
        let got = read(visual);
        assert!(
            near(got, neutral),
            "{end}: the grade did not return: {got:?}"
        );
    }
}
