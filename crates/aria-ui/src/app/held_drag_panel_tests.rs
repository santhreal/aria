//! A drag begun on a Viewer, Mixer or track header element follows the
//! pointer past that element, and the release ends it wherever the button
//! comes up.
//!
//! An element's own move and release handlers run only while the pointer is
//! over it, so a drag they carry stops at the element's edge and a release
//! beside it reaches none of them: the edit is lost and the drag stays held.
//! The class closed here: each drag of these panels that was carried by the
//! handlers of the element it began on. A picture moved by its handle on the
//! Viewer, a title's selection dragged with the Type tool, a zoomed frame
//! panned, the master fader ridden and a track header dropped on another row
//! are each pressed, carried off their element, and let go there. Each follows
//! the pointer the whole way, the release writes it, and a move after the
//! release changes nothing.
//!
//! The timeline's drags are in `held_drag_tests`.
//!
//! Not covered: a crop or mask handle, a motion path point and a mask path
//! vertex, which the picture handle's branch of the shell carries and no test
//! here drives, and a guide pulled from a Viewer ruler.

use aria_models::TrackKind;
use gpui::{Modifiers, VisualTestContext, point, px};

use super::held_drag_tests::{bounds, carry, press, release};
use crate::app::mixer::FaderTarget;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::types::{EditorTool, OverlayMode, UiTrackKind};

/// Draw the shell twice: the Viewer records its picture area in paint, and
/// the handles are placed from that record on the next frame.
fn settle(visual: &mut VisualTestContext) {
    for _ in 0..2 {
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
    }
}

/// A move with no button held.
fn hover(visual: &mut VisualTestContext, at: gpui::Point<gpui::Pixels>) {
    visual.simulate_mouse_move(at, None, Modifiers::none());
    visual.run_until_parked();
}

fn close(a: [f32; 2], b: [f32; 2]) -> bool {
    (a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3
}

#[gpui::test]
fn a_picture_moved_off_the_viewer_lands_where_the_button_comes_up(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "held-viewer-move");
    change(visual, &state, |state| {
        let clip = state
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.kind == UiTrackKind::Video)
            .flat_map(|lane| lane.clips.iter())
            .next()
            .expect("the take has a picture clip")
            .id;
        state.select_clip(aria_models::ClipId::from_raw(clip));
        state.set_overlay_mode(OverlayMode::Transform);
    });
    settle(visual);
    let (container, canvas, layer, start) = visual.read(|app| {
        let state = state.read(app);
        let frame = state.overlay_frame;
        let pose = state.selected_pose().expect("the take is selected");
        (
            frame
                .container
                .expect("the Viewer records its picture area"),
            frame.canvas.expect("the Viewer draws the canvas"),
            frame.layer.expect("the selected take draws a layer"),
            [pose.position_x, pose.position_y],
        )
    });
    let position = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let pose = state
                .read(app)
                .selected_pose()
                .expect("the take is selected");
            [pose.position_x, pose.position_y]
        })
    };
    let drafted = |visual: &mut VisualTestContext| {
        visual.read(|app| {
            let pose = state
                .read(app)
                .overlay_transform()
                .expect("the take is selected");
            [pose.position_x, pose.position_y]
        })
    };
    // Off the anchor point at the layer's centre, on the move surface.
    let grab = layer.at([0.35, 0.35]);
    let to = [grab[0] + 25.0, container.y + container.h + 40.0];
    let moved = [
        start[0] + (to[0] - grab[0]) / canvas.w,
        start[1] + (to[1] - grab[1]) / canvas.h,
    ];

    press(visual, point(px(grab[0]), px(grab[1])));
    assert!(
        visual.read(|app| state.read(app).overlay_dragging()),
        "the press on the picture grabs it"
    );
    carry(visual, point(px(to[0]), px(to[1])));
    assert!(
        close(drafted(visual), moved),
        "a move below the Viewer drafts {:?}, not {moved:?}",
        drafted(visual)
    );
    release(visual, point(px(to[0]), px(to[1])));
    assert!(
        !visual.read(|app| state.read(app).overlay_dragging()),
        "the release below the Viewer ends the drag"
    );
    assert!(
        close(position(visual), moved),
        "the release below the Viewer writes {:?}, not {moved:?}",
        position(visual)
    );

    hover(visual, point(px(grab[0]), px(grab[1])));
    assert!(
        close(position(visual), moved),
        "a move after the release moved the picture to {:?}",
        position(visual)
    );
}

