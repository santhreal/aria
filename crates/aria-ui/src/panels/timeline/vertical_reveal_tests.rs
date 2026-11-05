//! Vertical reveal on the timeline's bottom-anchored video side.
//!
//! WHY: The video side numbers upward from `V1` on the A/V divider, so its
//! overflow extends above the top of the side while `video_scroll_y` is `0.0`.
//! Without vertical reveal, two classes of action put the target lane above the
//! top edge of the video viewport:
//! 1. Creating a visual lane above an overflowing video side (such as the Type
//!    tool creating `T2` above `T1`, or adding a video/adjustment/sticker/effect
//!    lane) leaves the new lane and its clip cut off above the ruler until a
//!    manual wheel scroll.
//! 2. Expanding a clip's property outline or property groups on the
//!    bottom-anchored video side grows `content_h` upward from the divider,
//!    pushing the lane's header above the top of the viewport so a second press
//!    on the header's chevron lands on a property row instead.
//!
//! This suite tests the invariant across every track kind in
//! [`aria_models::track_kind_specs`], every [`PropertyGroup`] in
//! [`PropertyGroup::ALL`], [`RevealAction`] chords, lane collapse/expand, and
//! drawn GPUI shell bounds: after creating a visual lane or toggling a lane's
//! outline or collapse state, the lane's header stands inside `[0.0, viewport_h]`
//! (at `0.0` when the expanded lane exceeds `viewport_h`).
//!
//! Not caught: intermediate frames of the 180 ms lane glide when reduced motion
//! is off, where `first_lane_top` clamps `video_scroll_y` to each frame's
//! intermediate overflow until the glide settles.

use std::path::PathBuf;

use aria_models::{ClipId, Generator, TrackDisplay, TrackKind};
use gpui::{Bounds, Pixels, VisualTestContext, px};

use crate::app::AppState;
use crate::app::premiere_tests::{place_media, with_clip};
use crate::app::property_rows::{GRAPH_EDITOR_H, PROPERTY_ROW_H, PropertyGroup, RevealAction};
use crate::theme::Theme;

const EPS: f32 = 0.51;
const VIDEO_SIDE: &str = "timeline-video-canvas";

/// Compute `(y_v_top, y_v_header_bottom, y_v_bottom, viewport_h)` for visual
/// lane `track_id` in the video side's viewport, using the panel's frame,
/// graph editor state, A/V split, and lane heights.
fn lane_viewport_span(state: &AppState, track_id: u64) -> (f32, f32, f32, f32) {
    let panel_h = state.timeline_frame.map(|f| f[3]).unwrap_or(320.0);
    let graph_h = if state.timeline_graph_open() {
        GRAPH_EDITOR_H
    } else {
        0.0
    };
    let stack_h = (panel_h
        - super::RULER_H
        - 2.0 * Theme::SPACE_SM
        - super::AV_DIVIDER_H
        - graph_h
        - super::overview::OVERVIEW_H)
        .max(0.0);
    let total_rows = state.store.sequence.tracks.len();
    let visual_rows = state.visual_lane_count();
    let (video_side, _) = super::sides::sides(
        visual_rows,
        total_rows.saturating_sub(visual_rows),
        state.timeline_av_split,
    );
    let viewport_h = video_side.viewport(stack_h);

    let row = state.store.sequence.tracks[..visual_rows]
        .iter()
        .position(|t| t.id == track_id)
        .expect("visual lane exists");

    let mut y_top = 0.0;
    let mut clip_h_target = 0.0;
    let mut y_bottom = 0.0;
    let mut content_h = 0.0;
    for (idx, track) in state.store.sequence.tracks[..visual_rows]
        .iter()
        .enumerate()
    {
        let clip_h = if track.collapsed {
            super::COLLAPSED_H
        } else {
            state
                .lane_preview(track.id)
                .unwrap_or(track.height)
                .max(*TrackDisplay::HEIGHT.start() as f32)
        };
        let outline_h = if track.collapsed || state.expanded_clips.is_empty() {
            0.0
        } else {
            state.track_outline_rows(track.id).len() as f32 * PROPERTY_ROW_H
        };
        let h = clip_h + outline_h;
        if idx == row {
            y_top = content_h;
            clip_h_target = clip_h;
            y_bottom = content_h + h;
        }
        content_h += h;
    }

    let max_scroll = (content_h - viewport_h).max(0.0);
    let scroll = state.video_scroll_y.clamp(0.0, max_scroll);
    let first_top = viewport_h - content_h + scroll;
    let y_v_top = first_top + y_top;
    (
        y_v_top,
        y_v_top + clip_h_target,
        first_top + y_bottom,
        viewport_h,
    )
}

