//! A press on the Viewer's picture picks the clip it moves, and one drag moves
//! it.
//!
//! The class closed here: a press on the picture that selected one clip and
//! dragged another, or selected a clip and dragged nothing. Two handlers take
//! a left press on the picture, the selected layer's move surface first and
//! the picture under it second, and each picked its clip on its own. A press
//! on a clip not yet selected reached only the picture, which selected the
//! clip and took no hold of it, so the drag that followed did nothing. A press
//! inside the selected clip where another clip lay on top reached both: the
//! move surface took hold of the selected clip, then the picture selected the
//! clip on top, and the release wrote the held clip's pose onto that one. A
//! move that arrives before the Viewer paints again is carried too: the press
//! records the picked clip's box itself, where a press with nothing selected
//! left no box and the move was dropped.
//!
//! Not covered: a press with the Type tool or the Pen, which place a caret or
//! a vertex and move nothing, and a press with the crop or mask handles up.

use aria_models::{ClipId, TrackKind};
use gpui::{VisualTestContext, point, px};

use super::held_drag_panel_tests::settle;
use super::held_drag_tests::{carry, press, release};
use super::transform_handle_tests::selected;
use super::viewer_row_tests::{change, shell_with_take};
use crate::app::AppState;
use crate::app::canvas_overlay::Rect;
use crate::surfaces::overlay_gesture::{self, Step};
use crate::types::{EditMode, OverlayMode, UiTrackKind};

/// The picture clips on the timeline, bottom lane first.
fn picture_clips(state: &AppState) -> Vec<ClipId> {
    let mut clips: Vec<ClipId> = state
        .store
        .sequence
        .tracks
        .iter()
        .filter(|lane| lane.kind == UiTrackKind::Video)
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| ClipId::from_raw(clip.id))
        .collect();
    // Visual lanes are listed top first.
    clips.reverse();
    clips
}

/// `clip`'s position, read by selecting it.
fn position_of(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    clip: ClipId,
) -> [f32; 2] {
    let mut at = [0.0; 2];
    change(visual, state, |state| {
        state.select_clip(clip);
        let pose = state.selected_pose().expect("the clip has a pose");
        at = [pose.position_x, pose.position_y];
    });
    at
}

/// Press at `grab`, carry 40 px right and 30 px down, and let go there.
/// Returns the position the drag should write onto a clip that started at
/// `start`.
fn drag_picture(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    grab: [f32; 2],
    start: [f32; 2],
) -> [f32; 2] {
    let canvas = visual.read(|app| {
        state
            .read(app)
            .overlay_frame
            .canvas
            .expect("the Viewer draws the canvas")
    });
    let to = [grab[0] + 40.0, grab[1] + 30.0];
    press(visual, point(px(grab[0]), px(grab[1])));
    assert!(
        visual.read(|app| state.read(app).overlay_dragging()),
        "the press on the picture took hold of no clip"
    );
    carry(visual, point(px(to[0]), px(to[1])));
    release(visual, point(px(to[0]), px(to[1])));
    [
        start[0] + (to[0] - grab[0]) / canvas.w,
        start[1] + (to[1] - grab[1]) / canvas.h,
    ]
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[gpui::test]
fn a_press_on_a_clip_not_selected_selects_it_and_the_drag_moves_it(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "picture-press-unselected");
    let take = visual.read(|app| picture_clips(state.read(app))[0]);
    let start = position_of(visual, &state, take);
    change(visual, &state, |state| {
        state.clear_selection();
        state.set_overlay_mode(OverlayMode::Transform);
    });
    settle(visual);
    let canvas = visual.read(|app| {
        state
            .read(app)
            .overlay_frame
            .canvas
            .expect("the Viewer draws the canvas")
    });
    // Off the anchor point at the center of the frame the take fills.
    let grab = canvas.at([0.35, 0.35]);

    let moved = drag_picture(visual, &state, grab, start);
    assert_eq!(
        visual.read(|app| state.read(app).selected_clip()),
        Some(take),
        "the press selected another clip"
    );
    let landed = position_of(visual, &state, take);
    assert!(
        close(landed, moved),
        "the drag left the take at {landed:?}, not {moved:?}"
    );
}

#[gpui::test]
fn a_press_inside_the_selected_clip_under_another_moves_the_selected_one(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, media) = shell_with_take(cx, "picture-press-covered");
    // A second copy of the take on a lane above covers the whole frame. The
    // drop overwrites: a ripple drop would push the take past the playhead.
    change(visual, &state, |state| {
        state.edit_mode = EditMode::Overwrite;
        let above = state
            .add_lane_of_kind(TrackKind::Video, None)
            .expect("a lane above the take");
        state
            .drop_media_on_timeline(media, Some(above), 0)
            .expect("place the copy above");
    });
    let clips = visual.read(|app| picture_clips(state.read(app)));
    let [under, over] = clips[..] else {
        panic!("two picture clips, one over the other: {clips:?}");
    };
    let (under_start, over_start) = (
        position_of(visual, &state, under),
        position_of(visual, &state, over),
    );
    change(visual, &state, |state| {
        state.select_clip(under);
        state.set_overlay_mode(OverlayMode::Transform);
    });
    settle(visual);
    let layer = visual.read(|app| {
        state
            .read(app)
            .overlay_frame
            .layer
            .expect("the selected clip draws a layer")
    });

    let moved = drag_picture(visual, &state, layer.at([0.35, 0.35]), under_start);
    assert_eq!(
        visual.read(|app| state.read(app).selected_clip()),
        Some(under),
        "the press inside the selected clip selected the clip over it"
    );
    let landed = position_of(visual, &state, under);
    assert!(
        close(landed, moved),
        "the drag left the selected clip at {landed:?}, not {moved:?}"
    );
    assert_eq!(
        position_of(visual, &state, over),
        over_start,
        "the drag of the selected clip moved the clip over it"
    );
}

#[test]
fn a_move_before_the_next_paint_carries_the_clip_the_press_picked() {
    let mut state = selected();
    state
        .set_canvas_resolution(Some([400, 200]))
        .expect("a 400 by 200 canvas");
    let still = state.selected_clip().expect("the still is selected");
    let pose = state.selected_pose().expect("the still has a pose");
    let start = [pose.position_x, pose.position_y];
    // The Viewer last painted with nothing selected, so it recorded no box.
    state.clear_selection();
    state.set_overlay_mode(OverlayMode::Transform);
    state.record_overlay_frame(Rect::new(0.0, 0.0, 400.0, 200.0));
    assert_eq!(
        state.overlay_frame.layer, None,
        "a box with nothing selected"
    );

    let grab = [180.0, 80.0];
    let to = [grab[0] + 40.0, grab[1] + 30.0];
    overlay_gesture::press(&mut state, grab, false);
    overlay_gesture::run(&mut state, Step::Carry(to));
    overlay_gesture::run(&mut state, Step::Release);
    assert_eq!(
        state.selected_clip(),
        Some(still),
        "the press picked no clip"
    );
    let pose = state.selected_pose().expect("the still has a pose");
    let moved = [start[0] + 40.0 / 400.0, start[1] + 30.0 / 200.0];
    assert!(
        close([pose.position_x, pose.position_y], moved),
        "the move before the next paint left the still at {:?}, not {moved:?}",
        [pose.position_x, pose.position_y]
    );
}
