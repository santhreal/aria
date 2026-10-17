//! Clip parenting from the editor: what the Parent row offers, the pick whip,
//! and a child moved on the canvas.
//!
//! Closes the class where the row offers a parent the model would refuse (the
//! clip itself, its own child), a whip lands for a clip that is no longer
//! selected, or a drag on the canvas moves a child along its parent's turned
//! axes rather than with the pointer, in the outline or in the drafted value.
//!
//! Not covered: how the row is drawn, and the timeline press and release that
//! call `land_parent_whip`, which the headed drive shows.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipId, ClipTransform, ParentKeep};

use crate::app::AppState;
use crate::app::canvas_overlay::Rect;
use crate::app::fixtures::{parented_pair, stacked_pair};
use crate::app::sent_census::Watch;

/// Turn `clip` a quarter and move it right of centre, so its frame differs
/// from the canvas on every axis.
fn turn(state: &mut AppState, clip: ClipId) {
    state
        .apply_command(Command::Edit(EditCommand::SetClipTransform {
            clip,
            transform: ClipTransform {
                position: [0.25, 0.0],
                rotation: 90.0,
                ..ClipTransform::IDENTITY
            },
            at: None,
        }))
        .expect("turn the clip");
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-4 && (a[1] - b[1]).abs() < 1e-4
}

#[test]
fn the_parent_row_offers_the_overlapping_clip_and_not_the_clip_itself() {
    let (state, _, upper) = stacked_pair();
    let choices = state.selected_parent_choices().expect("a selected clip");
    assert_eq!(choices.current, None);
    assert_eq!(&*choices.eligible, &[upper]);
}

#[test]
fn a_parent_is_not_offered_its_own_child() {
    let (mut state, child) = parented_pair();
    let choices = state
        .selected_parent_choices()
        .expect("the child is selected");
    let parent = choices.current.expect("the child has a parent");
    assert_eq!(&*choices.eligible, &[parent], "the parent stays offered");

    state.select_clip(parent);
    let choices = state
        .selected_parent_choices()
        .expect("the parent is selected");
    assert_eq!(choices.current, None);
    assert!(
        !choices.eligible.contains(&child),
        "the parent was offered its own child: {:?}",
        choices.eligible
    );
}

#[test]
fn the_pick_whip_parents_the_clip_it_was_armed_for() {
    for snap in [false, true] {
        let (mut state, lower, upper) = stacked_pair();
        turn(&mut state, upper);
        state.select_clip(lower);
        state.toggle_parent_whip();
        assert_eq!(state.parent_whip(), Some(lower), "snap {snap}");

        state.land_parent_whip(upper, snap).expect("the whip lands");
        assert_eq!(state.parent_whip(), None, "snap {snap}: the whip is down");
        let model = state.selected_model().expect("the child stays selected");
        assert_eq!(model.id, lower, "snap {snap}");
        assert_eq!(model.parent, Some(upper), "snap {snap}");
        let own = model.transform.sample(0).position;
        let frame = state
            .selected_parent_frame()
            .expect("the parent covers the playhead");
        if snap {
            assert_eq!(own, [0.0, 0.0], "a snap sits on the parent's anchor");
        } else {
            assert!(
                close(frame.place(own), [0.0, 0.0]),
                "the clip moved on the canvas: drawn at {:?}",
                frame.place(own)
            );
        }
    }
}

#[test]
fn a_whip_that_lands_nowhere_new_sends_nothing() {
    // Armed for one clip and then a different selection; armed and landed on
    // its own clip; armed and pressed again.
    type Step = fn(&mut AppState, ClipId, ClipId);
    let cases: [(&str, Step); 3] = [
        ("selection moved", |state, lower, upper| {
            state.toggle_parent_whip();
            state.select_clip(upper);
            assert_eq!(state.parent_whip(), None);
            state.land_parent_whip(lower, false).expect("inert");
        }),
        ("landed on itself", |state, lower, _| {
            state.toggle_parent_whip();
            state.land_parent_whip(lower, false).expect("inert");
            assert_eq!(state.parent_whip(), None);
        }),
        ("pressed again", |state, _, upper| {
            state.toggle_parent_whip();
            state.toggle_parent_whip();
            assert_eq!(state.parent_whip(), None);
            state.land_parent_whip(upper, false).expect("inert");
        }),
    ];
    for (label, step) in cases {
        let (mut state, lower, upper) = stacked_pair();
        let watch = Watch::on(&state);
        step(&mut state, lower, upper);
        assert!(watch.take().is_empty(), "{label}: the whip sent an edit");
    }
}

/// A Viewer arrow moves a child of a turned parent right on the canvas, not
/// along the parent's turned axis.
#[test]
fn an_arrow_moves_a_child_right_inside_a_turned_parent() {
    let (mut state, lower, upper) = stacked_pair();
    turn(&mut state, upper);
    state
        .set_clip_parent(lower, Some(upper), ParentKeep::Place)
        .expect("parent the clip");
    state.select_clip(lower);
    let [width, _] = state.canvas_size();
    let start = state.selected_pose().expect("a selected clip");
    state.viewer_arrow(10.0, 0.0).expect("nudge right");
    let after = state.selected_pose().expect("a selected clip");
    let frame = state
        .selected_parent_frame()
        .expect("the parent covers the playhead");
    let from = frame.place([start.position_x, start.position_y]);
    let to = frame.place([after.position_x, after.position_y]);
    assert!(
        close(
            [to[0] - from[0], to[1] - from[1]],
            [10.0 / width as f32, 0.0]
        ),
        "the arrow moved the clip by {:?} on the canvas",
        [to[0] - from[0], to[1] - from[1]]
    );
}

/// A child of a parent turned a quarter is dragged right by a quarter of the
/// canvas. On the canvas it moves right; its own position, held in the
/// parent's frame, moves along the parent's axes.
#[test]
fn a_child_follows_the_pointer_inside_a_turned_parent() {
    let (mut state, lower, upper) = stacked_pair();
    turn(&mut state, upper);
    state
        .set_clip_parent(lower, Some(upper), ParentKeep::Place)
        .expect("parent the clip");
    state.select_clip(lower);
    let container = Rect::new(0.0, 0.0, 400.0, 200.0);
    state.record_overlay_frame(container);
    let before = state.overlay_frame.layer.expect("the child draws a layer");
    let canvas = state.overlay_frame.canvas.expect("a canvas");
    let start = state.overlay_transform().expect("a selected clip");

    state.begin_transform_drag(None, [200.0, 100.0]);
    state.drag_overlay_to([300.0, 100.0]);
    let draft = state.overlay_transform().expect("a draft mid-drag");
    let frame = state
        .selected_parent_frame()
        .expect("the parent covers the playhead");
    let from = frame.place([start.position_x, start.position_y]);
    let to = frame.place([draft.position_x, draft.position_y]);
    assert!(
        close([to[0] - from[0], to[1] - from[1]], [100.0 / canvas.w, 0.0]),
        "the drafted position moves the clip by {:?} on the canvas",
        [to[0] - from[0], to[1] - from[1]]
    );

    state.record_overlay_frame(container);
    let after = state.overlay_frame.layer.expect("the child draws a layer");
    let moved = [
        after.x + after.w / 2.0 - (before.x + before.w / 2.0),
        after.y + after.h / 2.0 - (before.y + before.h / 2.0),
    ];
    assert!(
        (moved[0] - 100.0).abs() < 0.5 && moved[1].abs() < 0.5,
        "the outline moved by {moved:?}, not with the pointer"
    );
}