#[gpui::test]
fn a_title_selection_dragged_off_the_viewer_follows_the_pointer(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "held-text-caret");
    settle(visual);
    change(visual, &state, |state| {
        let canvas = state
            .overlay_frame
            .canvas
            .expect("the Viewer draws the canvas");
        state.tool = EditorTool::Type;
        assert!(
            state.select_layer_at(canvas.at([0.5, 0.3]), false),
            "the Type tool places a title"
        );
        for ch in "abcdefgh".chars() {
            state.title_push(ch);
        }
        state.end_text_edit();
    });
    settle(visual);
    // The title's boundaries, in window pixels, by index.
    let (container, stops) = visual.read(|app| {
        let state = state.read(app);
        let canvas = state
            .overlay_frame
            .canvas
            .expect("the Viewer draws the canvas");
        let [width, height] = state.canvas_size();
        let window = |[x, y]: [f32; 2]| {
            [
                canvas.x + x * canvas.w / width as f32,
                canvas.y + y * canvas.h / height as f32,
            ]
        };
        let carets = state
            .selected
            .as_ref()
            .and_then(|detail| detail.carets.clone())
            .expect("a title on screen has carets");
        let mut stops: Vec<(usize, [f32; 2])> = carets
            .iter()
            .map(|caret| {
                (
                    caret.index,
                    window([
                        0.5 * (caret.top[0] + caret.bottom[0]),
                        0.5 * (caret.top[1] + caret.bottom[1]),
                    ]),
                )
            })
            .collect();
        stops.sort_by_key(|(index, _)| *index);
        (
            state
                .overlay_frame
                .container
                .expect("the Viewer records its picture area"),
            stops,
        )
    });
    let (last, end) = *stops.last().expect("the title has boundaries");
    assert_eq!(last, 8, "the title holds the typed text");
    let selection =
        |visual: &mut VisualTestContext| visual.read(|app| state.read(app).text_selection());
    let at = |[x, y]: [f32; 2]| point(px(x), px(y));
    let below = [end[0], container.y + container.h + 40.0];

    press(visual, at(stops[2].1));
    assert_eq!(selection(visual), (2, 2), "the press places the caret");
    assert!(visual.read(|app| state.read(app).text_dragging()));
    carry(visual, at(below));
    assert_eq!(
        selection(visual),
        (2, 8),
        "a move below the Viewer selects to the nearest boundary"
    );
    release(visual, at(below));
    assert!(
        !visual.read(|app| state.read(app).text_dragging()),
        "the release below the Viewer ends the drag"
    );

    hover(visual, at(stops[0].1));
    assert_eq!(
        selection(visual),
        (2, 8),
        "a move after the release changed the selection"
    );
}

#[gpui::test]
fn a_zoomed_frame_panned_off_the_viewer_follows_the_pointer(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "held-viewer-pan");
    change(visual, &state, |state| {
        state.clear_selection();
        state.preview_view.zoom = Some(2.0);
        state.preview_view.pan = (0.0, 0.0);
    });
    settle(visual);
    let container = visual.read(|app| {
        state
            .read(app)
            .overlay_frame
            .container
            .expect("the Viewer records its picture area")
    });
    let pan = |visual: &mut VisualTestContext| visual.read(|app| state.read(app).preview_view.pan);
    let grab = [
        container.x + container.w / 2.0,
        container.y + container.h / 2.0,
    ];
    let below = [grab[0] + 40.0, container.y + container.h + 40.0];
    let at = |[x, y]: [f32; 2]| point(px(x), px(y));

    press(visual, at(grab));
    carry(visual, at([grab[0] + 30.0, grab[1] + 20.0]));
    assert_eq!(pan(visual), (30.0, 20.0), "a move on the picture");
    carry(visual, at(below));
    assert_eq!(
        pan(visual),
        (below[0] - grab[0], below[1] - grab[1]),
        "a move below the Viewer"
    );
    release(visual, at(below));

    hover(visual, at(grab));
    assert_eq!(
        pan(visual),
        (below[0] - grab[0], below[1] - grab[1]),
        "a move after the release panned the frame"
    );
}

