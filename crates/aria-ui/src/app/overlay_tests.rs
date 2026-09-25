//! Contracts for the on-canvas crop and mask handles.
//!
//! Two classes are closed here. The first is geometry that only looks right: a
//! handle that lands on the letterbox bar instead of the picture, or a drag whose
//! fraction is taken across the panel rather than across the clip's own quad. The
//! second is the one every gesture in this app has to answer — a drag is one edit,
//! a release that moved nothing is no edit, and an abandoned drag costs nothing.
//!
//! Handle and drag rules are enumerated from `CropHandle::ALL` and
//! `MaskHandle::ALL` at run time, so a new handle fails until it is covered.

use aria_models::{CropRect, MIN_CROP_FRACTION};

use super::canvas_overlay::{
    CropHandle, MaskHandle, Rect, crop_handle_anchor, drag_crop, drag_mask_center,
    drag_mask_rotation, drag_mask_size, fit_frame, layer_rect, mask_box, mask_handle_anchor,
};
use super::premiere_tests::with_clip;

fn approx(got: f32, want: f32, what: &str) {
    assert!(
        (got - want).abs() < 1e-3,
        "{what}: expected {want}, got {got}"
    );
}

/// The picture area is letterboxed exactly the way the image element draws it, so
/// a handle sits on the frame and not on the bar beside it.
#[test]
fn the_canvas_is_letterboxed_inside_the_panel() {
    // A wide canvas in a square panel: bars top and bottom.
    let fit = fit_frame(Rect::new(0.0, 0.0, 400.0, 400.0), [1920, 1080]);
    approx(fit.w, 400.0, "width fills");
    approx(fit.h, 225.0, "height keeps the ratio");
    approx(fit.x, 0.0, "no horizontal bar");
    approx(fit.y, 87.5, "centered vertically");

    // A tall canvas in a wide panel: bars left and right.
    let fit = fit_frame(Rect::new(10.0, 20.0, 800.0, 400.0), [1080, 1920]);
    approx(fit.h, 400.0, "height fills");
    approx(fit.w, 225.0, "width keeps the ratio");
    approx(fit.x, 10.0 + (800.0 - 225.0) / 2.0, "centered horizontally");
    approx(fit.y, 20.0, "no vertical bar");

    // A degenerate canvas leaves the container alone rather than dividing by zero.
    let container = Rect::new(0.0, 0.0, 100.0, 50.0);
    assert_eq!(fit_frame(container, [0, 0]), container);
}

/// A clip's handles follow the clip's own quad, so a scaled or moved clip is
/// cropped where it is drawn rather than where the canvas is.
#[test]
fn the_layer_quad_follows_the_resolved_placement() {
    let canvas_rect = Rect::new(100.0, 50.0, 960.0, 540.0);
    // Full-canvas layer: the quad is the canvas.
    let full = layer_rect(canvas_rect, [1920, 1080], [960.0, 540.0], [1920.0, 1080.0]);
    assert_eq!(full, canvas_rect);

    // Half-size layer pushed to the right: half the size, offset by the same
    // fraction of the drawn canvas.
    let half = layer_rect(canvas_rect, [1920, 1080], [1440.0, 540.0], [960.0, 540.0]);
    approx(half.w, 480.0, "quad width");
    approx(half.h, 270.0, "quad height");
    approx(half.x, 100.0 + 480.0, "quad left");
    approx(half.y, 50.0 + 135.0, "quad top");
}

/// A window point is read across the clip's quad, and a pointer that leaves the
/// quad stops at its edge instead of running away with the crop.
#[test]
fn a_pointer_position_reads_as_a_clamped_layer_fraction() {
    let layer = Rect::new(100.0, 100.0, 400.0, 200.0);
    assert_eq!(layer.fraction_of([300.0, 200.0]), [0.5, 0.5]);
    assert_eq!(layer.fraction_of([100.0, 100.0]), [0.0, 0.0]);
    assert_eq!(layer.fraction_of([500.0, 300.0]), [1.0, 1.0]);
    assert_eq!(layer.fraction_of([-500.0, 9999.0]), [0.0, 1.0]);
    // A collapsed quad reads as its origin rather than dividing by zero.
    assert_eq!(
        Rect::new(5.0, 5.0, 0.0, 0.0).fraction_of([9.0, 9.0]),
        [0.0, 0.0]
    );
    assert_eq!(layer.at([0.5, 0.25]), [300.0, 150.0]);
}

