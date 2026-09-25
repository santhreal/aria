//! Driving the speed graph's own pointer gestures.
//!
//! # Why
//!
//! Four rules lived in four gpui listeners on the ramp canvas: a press grabs a
//! nearby point or adds one, a move is inert unless something is grabbed, a
//! release that moved nothing writes nothing, and a right press drops the point
//! under it or does nothing. Nothing could enumerate them, so the inert cases
//! read like ordinary early returns and a canvas wired to the wrong call read
//! like one wired to the right call.
//!
//! The registry is [`ramp_gesture`](crate::surfaces::ramp_gesture). Each row is
//! pressed against a graph that offers it: a press against an empty graph,
//! because that is the press that adds a point, and the drag and release against
//! a graph with a point already grabbed, because a pointer move over a graph with
//! nothing grabbed is not a drag.
//!
//! The graph rectangle is recorded the way the panel records it during paint. A
//! census that skipped it would drive a graph that had never been drawn, where
//! every gesture is inert and every assertion passes.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::sent_census::Watch;
use crate::surfaces::ramp_gesture::{self, Gesture};
use aria_models::SPEED_CURVE_SCALE;

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "ramp_gesture.rs";

/// The graph rectangle, in the window units a pointer arrives in.
const FRAME: [f32; 4] = [0.0, 0.0, 100.0, 100.0];

/// A clip that can carry a ramp, with the graph measured as if it had been drawn.
fn graphed() -> AppState {
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    assert!(
        state.selected_can_ramp(),
        "the fixture's clip refuses a ramp, so no gesture on it means anything"
    );
    state.record_ramp_frame(FRAME);
    state
}

/// A graph with a point at its middle, and the pointer position that grabs it.
fn seeded() -> (AppState, [f32; 2]) {
    let mut state = graphed();
    state
        .set_selected_ramp_point(SPEED_CURVE_SCALE / 2, 2.0)
        .expect("seed a point to grab");
    (state, [50.0, 50.0])
}

/// Every gesture the graph offers. One per arm of `Gesture::what`, which
/// `every_gesture_the_graph_offers_is_pressed` holds this list to.
fn gestures() -> Vec<Gesture> {
    vec![
        Gesture::Press { at: [50.0, 50.0] },
        Gesture::Drag { at: [60.0, 40.0] },
        Gesture::Release,
        Gesture::Drop { at: [50.0, 50.0] },
    ]
}

/// Run one gesture, refusing to pass a refusal off as a gesture.
fn run(state: &mut AppState, gesture: Gesture) -> bool {
    ramp_gesture::apply(state, gesture)
        .unwrap_or_else(|err| panic!("`{}` was refused: {err}", gesture.what()))
}

/// A graph dressed for `gesture`, and the gesture itself.
///
/// A drag and a release both need a point already grabbed, which is what the
/// press before them does; the other two are pressed on a graph that has only
/// been drawn.
fn staged(gesture: Gesture) -> AppState {
    match gesture {
        Gesture::Press { .. } => graphed(),
        Gesture::Drag { .. } | Gesture::Release => {
            let (mut state, grab) = seeded();
            run(&mut state, Gesture::Press { at: grab });
            assert!(
                state.ramp_dragging().is_some(),
                "the press did not grab the seeded point, so this is not a drag"
            );
            state
        }
        Gesture::Drop { .. } => seeded().0,
    }
}

