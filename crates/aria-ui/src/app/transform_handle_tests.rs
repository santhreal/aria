//! Moving and scaling the picture on the canvas.
//!
//! The class these close: framing was reachable only through inspector steppers,
//! and the on-canvas handles that did exist were crop and mask only. Adding a
//! third handle set on the same four corners makes "which set is live" the thing
//! that can silently go wrong, so the mode is asserted as state rather than
//! inferred from geometry, and the drag arithmetic is asserted in the units the
//! model stores (canvas-normalized position, aspect-fit-relative scale).
//!
//! Not covered: the drawn dots and pointer routing, which need a window. What is
//! covered is every rule the panel calls into.

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::canvas_overlay::{OverlayDrag, Rect, TransformHandle, drag_position, drag_scale};
use crate::scratch_pool::scratch;
use crate::types::OverlayMode;

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// A project with the still on the timeline and that clip selected.
pub(super) fn selected() -> AppState {
    let mut state = AppState::new(PluginContext::new(), scratch("xform")).expect("state");
    state.new_project().expect("project");
    let media = state.import_media(fixture()).expect("import the still");
    state
        .drop_media_on_timeline(media, None, 0)
        .expect("place the still");
    let clip = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|track| track.clips.iter())
        .map(|clip| clip.id)
        .next()
        .expect("one clip");
    state.select_clip(aria_models::ClipId::from_raw(clip));
    state
}

/// A canvas drawn at a known place, with the layer filling it: the arithmetic
/// below is stated against these numbers rather than against whatever the
/// resolver happens to produce.
pub(super) fn framed(state: &mut AppState) {
    state.overlay_frame.container = Some(Rect::new(0.0, 0.0, 400.0, 200.0));
    state.overlay_frame.canvas = Some(Rect::new(0.0, 0.0, 400.0, 200.0));
    state.overlay_frame.layer = Some(Rect::new(100.0, 50.0, 200.0, 100.0));
}

#[test]
fn position_moves_by_the_pointer_delta_over_the_canvas() {
    // Half the canvas width is half a position unit, whatever the layer's scale.
    let moved = drag_position(
        [0.0, 0.0],
        [0.0, 0.0],
        [200.0, 50.0],
        Rect::new(0.0, 0.0, 400.0, 200.0),
    );
    assert_eq!(moved, [0.5, 0.25]);
}

#[test]
fn position_keeps_its_starting_offset() {
    let moved = drag_position(
        [-0.25, 0.5],
        [10.0, 10.0],
        [10.0, 10.0],
        Rect::new(0.0, 0.0, 400.0, 200.0),
    );
    assert_eq!(moved, [-0.25, 0.5]);
}

#[test]
fn position_is_inert_on_a_degenerate_canvas() {
    let moved = drag_position(
        [0.1, 0.2],
        [0.0, 0.0],
        [50.0, 50.0],
        Rect::new(0.0, 0.0, 0.0, 0.0),
    );
    assert_eq!(moved, [0.1, 0.2]);
}

#[test]
fn a_corner_dragged_outward_scales_up_uniformly() {
    // Layer centered at (200, 100); the grab is its top-left corner, one unit of
    // distance away, and the pointer is dragged to twice that distance.
    let layer = Rect::new(100.0, 50.0, 200.0, 100.0);
    let scaled = drag_scale([1.0, 1.0], layer, [100.0, 50.0], [0.0, 0.0]);
    assert!((scaled[0] - 2.0).abs() < 1e-4, "scale x was {}", scaled[0]);
    assert_eq!(scaled[0], scaled[1], "a corner drag stays uniform");
}

#[test]
fn a_corner_dragged_inward_scales_down() {
    let layer = Rect::new(100.0, 50.0, 200.0, 100.0);
    let scaled = drag_scale([1.0, 1.0], layer, [100.0, 50.0], [150.0, 75.0]);
    assert!(
        scaled[0] < 1.0 && scaled[0] > 0.0,
        "scale was {}",
        scaled[0]
    );
}

#[test]
fn scale_is_clamped_rather_than_collapsing_to_zero() {
    let layer = Rect::new(100.0, 50.0, 200.0, 100.0);
    // Released exactly on the center: the factor is zero, the scale is the floor.
    let scaled = drag_scale([1.0, 1.0], layer, [100.0, 50.0], [200.0, 100.0]);
    assert!(scaled[0] >= 0.02, "scale floor breached: {}", scaled[0]);
    let big = drag_scale([19.0, 19.0], layer, [100.0, 50.0], [-4000.0, -4000.0]);
    assert_eq!(big, [20.0, 20.0], "scale ceiling");
}

#[test]
fn a_grab_on_the_layer_center_is_inert() {
    // Dividing by a near-zero starting distance would send the scale to the
    // ceiling from a one-pixel move.
    let layer = Rect::new(100.0, 50.0, 200.0, 100.0);
    let scaled = drag_scale([1.5, 1.5], layer, [200.0, 100.0], [260.0, 100.0]);
    assert_eq!(scaled, [1.5, 1.5]);
}

