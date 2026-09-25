//! Driving the preview's on-canvas handles: the move, the four scale corners,
//! the eight crop handles and the three mask handles.
//!
//! # Why
//!
//! `Transform` claimed the preview's handles and nothing pressed them. Which
//! handle set is live is the overlay mode's answer, and a crop corner and a
//! scale corner land on the same pixel when the crop is full, so a corner wired
//! to the wrong verb is invisible by inspection: it draws in the right place and
//! edits the wrong thing. The registry is
//! [`overlay_gesture`](crate::surfaces::overlay_gesture) and this drags every
//! handle it names.
//!
//! A drag is three steps and the census makes all three: the press, one move,
//! and the release. Only the release edits, so a census that pressed and
//! released without moving would measure nothing at all, which is its own test
//! below.
//!
//! The canvas geometry is stated rather than rendered: `framed` puts the picture
//! at a known place, because a gate has no window to paint one in.

use std::collections::BTreeSet;

use aria_models::MaskKind;

use crate::app::AppState;
use crate::app::canvas_overlay::{CropHandle, MaskHandle, TransformHandle};
use crate::app::sent_census::Watch;
use crate::app::transform_handle_tests::{framed, selected};
use crate::surfaces::overlay_gesture::{self, Gesture, Step};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "overlay_gesture.rs";

/// How far the pointer travels, in window pixels. Both axes move, so a scale
/// that reads one of them still changes, and none of the three sets lands back
/// on its own value: an idempotent write is a write nothing can see.
const TRAVEL: [f32; 2] = [60.0, 30.0];

/// Where a drag on one handle starts, in window pixels.
///
/// A handle is grabbed where it is drawn. That is not decoration: scaling
/// measures the pointer's distance from the layer's centre, so a corner grabbed
/// at the centre has no distance to grow from and the whole gesture is a no-op.
/// A crop or mask press carries no position at all, and the centre is as good a
/// place as any to start one from.
fn grab(state: &AppState, gesture: Gesture) -> [f32; 2] {
    let layer = state
        .overlay_frame
        .layer
        .expect("`framed` declared the picture's rectangle");
    match gesture {
        Gesture::Scale(handle) => layer.at(handle.anchor()),
        Gesture::Move | Gesture::Crop(_) | Gesture::Mask(_) => layer.at([0.5, 0.5]),
    }
}

/// Where that drag ends.
fn release(from: [f32; 2]) -> [f32; 2] {
    [from[0] + TRAVEL[0], from[1] + TRAVEL[1]]
}

/// A selected, framed clip carrying a mask, so all three handle sets are drawn.
fn framed_clip() -> AppState {
    let mut state = selected();
    framed(&mut state);
    state
        .set_selected_mask_kind(MaskKind::Circle)
        .expect("a mask to drag");
    state
}

/// Every handle the preview draws, in the order the panel draws them.
///
/// The three catalogs are the ones the panel loops over, so a fourth crop
/// handle or a second mask control is dragged here without an edit.
fn gestures() -> Vec<Gesture> {
    let mut out = vec![Gesture::Move];
    out.extend(TransformHandle::ALL.into_iter().map(Gesture::Scale));
    out.extend(CropHandle::ALL.into_iter().map(Gesture::Crop));
    out.extend(MaskHandle::ALL.into_iter().map(Gesture::Mask));
    out
}

/// The command one handle set writes.
fn writes(gesture: Gesture) -> &'static str {
    match gesture {
        Gesture::Move | Gesture::Scale(_) => "Transform",
        Gesture::Crop(_) => "Crop",
        Gesture::Mask(_) => "Set mask",
    }
}

/// Drag one handle the way the pointer does: press, move, release.
fn drag(state: &mut AppState, gesture: Gesture) {
    state.set_overlay_mode(gesture.mode());
    let from = grab(state, gesture);
    step(state, Step::Grab(gesture, from));
    step(state, Step::Carry(release(from)));
    step(state, Step::Release);
}

