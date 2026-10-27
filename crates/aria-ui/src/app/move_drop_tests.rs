//! A clip drag drops its clip where the button comes up.
//!
//! # Why
//!
//! A drag let go over another clip was refused with a generic error, and a
//! drag carried straight up or down to another lane was taken for a click and
//! dropped nothing. The drag sends the overwrite move, which covers what it
//! lands on, and a release on another lane lands there however little the
//! pointer travelled along the lane. A slip or a slide keeps its lane, so the
//! same release with either tool edits nothing. A move carried over a lane
//! whose kind refuses the clip keeps the last lane that holds it, so its
//! release sends no edit the engine refuses; a lane of every kind the model
//! declares is driven. Each case drives the pointer through the drawn shell,
//! and one undo puts a drop back.
//!
//! Not covered: the rule of the cover at every landing tick and its redo,
//! which `aria-engine/tests/overwrite_move.rs` asserts against the commands,
//! and a picture clip carried to the audio side, which no lane there takes
//! from a drag that began on the video side.

use std::path::PathBuf;

use aria_models::{ClipId, TrackId, TrackKind, track_kind_specs};
use gpui::{Entity, Pixels, VisualTestContext, point, px};

use super::held_drag_tests::{bounds, carry, picture_clip, press, release, tick_px};
use crate::app::AppState;
use crate::app::viewer_row_tests::{change, shell_with_take};
use crate::types::{EditorTool, UiTrackKind};

/// `(lane, start, end)` of `clip` as the store draws it.
fn placed(state: &Entity<AppState>, visual: &mut VisualTestContext, clip: u64) -> (u64, i64, i64) {
    visual.read(|app| {
        let clip = state
            .read(app)
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|lane| lane.clips.iter())
            .find(|c| c.id == clip)
            .expect("the clip is on the sequence");
        (
            clip.track_id,
            clip.timeline_start_ticks,
            clip.timeline_end_ticks,
        )
    })
}

/// The window x of `tick` on the lane-group canvas, a quarter of a tick past
/// it so a press reads as that tick whether the canvas rounds or floors.
fn tick_x(state: &Entity<AppState>, visual: &mut VisualTestContext, tick: i64) -> Pixels {
    let canvas = bounds(visual, "timeline-video-canvas");
    let tick_px = tick_px(state, visual);
    let scroll = visual.read(|app| state.read(app).scroll_x);
    canvas.left() + px((tick as f32 + 0.25) * tick_px - scroll)
}

fn set_linked(visual: &mut VisualTestContext, state: &Entity<AppState>, linked: bool) {
    change(visual, state, |state| {
        if state.linked_selection != linked {
            state.toggle_linked_selection();
        }
    });
}

fn undo(visual: &mut VisualTestContext, state: &Entity<AppState>) -> bool {
    let undone = visual.update(|_, app| {
        state.update(app, |state, cx| {
            cx.notify();
            state.undo()
        })
    });
    visual.run_until_parked();
    undone
}

#[gpui::test]
fn a_clip_let_go_over_another_covers_it(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "move-drop-cover");
    let (take, _, take_end) = picture_clip(&state, visual);
    let (lane, ..) = placed(&state, visual, take);
    let still_at = take_end + 30;
    change(visual, &state, |state| {
        let still = state
            .import_media(
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png"),
            )
            .expect("import the still");
        state
            .drop_media_on_timeline(still, None, still_at)
            .expect("place the still");
    });
    let (still, still_len) = visual.read(|app| {
        let clip = state
            .read(app)
            .store
            .sequence
            .tracks
            .iter()
            .filter(|track| track.kind == UiTrackKind::Video)
            .flat_map(|track| track.clips.iter())
            .find(|c| c.timeline_start_ticks == still_at)
            .expect("the still is on the take's lane");
        (clip.id, clip.timeline_end_ticks - clip.timeline_start_ticks)
    });
    let row = bounds(visual, "track-header-V1");
    // Halfway into the take, clear of its edges and of the sequence start.
    let landing = take_end / 2;
    // Into the still's body, clear of its edge and its fade grip.
    let grip = 20;

    for linked in [true, false] {
        set_linked(visual, &state, linked);
        let from = point(
            tick_x(&state, visual, still_at + grip),
            row.top() + px(10.0),
        );
        let to = point(tick_x(&state, visual, landing + grip), from.y);
        press(visual, from);
        carry(visual, point(from.x - px(12.0), from.y));
        carry(visual, to);
        release(visual, to);
        assert_eq!(
            placed(&state, visual, still),
            (lane, landing, landing + still_len),
            "linked selection {linked}: the still let go over the take"
        );
        assert_eq!(
            placed(&state, visual, take),
            (lane, 0, landing),
            "linked selection {linked}: the take the still landed on"
        );

        assert!(undo(visual, &state), "linked selection {linked}: undo");
        assert_eq!(
            placed(&state, visual, still),
            (lane, still_at, still_at + still_len),
            "linked selection {linked}: the still after undo"
        );
        assert_eq!(
            placed(&state, visual, take),
            (lane, 0, take_end),
            "linked selection {linked}: the take after undo"
        );
    }
}