#[test]
fn the_mode_cycles_through_every_handle_set_and_wraps() {
    let mut state = selected();
    assert_eq!(state.overlay_mode(), OverlayMode::Transform, "the default");
    let mut seen = vec![state.overlay_mode()];
    for _ in 0..OverlayMode::ALL.len() {
        state.cycle_overlay_mode();
        seen.push(state.overlay_mode());
    }
    assert_eq!(
        seen.last(),
        Some(&OverlayMode::Transform),
        "the cycle returns to where it started"
    );
    for mode in OverlayMode::ALL {
        assert!(seen.contains(&mode), "{mode:?} was never offered");
    }
}

#[test]
fn cycling_names_the_live_set() {
    let mut state = selected();
    state.cycle_overlay_mode();
    let label = state.overlay_mode().label();
    assert_eq!(
        state.status_message.as_deref(),
        Some(&*format!("{label} handles"))
    );
}

#[test]
fn a_move_drag_writes_the_dragged_position() {
    let mut state = selected();
    framed(&mut state);
    state.begin_transform_drag(None, [200.0, 100.0]);
    state.drag_overlay_to([300.0, 100.0]);
    state.end_overlay_drag().expect("commit the move");
    let after = state.selected_ui_clip().expect("selection").transform;
    assert!(
        (after.position_x - 0.25).abs() < 1e-4,
        "position x was {}",
        after.position_x
    );
    assert!(!state.overlay_dragging(), "the drag is over");
}

#[test]
fn a_corner_drag_writes_the_dragged_scale() {
    let mut state = selected();
    framed(&mut state);
    let before = state.selected_ui_clip().expect("selection").transform;
    state.begin_transform_drag(Some(TransformHandle::TopLeft), [100.0, 50.0]);
    state.drag_overlay_to([0.0, 0.0]);
    state.end_overlay_drag().expect("commit the scale");
    let after = state.selected_ui_clip().expect("selection").transform;
    assert!(
        after.scale_x > before.scale_x,
        "scale did not grow: {} -> {}",
        before.scale_x,
        after.scale_x
    );
}

#[test]
fn a_press_with_no_movement_costs_no_undo_entry() {
    let mut state = selected();
    framed(&mut state);
    let revision = state.revision;
    state.begin_transform_drag(None, [200.0, 100.0]);
    state.end_overlay_drag().expect("release");
    assert_eq!(state.revision, revision, "a click is not an edit");
}

#[test]
fn the_live_draft_is_what_the_handles_read() {
    let mut state = selected();
    framed(&mut state);
    let committed = state.selected_ui_clip().expect("selection").transform;
    state.begin_transform_drag(None, [200.0, 100.0]);
    state.drag_overlay_to([240.0, 100.0]);
    let drafted = state.overlay_transform().expect("a draft mid-drag");
    assert_ne!(
        drafted.position_x, committed.position_x,
        "the handles must follow the pointer, not the project"
    );
    state.cancel_overlay_drag();
    assert_eq!(
        state.overlay_transform().expect("the committed transform"),
        committed,
        "cancelling restores what the project holds"
    );
}

#[test]
fn cancelling_a_transform_drag_writes_nothing() {
    let mut state = selected();
    framed(&mut state);
    let revision = state.revision;
    state.begin_transform_drag(Some(TransformHandle::BottomRight), [300.0, 150.0]);
    state.drag_overlay_to([390.0, 190.0]);
    state.cancel_overlay_drag();
    assert_eq!(state.revision, revision);
    assert!(!state.overlay_dragging());
}

#[test]
fn a_transform_drag_needs_a_selection() {
    let mut state = AppState::new(PluginContext::new(), scratch("xform-empty")).expect("state");
    state.new_project().expect("project");
    state.begin_transform_drag(None, [10.0, 10.0]);
    assert!(
        !state.overlay_dragging(),
        "nothing selected means nothing to move"
    );
}

#[test]
fn every_corner_has_its_own_id_and_anchor() {
    let mut ids = std::collections::BTreeSet::new();
    let mut anchors = Vec::new();
    for handle in TransformHandle::ALL {
        assert!(ids.insert(handle.id()), "duplicate id {}", handle.id());
        assert_eq!(
            TransformHandle::from_id(handle.id()),
            Some(handle),
            "{} does not round-trip",
            handle.id()
        );
        anchors.push(handle.anchor());
    }
    assert_eq!(ids.len(), 4);
    for anchor in &anchors {
        assert!(anchors.iter().filter(|other| *other == anchor).count() == 1);
        assert!(
            anchor.iter().all(|f| *f == 0.0 || *f == 1.0),
            "corners only"
        );
    }
}

#[test]
fn a_move_and_a_scale_are_distinguishable_mid_drag() {
    let mut state = selected();
    framed(&mut state);
    state.begin_transform_drag(Some(TransformHandle::TopLeft), [100.0, 50.0]);
    match state.overlay_drag {
        Some(OverlayDrag::Transform { handle, .. }) => {
            assert_eq!(handle, Some(TransformHandle::TopLeft))
        }
        other => panic!("expected a transform drag, got {other:?}"),
    }
    state.cancel_overlay_drag();
    state.begin_transform_drag(None, [200.0, 100.0]);
    match state.overlay_drag {
        Some(OverlayDrag::Transform { handle, .. }) => assert_eq!(handle, None, "no corner moves"),
        other => panic!("expected a transform drag, got {other:?}"),
    }
}