/// One step, refusing to pass a refusal off as a drag.
fn step(state: &mut AppState, step: Step) {
    overlay_gesture::apply(state, step)
        .unwrap_or_else(|err| panic!("a drag on the picture was refused: {err}"));
}

/// Every command a handle drag asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for gesture in gestures() {
        let mut state = framed_clip();
        let watch = Watch::on(&state);
        drag(&mut state, gesture);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_handle_the_preview_draws_is_dragged() {
    // Fail by default on a new arm: `what` is a total match, so the compiler
    // forces prose for it and this forces a drag.
    let dragged: BTreeSet<String> = gestures()
        .into_iter()
        .map(|gesture| gesture.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !dragged.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these canvas handles are drawn and nothing drags them: {missing:?}"
    );
}

#[test]
fn a_handle_sends_the_command_its_own_set_owns() {
    // The defect the missing surface hid: a corner in the wrong set. Every
    // handle is asserted to write its own set's command and nothing from
    // another set, because the sets share their pixels.
    for gesture in gestures() {
        let label = writes(gesture);
        let mut state = framed_clip();
        let watch = Watch::on(&state);
        drag(&mut state, gesture);
        let sent = watch.take();
        assert!(
            sent.contains(&label),
            "{} sent {sent:?} rather than `{label}`",
            gesture.what()
        );
        for other in ["Transform", "Crop", "Set mask"] {
            if other == label {
                continue;
            }
            assert!(
                !sent.contains(&other),
                "{} reached into the {other} set as well: {sent:?}",
                gesture.what()
            );
        }
    }
}

#[test]
fn a_press_that_never_moved_is_a_click_rather_than_an_edit() {
    // Selecting the picture is a press on the same surface a move starts from.
    // An edit here would put a no-op on the undo stack for every click.
    let mut state = framed_clip();
    let watch = Watch::on(&state);
    state.set_overlay_mode(Gesture::Move.mode());
    let from = grab(&state, Gesture::Move);
    step(&mut state, Step::Grab(Gesture::Move, from));
    step(&mut state, Step::Release);
    assert!(
        watch.take().is_empty(),
        "a press with no movement wrote an edit"
    );
}

#[test]
fn the_picture_follows_the_drag_before_it_is_committed() {
    // The handles read a draft while the drag is in flight, and the preview is
    // drafted with it: an outline sliding over a stale frame is the class this
    // closes. Asserted as the draft the handles read, because a gate has no
    // window to read pixels from.
    let mut state = framed_clip();
    let before = state.overlay_transform().expect("a selected transform");
    state.set_overlay_mode(Gesture::Move.mode());
    let from = grab(&state, Gesture::Move);
    step(&mut state, Step::Grab(Gesture::Move, from));
    step(&mut state, Step::Carry(release(from)));
    let mid = state.overlay_transform().expect("a drafted transform");
    assert_ne!(
        mid.position_x, before.position_x,
        "mid-drag the handles still read the committed position"
    );
    step(&mut state, Step::Release);
    assert!(
        !state.overlay_dragging(),
        "the drag outlived its own release"
    );
}

#[test]
fn a_cancelled_drag_leaves_the_project_alone() {
    // Every way out of a drag has to drop the draft, or the preview stays frozen
    // on a frame the project never received.
    let mut state = framed_clip();
    let watch = Watch::on(&state);
    state.set_overlay_mode(Gesture::Move.mode());
    let from = grab(&state, Gesture::Move);
    step(&mut state, Step::Grab(Gesture::Move, from));
    step(&mut state, Step::Carry(release(from)));
    state.cancel_overlay_drag();
    assert!(
        watch.take().is_empty(),
        "cancelling a drag committed it anyway"
    );
    assert!(
        !state.overlay_dragging(),
        "the cancelled drag is still live"
    );
}

#[test]
fn the_census_reaches_every_command_these_handles_claim() {
    let sent = dispatched();
    for label in ["Transform", "Crop", "Set mask"] {
        assert!(
            sent.contains(label),
            "the overlay census dragged every handle the preview draws and none \
             of them sent `{label}`"
        );
    }
}

#[test]
fn the_preview_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "preview");
}