#[gpui::test]
fn a_clip_carried_straight_to_another_lane_lands_there(cx: &mut gpui::TestAppContext) {
    let (state, visual, _) = shell_with_take(cx, "move-drop-lane");
    change(visual, &state, |state| {
        state
            .add_track(TrackKind::Video, "V2")
            .expect("a second video lane");
    });
    let (take, start, end) = picture_clip(&state, visual);
    let (from_lane, ..) = placed(&state, visual, take);
    let to_lane = visual.read(|app| {
        state
            .read(app)
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.kind == UiTrackKind::Video && track.id != from_lane)
            .expect("the second video lane")
            .id
    });
    let (v1, v2) = (
        bounds(visual, "track-header-V1"),
        bounds(visual, "track-header-V2"),
    );

    for linked in [true, false] {
        set_linked(visual, &state, linked);
        let x = tick_x(&state, visual, (start + end) / 2);
        press(visual, point(x, v1.top() + px(10.0)));
        carry(visual, point(x, v2.top() + px(10.0)));
        // A pixel along the lane: under the drag threshold, so no travel.
        let up = point(x + px(1.0), v2.top() + px(10.0));
        carry(visual, up);
        release(visual, up);
        assert_eq!(
            placed(&state, visual, take),
            (to_lane, start, end),
            "linked selection {linked}: the take carried straight up"
        );

        assert!(undo(visual, &state), "linked selection {linked}: undo");
        assert_eq!(
            placed(&state, visual, take),
            (from_lane, start, end),
            "linked selection {linked}: the take after undo"
        );
    }
}

#[gpui::test]
fn a_clip_carried_over_a_lane_that_refuses_it_keeps_the_lane_that_holds_it(
    cx: &mut gpui::TestAppContext,
) {
    let (state, visual, _) = shell_with_take(cx, "move-drop-kind");
    let (take, start, end) = picture_clip(&state, visual);
    let (home, ..) = placed(&state, visual, take);
    // The header of `lane`, by the badge the shell draws on it.
    let header = |visual: &mut VisualTestContext, lane: u64| {
        let badge = visual.read(|app| {
            let state = state.read(app);
            let row = state
                .store
                .sequence
                .tracks
                .iter()
                .position(|track| track.id == lane)
                .expect("the lane is on the sequence");
            state.lane_badge_at(row)
        });
        bounds(visual, &format!("track-header-{badge}"))
    };
    let (mut taken, mut kept) = (0, 0);
    for spec in track_kind_specs() {
        let name = format!("{} extra", spec.default_name);
        change(visual, &state, |state| {
            state
                .add_track(spec.kind, name.clone())
                .unwrap_or_else(|err| panic!("a {:?} lane: {err}", spec.kind));
        });
        let (lane, holds) = visual.read(|app| {
            let state = state.read(app);
            let lane = state
                .store
                .sequence
                .tracks
                .iter()
                .find(|track| track.name == name)
                .expect("the lane is on the sequence")
                .id;
            let engine = state.engine.lock();
            let timeline = engine.project().timeline();
            let clip = timeline.clip(ClipId::from_raw(take)).expect("the take");
            let kind = timeline
                .track(TrackId::from_raw(lane))
                .expect("the lane")
                .kind;
            (lane, kind.accepts_clip(clip))
        });
        let video = bounds(visual, "timeline-video-canvas");
        let over = header(visual, lane).center().y;
        // The lanes a drag begun on the video side can reach.
        let reached = video.top() <= over && over <= video.bottom();
        let to = if holds && reached { lane } else { home };
        if to == lane {
            taken += 1;
        } else {
            kept += 1;
        }

        let x = tick_x(&state, visual, (start + end) / 2);
        let from = point(x, header(visual, home).top() + px(10.0));
        press(visual, from);
        carry(visual, point(x, over));
        let target = visual.read(|app| state.read(app).held_move.as_ref().map(|held| held.track));
        assert_eq!(
            target,
            Some(to),
            "the take held over a {:?} lane",
            spec.kind
        );
        let up = point(x + px(1.0), over);
        carry(visual, up);
        release(visual, up);
        assert_eq!(
            placed(&state, visual, take),
            (to, start, end),
            "the take let go over a {:?} lane",
            spec.kind
        );
        assert_eq!(
            visual.read(|app| state.read(app).last_error.clone()),
            None,
            "the release over a {:?} lane",
            spec.kind
        );
        if to == lane {
            assert!(undo(visual, &state), "{:?}: undo", spec.kind);
        }
        change(visual, &state, |state| {
            state
                .remove_track(TrackId::from_raw(lane))
                .unwrap_or_else(|err| panic!("remove the {:?} lane: {err}", spec.kind));
        });
    }
    assert!(
        taken > 0 && kept > 0,
        "every kind landed one way: {taken} took the take, {kept} kept it home"
    );
}

#[gpui::test]
fn a_slip_or_slide_carried_to_another_lane_edits_nothing(cx: &mut gpui::TestAppContext) {
    // Only a move changes lanes. A slip or a slide carried straight up keeps
    // its clip where it is, so a release with no travel along the lane is a
    // click: no history step and no error.
    let (state, visual, _) = shell_with_take(cx, "move-drop-tool");
    change(visual, &state, |state| {
        state
            .add_track(TrackKind::Video, "V2")
            .expect("a second video lane");
    });
    let (take, start, end) = picture_clip(&state, visual);
    let before = placed(&state, visual, take);
    let (v1, v2) = (
        bounds(visual, "track-header-V1"),
        bounds(visual, "track-header-V2"),
    );
    let steps = visual.read(|app| state.read(app).history_entries().len());

    for tool in [EditorTool::Slip, EditorTool::Slide] {
        change(visual, &state, |state| state.tool = tool);
        let x = tick_x(&state, visual, (start + end) / 2);
        press(visual, point(x, v1.top() + px(10.0)));
        carry(visual, point(x, v2.top() + px(10.0)));
        let up = point(x + px(1.0), v2.top() + px(10.0));
        carry(visual, up);
        release(visual, up);
        visual.read(|app| {
            let state = state.read(app);
            assert_eq!(
                state.history_entries().len(),
                steps,
                "{tool:?}: a release with no travel added a history step"
            );
            assert_eq!(state.last_error, None, "{tool:?}: the release reported");
        });
        assert_eq!(placed(&state, visual, take), before, "{tool:?}: the take");
    }
}