#[test]
fn every_visual_lane_kind_created_on_an_overflowing_side_stands_in_the_viewport() {
    for (frame, with_audio, with_graph, split, pre_lanes) in [
        (None, true, false, 0.55, 6usize),
        (Some([0.0, 0.0, 1200.0, 260.0]), false, false, 0.55, 6),
        // With the graph editor open (`GRAPH_EDITOR_H = 168`), 2 pre-existing
        // lanes + 1 new lane (`144 px`) overflow the `90.75 px` video side
        // even though they would fit if `GRAPH_EDITOR_H` were ignored (`183.15 px`).
        (Some([0.0, 0.0, 1200.0, 420.0]), true, true, 0.55, 2),
    ] {
        for spec in aria_models::track_kind_specs() {
            let (mut state, _clip) = with_clip();
            if let Some(f) = frame {
                state.record_timeline_frame(f);
            }
            state.set_timeline_av_split(split);
            if with_audio {
                state
                    .add_track(TrackKind::Audio, "A1")
                    .expect("add audio track");
            }
            if with_graph {
                state.toggle_timeline_graph_editor();
            }
            for n in 2..=pre_lanes {
                state
                    .add_track(TrackKind::Video, format!("V{n}"))
                    .expect("add video track");
            }

            if spec.is_audio || spec.is_bus {
                state.video_scroll_y = 19.0;
                state
                    .add_lane_of_kind(spec.kind, None)
                    .expect("add audio or bus lane");
                assert_eq!(
                    state.video_scroll_y, 19.0,
                    "adding non-visual lane {} must not disturb video_scroll_y",
                    spec.id
                );
                continue;
            }

            // 1. From rest (`video_scroll_y = 0.0`), a newly added visual lane
            // lands above V1 and must be scrolled into `[0.0, viewport_h]`.
            state.video_scroll_y = 0.0;
            let track_id = state
                .add_lane_of_kind(spec.kind, None)
                .expect("add visual lane")
                .raw();
            assert!(
                state.video_scroll_y > 0.0,
                "kind {} (frame={frame:?}, audio={with_audio}, graph={with_graph}): \
                 overflowing side must raise video_scroll_y above 0.0",
                spec.id
            );
            let (y_top, y_header_bottom, y_bottom, viewport_h) =
                lane_viewport_span(&state, track_id);
            assert!(
                y_top >= -EPS && y_bottom <= viewport_h + EPS,
                "kind {} (frame={frame:?}, audio={with_audio}, graph={with_graph}): \
                 new lane span [{y_top:.1}, {y_bottom:.1}] (header bottom {y_header_bottom:.1}) \
                 outside viewport [0.0, {viewport_h:.1}] (video_scroll_y={:.1})",
                spec.id,
                state.video_scroll_y
            );
        }
    }

    // 2. Type tool / generator placement creating a new Text lane above an
    // existing occupied Text lane on an overflowing video side reveals the new
    // top lane.
    let (mut state, _clip) = with_clip();
    state.record_timeline_frame([0.0, 0.0, 1200.0, 260.0]);
    for n in 2..=5 {
        state
            .add_track(TrackKind::Video, format!("V{n}"))
            .expect("add video track");
    }
    let (first_title, _) = state
        .place_generated(Generator::text("Title 1"), None, 0)
        .expect("first title on T1");
    let t1 = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == first_title.raw()))
        .expect("T1 holds first title")
        .id;
    state.video_scroll_y = 0.0;
    let (second_title, _) = state
        .place_generated(Generator::text("Title 2"), None, 0)
        .expect("second title at same tick creates T2 above T1");
    let t2 = state
        .store
        .sequence
        .tracks
        .iter()
        .find(|t| t.clips.iter().any(|c| c.id == second_title.raw()))
        .expect("T2 holds second title")
        .id;
    assert_ne!(t1, t2, "occupied T1 forces a second Text lane T2");
    let (y_top, _, y_bottom, viewport_h) = lane_viewport_span(&state, t2);
    assert!(
        (y_top - 0.0).abs() <= EPS && y_bottom <= viewport_h + EPS,
        "T2 created by place_generated must stand at top of viewport: \
         [{y_top:.1}, {y_bottom:.1}] in [0.0, {viewport_h:.1}]"
    );

    // 3. When scrolled all the way to the top (`video_scroll_y = max_scroll`),
    // creating a visual lane near the bottom of the video side lowers
    // `video_scroll_y` so the new lane's bottom edge fits inside `viewport_h`.
    state.video_scroll_y = 10_000.0;
    let visual_rows = state.visual_lane_count();
    let low_lane = state
        .add_lane_of_kind(TrackKind::Video, Some(visual_rows))
        .expect("add video lane at bottom of visual stack")
        .raw();
    let (y_top, _, y_bottom, viewport_h) = lane_viewport_span(&state, low_lane);
    assert!(
        y_top >= -EPS && (y_bottom - viewport_h).abs() <= EPS,
        "bottom visual lane added while scrolled to top must lower scroll into view: \
         [{y_top:.1}, {y_bottom:.1}] in [0.0, {viewport_h:.1}]"
    );
}