/// Every crop handle sits on the crop window it moves, and moves only the edges it
/// owns. Enumerated from `CropHandle::ALL`, so a ninth handle fails here first.
#[test]
fn every_crop_handle_sits_on_its_edge_and_moves_only_that_edge() {
    let crop = CropRect {
        x: 0.2,
        y: 0.1,
        w: 0.6,
        h: 0.8,
    };
    assert_eq!(CropHandle::ALL.len(), 8);
    for handle in CropHandle::ALL {
        assert_eq!(CropHandle::from_id(handle.id()), Some(handle));
        let [ax, ay] = crop_handle_anchor(crop, handle);
        // The anchor is always on the crop rect's boundary or its mid-line.
        let on_x = (ax - crop.x).abs() < 1e-6
            || (ax - (crop.x + crop.w)).abs() < 1e-6
            || (ax - (crop.x + crop.w * 0.5)).abs() < 1e-6;
        let on_y = (ay - crop.y).abs() < 1e-6
            || (ay - (crop.y + crop.h)).abs() < 1e-6
            || (ay - (crop.y + crop.h * 0.5)).abs() < 1e-6;
        assert!(on_x && on_y, "{} sits off the rect", handle.id());

        // Dragging to the anchor itself changes nothing: a click without a move is
        // not an edit.
        let same = drag_crop(crop, handle, [ax, ay]);
        approx(same.x, crop.x, handle.id());
        approx(same.y, crop.y, handle.id());
        approx(same.w, crop.w, handle.id());
        approx(same.h, crop.h, handle.id());

        // A real drag keeps the far edges of the axes it does not own.
        let moved = drag_crop(crop, handle, [0.45, 0.55]);
        moved
            .validate()
            .unwrap_or_else(|err| panic!("{} produced an invalid rect: {err:?}", handle.id()));
        if (ax - (crop.x + crop.w * 0.5)).abs() < 1e-6 {
            approx(moved.x, crop.x, "untouched x");
            approx(moved.w, crop.w, "untouched width");
        }
        if (ay - (crop.h * 0.5 + crop.y)).abs() < 1e-6 {
            approx(moved.y, crop.y, "untouched y");
            approx(moved.h, crop.h, "untouched height");
        }
    }
}

/// A drag past the opposite edge stalls at the minimum window instead of
/// inverting the rect, which the model would refuse.
#[test]
fn a_crop_drag_stops_at_the_minimum_window() {
    for handle in CropHandle::ALL {
        for target in [[-5.0, -5.0], [5.0, 5.0], [0.5, 0.5]] {
            let out = drag_crop(CropRect::FULL, handle, target);
            out.validate().unwrap_or_else(|err| {
                panic!("{} at {target:?}: {err:?}", handle.id());
            });
            assert!(
                out.w >= MIN_CROP_FRACTION - 1e-6 && out.h >= MIN_CROP_FRACTION - 1e-6,
                "{} at {target:?} collapsed to {out:?}",
                handle.id()
            );
        }
    }
}

/// The mask outline is derived from the model's center offset and size fractions,
/// and each handle sits on the part of it that it moves.
#[test]
fn every_mask_handle_sits_on_its_outline() {
    assert_eq!(MaskHandle::ALL.len(), 3);
    let (center, size) = ([0.1, -0.2], [0.5, 0.25]);
    let [x, y, w, h] = mask_box(center, size);
    approx(x, 0.5 + 0.1 - 0.25, "outline left");
    approx(y, 0.5 - 0.2 - 0.125, "outline top");
    approx(w, 0.5, "outline width");
    approx(h, 0.25, "outline height");

    for handle in MaskHandle::ALL {
        assert_eq!(MaskHandle::from_id(handle.id()), Some(handle));
        let [ax, ay] = mask_handle_anchor(center, size, handle);
        match handle {
            MaskHandle::Center => {
                approx(ax, x + w * 0.5, "center handle x");
                approx(ay, y + h * 0.5, "center handle y");
            }
            MaskHandle::Size => {
                approx(ax, x + w, "size handle x");
                approx(ay, y + h, "size handle y");
            }
            MaskHandle::Rotate => {
                approx(ax, x + w * 0.5, "rotate handle x");
                assert!(ay < y + h * 0.5, "the rotate handle sits above the box");
            }
        }
    }

    // A zero size still draws a grabbable outline rather than a point.
    let [_, _, w, h] = mask_box([0.0, 0.0], [0.0, 0.0]);
    assert!(w > 0.0 && h > 0.0);
}

/// Each mask drag moves what it names and nothing else: the center follows the
/// pointer, the size grows about the center, and the rotation reads as a clockwise
/// angle from straight up.
#[test]
fn each_mask_drag_moves_only_what_it_names() {
    assert_eq!(drag_mask_center([0.5, 0.5]), [0.0, 0.0]);
    assert_eq!(drag_mask_center([1.0, 0.0]), [0.5, -0.5]);

    // The corner follows the pointer, so the box is twice the distance from the
    // center on each axis.
    let size = drag_mask_size([0.0, 0.0], [0.75, 0.6]);
    approx(size[0], 0.5, "width follows the corner");
    approx(size[1], 0.2, "height follows the corner");
    // Dragging through the center clamps instead of inverting.
    let size = drag_mask_size([0.0, 0.0], [0.5, 0.5]);
    assert!(size[0] > 0.0 && size[1] > 0.0);

    // A square layer keeps the angle honest; straight up is zero and clockwise
    // grows.
    let square = Rect::new(0.0, 0.0, 100.0, 100.0);
    approx(
        drag_mask_rotation([0.0, 0.0], [0.5, 0.0], square),
        0.0,
        "up",
    );
    approx(
        drag_mask_rotation([0.0, 0.0], [1.0, 0.5], square),
        90.0,
        "right",
    );
    approx(
        drag_mask_rotation([0.0, 0.0], [0.5, 1.0], square),
        180.0,
        "down",
    );
    approx(
        drag_mask_rotation([0.0, 0.0], [0.0, 0.5], square),
        270.0,
        "left",
    );
    // No movement is no rotation, not a NaN.
    approx(
        drag_mask_rotation([0.0, 0.0], [0.5, 0.5], square),
        0.0,
        "no movement",
    );
}