#[gpui::test]
fn the_master_fader_ridden_off_its_strip_writes_where_the_button_comes_up(
    cx: &mut gpui::TestAppContext,
) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "held-fader");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state
                .add_lane_of_kind(TrackKind::Audio, None)
                .unwrap_or_else(|err| panic!("an audio lane: {err:?}"));
            assert!(state.apply_workspace("win-workspace-audio", cx));
        })
    });
    settle(visual);
    let fader = bounds(visual, "mixer-fader-0");
    let gain =
        |visual: &mut VisualTestContext| visual.read(|app| state.read(app).facts.mixer.master.gain);
    let start = gain(visual);
    let travel = f32::from(fader.size.height);
    // A quarter of the travel down, beside the master strip, which is the
    // last: no fader is under the pointer there.
    let to = point(
        fader.right() + px(90.0),
        fader.center().y + px(travel * 0.25),
    );
    let ridden = FaderTarget::Master.frac_to_value(FaderTarget::Master.value_to_frac(start) - 0.25);
    assert!(
        (ridden - start).abs() > 1e-3,
        "a quarter of the travel moves the fader"
    );

    press(visual, fader.center());
    carry(visual, to);
    let drafted = visual.read(|app| state.read(app).fader_dragging().map(|drag| drag.gain));
    assert!(
        drafted.is_some_and(|drafted| (drafted - ridden).abs() < 1e-4),
        "a move beside the strip rides the fader to {drafted:?}, not {ridden}"
    );
    release(visual, to);
    assert!(
        visual
            .read(|app| state.read(app).fader_dragging())
            .is_none(),
        "the release beside the strip ends the ride"
    );
    assert!(
        (gain(visual) - ridden).abs() < 1e-4,
        "the release beside the strip writes {}, not {ridden}",
        gain(visual)
    );

    hover(visual, fader.center());
    assert!(
        (gain(visual) - ridden).abs() < 1e-4,
        "a move after the release changed the gain to {}",
        gain(visual)
    );
}

#[gpui::test]
fn a_track_header_let_go_off_the_headers_drops_on_the_marked_row(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "held-track-header");
    // The take's lane is the main track, which stays at the bottom of the
    // stack, so the drag moves the top lane down onto the one under it.
    change(visual, &state, |state| {
        for name in ["V2", "V3"] {
            state
                .add_track(TrackKind::Video, name)
                .unwrap_or_else(|err| panic!("video lane {name}: {err:?}"));
        }
    });
    settle(visual);
    let rows = |visual: &mut VisualTestContext| -> Vec<u64> {
        visual.read(|app| {
            state
                .read(app)
                .store
                .sequence
                .tracks
                .iter()
                .map(|lane| lane.id)
                .collect()
        })
    };
    let before = rows(visual);
    assert!(
        before.len() >= 3,
        "three video lanes, top first: {before:?}"
    );
    let top = bounds(visual, "track-header-V3");
    let under = bounds(visual, "track-header-V2");
    let off = point(under.right() + px(200.0), under.center().y);

    press(visual, top.center());
    assert!(visual.read(|app| state.read(app).track_dragging()));
    carry(visual, under.center());
    assert_eq!(
        visual.read(|app| state.read(app).track_drop_row),
        Some(1),
        "the header under the top one marks its row"
    );
    carry(visual, off);
    release(visual, off);
    assert!(
        !visual.read(|app| state.read(app).track_dragging()),
        "the release off the headers ends the drag"
    );
    let mut swapped = before.clone();
    swapped.swap(0, 1);
    assert_eq!(
        rows(visual),
        swapped,
        "the release off the headers drops the lane on the marked row"
    );

    hover(visual, top.center());
    assert_eq!(
        rows(visual),
        swapped,
        "a move after the release moved a lane"
    );
}