#[test]
fn toggling_outline_rows_and_collapse_state_keeps_the_lane_header_in_the_viewport() {
    let (mut state, v1_clip) = with_clip();
    state.record_timeline_frame([0.0, 0.0, 1200.0, 260.0]);
    state.add_track(TrackKind::Audio, "A1").expect("add A1");

    let v1_track = state.store.sequence.tracks[0].id;
    for _ in 2..=4 {
        state
            .add_lane_of_kind(TrackKind::Video, None)
            .expect("add video lane above V1");
    }
    assert_eq!(state.lane_badge_at(3), "V1");
    // `tracks[0]` is the topmost video lane (`V4`), which sits above the
    // viewport when `video_scroll_y == 0.0`.
    let top_track = state.store.sequence.tracks[0].id;
    assert_eq!(state.lane_badge_at(0), "V4");
    let media_id = aria_models::MediaId::from_raw(state.store.media[0].id);
    state
        .drop_media_on_timeline(media_id, Some(aria_models::TrackId::from_raw(top_track)), 0)
        .expect("drop clip on top video lane");
    let top_clip = ClipId::from_raw(
        state
            .store
            .sequence
            .tracks
            .iter()
            .find(|t| t.id == top_track)
            .and_then(|t| t.clips.first())
            .expect("clip on top video lane")
            .id,
    );

    // 1. Expanding V1's outline on the bottom-anchored video side makes V1 +
    // its outline rows taller than the video side: V1's header must stand at
    // the top of the viewport (`y_v_top == 0.0`), not pushed above it.
    state.video_scroll_y = 0.0;
    state.toggle_clip_outline(v1_clip.raw());
    let (y_top, y_header_bottom, y_bottom, viewport_h) = lane_viewport_span(&state, v1_track);
    assert!(
        y_bottom - y_top > viewport_h,
        "expanded V1 ({:.1} px) must exceed viewport ({viewport_h:.1} px) in this fixture",
        y_bottom - y_top
    );
    assert!(
        (y_top - 0.0).abs() <= EPS && y_header_bottom <= viewport_h + EPS,
        "expanding V1 must place V1's header at viewport top: \
         y_top={y_top:.1}, header_bottom={y_header_bottom:.1}, viewport_h={viewport_h:.1}"
    );

    // Closing V1's outline a second time clamps `video_scroll_y` back so V1
    // rests on the A/V divider again.
    state.toggle_clip_outline(v1_clip.raw());
    assert_eq!(state.video_scroll_y, 0.0);
    let (y_top, _, y_bottom, viewport_h) = lane_viewport_span(&state, v1_track);
    assert!(
        y_top >= -EPS && (y_bottom - viewport_h).abs() <= EPS,
        "closing V1 outline must return V1 to the divider: [{y_top:.1}, {y_bottom:.1}] vs {viewport_h:.1}"
    );

    // 2. Toggling every PropertyGroup on V1 keeps the lane's header in the
    // viewport (`video_scroll_y > 0.0` from rest at `0.0`, since 48 + 96 = 144 px
    // already exceeds the 95.15 px video side at panel_h = 260).
    for group in PropertyGroup::ALL {
        state.video_scroll_y = 0.0;
        state.toggle_property_group(v1_clip, group);
        assert!(
            state.video_scroll_y > 0.0,
            "toggling {group:?} on V1 in a 95.15 px side must scroll V1 header into view"
        );
        let (y_top, y_header_bottom, _, viewport_h) = lane_viewport_span(&state, v1_track);
        assert!(
            (y_top - 0.0).abs() <= EPS && y_header_bottom <= viewport_h + EPS,
            "toggling {group:?} on V1 left header [{y_top:.1}, {y_header_bottom:.1}] \
             outside [0.0, {viewport_h:.1}]"
        );
    }

    // 3. When an active resize preview (`set_lane_preview`) on V1 makes the
    // stack overflow a tall viewport where the un-previewed stack would fit,
    // revealing `top_track` accounts for the previewed height.
    state.reveal_properties(RevealAction::CloseAll, false);
    state.record_timeline_frame([0.0, 0.0, 1200.0, 500.0]);
    // At panel_h = 500.0, stack_h = 413.0, viewport_h = 227.15 px.
    // 4 lanes * 48 = 192 px fits (< 227.15), but with V1 previewed at 160 px
    // total content is 3 * 48 + 160 = 304 px (> 227.15).
    state.set_lane_preview(v1_track, 160.0);
    state.video_scroll_y = 0.0;
    state
        .set_lane_collapsed(top_track, false)
        .expect("reveal top_track while V1 has active lane_preview");
    assert!(
        state.video_scroll_y > 0.0,
        "active lane_preview on V1 must be included in overflow calculation"
    );
    let (y_top, _, y_bottom, viewport_h) = lane_viewport_span(&state, top_track);
    assert!(
        (y_top - 0.0).abs() <= EPS && y_bottom <= viewport_h + EPS,
        "top_track with V1 lane_preview active must stand at viewport top: \
         [{y_top:.1}, {y_bottom:.1}] in [0.0, {viewport_h:.1}]"
    );
    state.take_lane_preview();

    // 4. `reveal_properties` (Alt+P/S/R/T/A/E/M/L/U and CloseAll) reveals the
    // targeted clip's lane when scrolled out of view and clamps scroll on CloseAll.
    state.record_timeline_frame([0.0, 0.0, 1200.0, 260.0]);
    state.select_clip(top_clip);
    for action in [
        RevealAction::Position,
        RevealAction::Scale,
        RevealAction::Rotation,
        RevealAction::Opacity,
        RevealAction::Anchor,
    ] {
        state.video_scroll_y = 0.0;
        state.reveal_properties(action, false);
        assert!(
            state.video_scroll_y > 0.0,
            "reveal_properties({action:?}) on top_clip must raise video_scroll_y above 0.0"
        );
        let (y_top, y_header_bottom, y_bottom, viewport_h) = lane_viewport_span(&state, top_track);
        assert!(
            y_top >= -EPS && y_header_bottom <= viewport_h + EPS && y_bottom <= viewport_h + EPS,
            "reveal_properties({action:?}) on top_track must bring it into [0.0, {viewport_h:.1}]: \
             [{y_top:.1}, {y_bottom:.1}]"
        );
    }
    state.toggle_clip_outline(v1_clip.raw());
    state.video_scroll_y = 500.0;
    state.reveal_properties(RevealAction::CloseAll, false);
    // With 4 lanes of 48 px (`192 px`) in a `90.2 px` viewport (`stack_h = 164`),
    // `max_scroll` is `101.8 px`, so `video_scroll_y` must be clamped from `500.0`
    // to `max_scroll`.
    assert!(
        (state.video_scroll_y - 101.8).abs() <= EPS,
        "CloseAll must clamp stale video_scroll_y to 101.8 (got {:.1})",
        state.video_scroll_y
    );

    // 5. Collapsing and expanding a top visual lane via `set_lane_collapsed` /
    // `toggle_lane_collapsed` reveals the lane in the viewport.
    state.video_scroll_y = 0.0;
    state
        .set_lane_collapsed(top_track, true)
        .expect("collapse top visual lane");
    assert!(
        state.video_scroll_y > 0.0,
        "collapsing top_track while scrolled to 0.0 must reveal top_track"
    );
    let (y_top, _, y_bottom, viewport_h) = lane_viewport_span(&state, top_track);
    assert!(
        (y_top - 0.0).abs() <= EPS
            && (y_bottom - super::COLLAPSED_H).abs() <= EPS
            && y_bottom <= viewport_h + EPS,
        "collapsing top lane must reveal its 20 px strip in viewport: [{y_top:.1}, {y_bottom:.1}]"
    );
    state.video_scroll_y = 0.0;
    state
        .toggle_lane_collapsed(top_track)
        .expect("expand top visual lane");
    let (y_top, _, y_bottom, viewport_h) = lane_viewport_span(&state, top_track);
    assert!(
        (y_top - 0.0).abs() <= EPS && y_bottom <= viewport_h + EPS,
        "expanding top lane must reveal its full height in viewport: [{y_top:.1}, {y_bottom:.1}]"
    );
}