/// A crop gesture is one edit: the moves build a draft, the release commits it,
/// and the preview shows the uncropped frame while the handles are moving so the
/// operator cuts into a picture they can see.
#[test]
fn a_crop_drag_is_one_edit_and_previews_the_uncropped_frame() {
    let (mut state, _) = with_clip();
    state.record_overlay_frame(Rect::new(0.0, 0.0, 100.0, 100.0));
    // The fixture's clip fills the canvas, so its quad is the picture area.
    let quad = state.overlay_frame.layer.expect("the clip has a quad");
    let before = state.engine().lock().revision();

    state.begin_crop_drag(CropHandle::Left);
    assert!(state.overlay_dragging());
    assert!(
        state.engine().lock().has_live_overrides(),
        "the drag must preview the uncropped frame"
    );

    // Three moves, no edits: the revision only grows on a committed command.
    for x in [0.1, 0.2, 0.3] {
        state.drag_overlay_to([quad.x + quad.w * x, quad.y + quad.h * 0.5]);
        assert_eq!(
            state.engine().lock().revision(),
            before,
            "a pointer move reached the project"
        );
    }
    let draft = state.overlay_crop().expect("a draft while dragging");
    approx(draft.x, 0.3, "the draft followed the pointer");

    state.end_overlay_drag().expect("commit");
    assert!(!state.overlay_dragging());
    assert!(
        !state.engine().lock().has_live_overrides(),
        "the override must be dropped on release"
    );
    let ui = state.selected_ui_clip().expect("clip");
    approx(ui.crop.x, 0.3, "the commit kept the dragged edge");

    // One undo, and the whole gesture is gone: a second entry from the same drag
    // would leave a half-cropped rect here.
    assert!(state.undo());
    assert!(
        state.selected_ui_clip().expect("clip").crop.is_full(),
        "one undo restores the whole frame"
    );
}

/// A release that moved nothing writes nothing, and Esc during a drag restores the
/// project without an edit.
#[test]
fn a_still_release_and_a_cancel_both_cost_nothing() {
    let (mut state, _) = with_clip();
    state.record_overlay_frame(Rect::new(0.0, 0.0, 100.0, 100.0));
    let before = state.engine().lock().revision();

    state.begin_crop_drag(CropHandle::Right);
    state.end_overlay_drag().expect("still release");
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a release that moved nothing wrote an edit"
    );

    let quad = state.overlay_frame.layer.expect("quad");
    state.begin_crop_drag(CropHandle::Top);
    state.drag_overlay_to([quad.x + quad.w * 0.5, quad.y + quad.h * 0.4]);
    state.cancel_overlay_drag();
    assert!(!state.overlay_dragging());
    assert!(
        !state.engine().lock().has_live_overrides(),
        "a cancelled drag left a preview override behind"
    );
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a cancelled drag wrote an edit"
    );
    assert!(
        state.selected_ui_clip().expect("clip").crop.is_full(),
        "a cancelled drag changed the project"
    );
}

/// A mask drag commits center, size, and rotation as one edit, and a clip with no
/// mask has nothing to drag: the shape is chosen in the inspector first.
#[test]
fn a_mask_drag_commits_once_and_needs_a_mask_to_start() {
    let (mut state, _) = with_clip();
    state.record_overlay_frame(Rect::new(0.0, 0.0, 200.0, 200.0));

    state.begin_mask_drag(MaskHandle::Center);
    assert!(
        !state.overlay_dragging(),
        "a clip with no mask must not start a mask drag"
    );

    state
        .set_selected_mask_kind(aria_models::MaskKind::Circle)
        .expect("add a mask");

    let quad = state.overlay_frame.layer.expect("quad");
    let before = state.engine().lock().revision();
    state.begin_mask_drag(MaskHandle::Center);
    assert!(state.overlay_dragging());
    state.drag_overlay_to([quad.x + quad.w * 0.75, quad.y + quad.h * 0.25]);
    assert_eq!(
        state.engine().lock().revision(),
        before,
        "a mask pointer move reached the project"
    );
    let (center, ..) = state.overlay_mask().expect("a live mask");
    approx(center[0], 0.25, "the mask centre followed the pointer");
    approx(center[1], -0.25, "the mask centre followed the pointer");

    state.end_overlay_drag().expect("commit");
    let mask = state.selected_ui_clip().expect("clip").mask.expect("mask");
    approx(mask.center[0], 0.25, "committed centre x");
    approx(mask.center[1], -0.25, "committed centre y");
    assert!(state.undo(), "the mask move is one undo entry");
    let mask = state.selected_ui_clip().expect("clip").mask.expect("mask");
    approx(mask.center[0], 0.0, "undo restored the centre");
}