/// Every command a graph gesture asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    // The count comes off a list that is dropped before the loop: a census
    // holding several projects open at once holds several compositor devices.
    let count = gestures().len();
    for index in 0..count {
        let gesture = gestures().swap_remove(index);
        let mut state = staged(gesture);
        let watch = Watch::on(&state);
        run(&mut state, gesture);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_gesture_the_graph_offers_is_pressed() {
    // Fail by default on a new arm: `what` is a total match, so the compiler
    // forces prose for a fifth gesture and this forces a press.
    let pressed: BTreeSet<String> = gestures()
        .into_iter()
        .map(|gesture| gesture.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "the graph offers these gestures and nothing presses them: {missing:?}"
    );
}

#[test]
fn a_press_on_an_empty_graph_puts_a_point_down() {
    let mut state = graphed();
    assert!(
        state.selected_ramp_curve().is_empty(),
        "the fixture's clip already carries a ramp"
    );
    let watch = Watch::on(&state);
    assert!(run(&mut state, Gesture::Press { at: [50.0, 50.0] }));
    assert_eq!(watch.take(), vec!["Set speed curve"]);
    assert!(
        !state.selected_ramp_curve().is_empty(),
        "the press wrote no ramp"
    );
}

#[test]
fn a_press_on_a_point_grabs_it_instead_of_adding_another() {
    let (mut state, grab) = seeded();
    let points = state.selected_ramp_curve().len();
    let watch = Watch::on(&state);
    assert!(run(&mut state, Gesture::Press { at: grab }));
    assert!(
        state.ramp_dragging().is_some(),
        "a press on a point did not grab it"
    );
    assert_eq!(
        watch.take(),
        Vec::<&'static str>::new(),
        "grabbing a point is not an edit"
    );
    assert_eq!(
        state.selected_ramp_curve().len(),
        points,
        "the press added a point on top of the one it grabbed"
    );
}

#[test]
fn a_pointer_move_with_nothing_grabbed_does_nothing() {
    let mut state = graphed();
    let watch = Watch::on(&state);
    assert!(
        !run(&mut state, Gesture::Drag { at: [60.0, 40.0] }),
        "a move over an untouched graph asked for a repaint"
    );
    assert_eq!(watch.take(), Vec::<&'static str>::new());
    assert!(
        state.selected_ramp_curve().is_empty(),
        "a move over an untouched graph wrote a point"
    );
}

#[test]
fn a_drag_is_a_draft_and_the_release_is_the_edit() {
    let mut state = staged(Gesture::Drag { at: [60.0, 40.0] });
    let watch = Watch::on(&state);
    assert!(run(&mut state, Gesture::Drag { at: [60.0, 40.0] }));
    assert_eq!(
        watch.take(),
        Vec::<&'static str>::new(),
        "a pointer move reached the project"
    );
    assert!(run(&mut state, Gesture::Release));
    assert!(
        !watch.take().is_empty(),
        "the release wrote nothing, so the drag was lost"
    );
    assert!(
        state.ramp_dragging().is_none(),
        "the release left the point grabbed"
    );
}

#[test]
fn a_release_with_nothing_grabbed_writes_nothing() {
    let mut state = graphed();
    let watch = Watch::on(&state);
    assert!(!run(&mut state, Gesture::Release));
    assert_eq!(watch.take(), Vec::<&'static str>::new());
}

#[test]
fn a_right_press_drops_the_point_under_it_and_nothing_else() {
    let (mut state, at) = seeded();
    let before = state.selected_ramp_curve().len();
    assert!(before > 2, "the seeded ramp has nothing to spare");
    let watch = Watch::on(&state);
    assert!(run(&mut state, Gesture::Drop { at }));
    assert_eq!(watch.take(), vec!["Set speed curve"]);
    assert_eq!(
        state.selected_ramp_curve().len(),
        before - 1,
        "the right press removed something other than one point"
    );

    // A right press where no point sits is inert, rather than removing the
    // nearest one.
    let watch = Watch::on(&state);
    assert!(!run(&mut state, Gesture::Drop { at: [7.0, 90.0] }));
    assert_eq!(watch.take(), Vec::<&'static str>::new());
}

#[test]
fn a_gesture_over_a_graph_that_was_never_drawn_is_inert() {
    // `ramp_at_pointer` answers nothing until the canvas has measured itself, so
    // every gesture has to fall through rather than mapping the pointer against
    // a rectangle it does not have.
    let (mut state, _) = crate::app::premiere_tests::with_clip();
    let watch = Watch::on(&state);
    for gesture in gestures() {
        assert!(
            !run(&mut state, gesture),
            "`{}` acted on a graph that was never drawn",
            gesture.what()
        );
    }
    assert_eq!(watch.take(), Vec::<&'static str>::new());
}

#[test]
fn the_census_reaches_the_command_the_graph_claims() {
    assert!(
        dispatched().contains("Set speed curve"),
        "the graph's gestures sent {:?}",
        dispatched()
    );
}

#[test]
fn the_speed_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "inspector/tabs/speed.rs");
}