fn bounds(visual: &mut VisualTestContext, selector: &str) -> Option<Bounds<Pixels>> {
    visual.debug_bounds(crate::interned::intern(selector))
}

fn redraw(
    visual: &mut VisualTestContext,
    state: &gpui::Entity<AppState>,
    edit: impl FnOnce(&mut AppState),
) {
    visual.update(|window, app| {
        state.update(app, |state, _| edit(state));
        window.refresh();
    });
    visual.run_until_parked();
}

#[gpui::test]
fn drawn_shell_reveals_a_new_title_lane_and_an_expanded_v1_header(cx: &mut gpui::TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = crate::test_support::drawn_shell(cx, "vertical-reveal-drawn");
    visual.update(|_, app| app.set_reduce_motion(true));

    let mut v1_clip = ClipId::from_raw(0);
    redraw(visual, &state, |state| {
        let fixture =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
        v1_clip = place_media(state, fixture);
        state.add_track(TrackKind::Audio, "A1").expect("add A1");
        state.set_timeline_av_split(0.35);
        for _ in 2..=5 {
            state
                .add_lane_of_kind(TrackKind::Video, None)
                .expect("add video lane above V1");
        }
        state
            .place_generated(Generator::text("Title 1"), None, 0)
            .expect("place first title on T1");
        state.video_scroll_y = 0.0;
    });
    // Second draw after the first frame records `timeline_frame`.
    redraw(visual, &state, |state| {
        state.video_scroll_y = 0.0;
        state
            .place_generated(Generator::text("Title 2"), None, 0)
            .expect("place second title on new T2 lane");
    });

    let video_canvas = bounds(visual, VIDEO_SIDE).expect("video side canvas is drawn");
    let t2_header = bounds(visual, "track-header-T2")
        .expect("newly created T2 header must be drawn inside the video side");
    assert!(
        t2_header.top() >= video_canvas.top() - px(EPS)
            && t2_header.bottom() <= video_canvas.bottom() + px(EPS),
        "T2 header {:?} must stand inside video side {:?}",
        t2_header,
        video_canvas
    );

    // Return to bottom-anchored rest (`video_scroll_y = 0.0`), then toggle V1's
    // outline open: V1's header must stand at the top of the video side rather
    // than being pushed above `video_canvas.top()`.
    redraw(visual, &state, |state| {
        state.video_scroll_y = 0.0;
        state.toggle_clip_outline(v1_clip.raw());
    });
    let video_canvas = bounds(visual, VIDEO_SIDE).expect("video side canvas is drawn");
    let v1_header = bounds(visual, "track-header-V1")
        .expect("expanded V1 header must stay drawn inside the video side");
    assert!(
        (f32::from(v1_header.top()) - f32::from(video_canvas.top())).abs() <= EPS
            && v1_header.bottom() <= video_canvas.bottom() + px(EPS),
        "expanded V1 header {:?} must stand at the top of video side {:?}",
        v1_header,
        video_canvas
    );
}
