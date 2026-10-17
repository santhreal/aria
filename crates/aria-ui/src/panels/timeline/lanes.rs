//! Virtualized lane-group rendering for the video and audio timeline zones.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Bounds, Entity, HitboxBehavior, InteractiveElement, IntoElement, ParentElement, RenderImage,
    ScrollDelta, ScrollWheelEvent, Styled, canvas, div, px,
};

use super::COLLAPSED_H;
use super::clip::{self, LaneGroupPaint, VisibleLane};
use super::glide;
use super::headers::track_header;
use super::media::visible_clips_slice;
use crate::app::AppState;
use crate::drag_hint as drop_hint;
use crate::motion;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::types::{TimelinePaintStats, UiEditorStore};

#[allow(clippy::too_many_arguments)]
pub(super) fn lane_group(
    theme: &Theme,
    state: &Entity<AppState>,
    store: &Arc<UiEditorStore>,
    row_range: Range<usize>,
    is_audio: bool,
    is_first_group: bool,
    scroll_y: f32,
    group_viewport_h: f32,
    track_dragging: bool,
    drop_row: Option<usize>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    view_start: i64,
    view_end: i64,
    playhead: f32,
    snap_guide: Option<crate::snap::SnapCandidate>,
    waves: Arc<std::collections::HashMap<u64, Vec<f32>>>,
    strips: Arc<std::collections::HashMap<u64, Vec<(f32, Arc<RenderImage>)>>>,
    reduced: bool,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement + use<> {
    let outline_w = state.read(cx).ui_settings.outline_width;
    let mut y_cursor = -scroll_y;
    let mut visible_lanes = Vec::new();
    let mut headers = Vec::new();
    let mut group_total_clips = 0usize;
    let total_lanes_in_group = row_range.len();

    for row in row_range {
        let track = &store.sequence.tracks[row];
        group_total_clips += track.clips.len();
        let previewed = state.read(cx).lane_preview(track.id);
        let target_h = if track.collapsed {
            COLLAPSED_H
        } else {
            previewed
                .unwrap_or(track.height)
                .max(*aria_models::TrackDisplay::HEIGHT.start() as f32)
        };
        let clip_h = glide::glide(
            motion::MotionKey::new("lane-h", track.id),
            target_h,
            glide::LANE,
            reduced || previewed.is_some(),
        );
        let mut outline_rows = if track.collapsed {
            Vec::new()
        } else {
            state.read(cx).track_outline_rows(track.id)
        };
        // A twirl opens and closes over the lane beat rather than in one step;
        // the rows past the travelling edge are not drawn until it reaches them.
        // The twirl button shows where the rows are going, not where they are.
        let twirled = !outline_rows.is_empty();
        let row_h = crate::app::property_rows::PROPERTY_ROW_H;
        let outline_h = glide::glide(
            motion::MotionKey::new("lane-outline-h", track.id),
            outline_rows.len() as f32 * row_h,
            glide::LANE,
            reduced,
        );
        outline_rows.truncate((outline_h / row_h).ceil() as usize);
        let height = clip_h + outline_h;
        let y_top = y_cursor;
        let clip_y_bottom = y_cursor + clip_h;
        let y_bottom = y_cursor + height;
        y_cursor = y_bottom;
        if y_bottom < 0.0 || y_top > group_viewport_h {
            continue;
        }
        let badge = state.read(cx).lane_badge_at(row);
        headers.push(
            track_header(
                theme,
                track,
                badge,
                row,
                y_top,
                outline_w,
                clip_h,
                target_h,
                track_dragging,
                drop_row,
                cx,
            )
            .into_any_element(),
        );
        if !track.collapsed
            && let Some(first_clip) = track.clips.first()
        {
            headers.push(
                super::property_rows::track_outline_twirl_button(
                    theme,
                    track.id,
                    first_clip.id,
                    twirled,
                    y_top,
                    clip_h,
                    cx,
                )
                .into_any_element(),
            );
        }
        for (idx, outline) in outline_rows.iter().enumerate() {
            let row_y = clip_y_bottom + idx as f32 * row_h;
            let slot = (track.id as usize).wrapping_mul(131).wrapping_add(idx);
            headers.push(
                super::property_rows::outline_row_header(theme, outline, row_y, slot, state, cx)
                    .into_any_element(),
            );
        }
        let band = state
            .read(cx)
            .marquee
            .filter(|_| state.read(cx).marquee_covers_row(row))
            .map(|b| b.ticks());
        let clips = visible_clips_slice(&track.clips, view_start, view_end).to_vec();
        visible_lanes.push(VisibleLane {
            row,
            track_id: track.id,
            y_top,
            clip_y_bottom,
            y_bottom,
            collapsed: track.collapsed,
            locked: track.locked,
            is_drop_row: drop_row == Some(row),
            band,
            drop_tick: drop_hint::lane_tick(track.id),
            clips,
            outline_rows,
        });
    }

    let total_content_h = (y_cursor + scroll_y).max(0.0);
    let (selected_edge, selected_gap, trim_feedback, tool) = {
        let app = state.read(cx);
        (
            app.selected_edge,
            app.selected_gap,
            app.trim_feedback.clone(),
            app.tool,
        )
    };

    let drop_targets: Rc<Vec<(u64, f32, f32)>> = Rc::new(
        visible_lanes
            .iter()
            .map(|l| (l.track_id, l.y_top, l.y_bottom))
            .collect(),
    );
    let shared_lanes = Rc::new(visible_lanes.clone());
    let canvas_bounds = Rc::new(Cell::new(Bounds::default()));
    let bounds_for_canvas = canvas_bounds.clone();
    let badges: clip::FxBadges = Rc::default();
    let badges_for_paint = badges.clone();
    let state_for_prepaint = state.clone();
    let state_for_paint = state.clone();
    let paint_theme = theme.clone();
    let paint_data = LaneGroupPaint {
        is_first_group,
        total_lanes: total_lanes_in_group,
        total_clips: group_total_clips,
        visible_lanes,
        pps,
        fps,
        scroll_x,
        playhead,
        snap_guide,
        selected_edge,
        selected_gap,
        trim_feedback,
        waves,
        strips,
        reduced,
    };

    let group_id = if is_audio {
        "timeline-audio-group"
    } else {
        "timeline-video-group"
    };
    let canvas_id = if is_audio {
        "timeline-audio-canvas"
    } else {
        "timeline-video-canvas"
    };

    let time_area = super::drop::group_target(
        div()
            .flex_1()
            .h_full()
            .relative()
            .overflow_hidden()
            .bg(theme.bg),
        drop_targets,
        scroll_x,
        fps,
        pps,
        theme.accent,
        cx,
    )
    .id(canvas_id)
    .debug_selector(move || canvas_id.into())
    .child(
        canvas(
            move |bounds, window, cx| {
                bounds_for_canvas.set(bounds);
                let w = f32::from(bounds.size.width).max(1.0);
                state_for_prepaint.update(cx, |state, _| {
                    state.timeline_viewport_w = w;
                });
                window.insert_hitbox(bounds, HitboxBehavior::Normal)
            },
            move |bounds, hitbox, window, cx| {
                let mut painted = badges_for_paint.borrow_mut();
                painted.clear();
                let (vis_lanes, vis_clips, calls) = clip::paint_lane_group(
                    bounds,
                    &paint_data,
                    &paint_theme,
                    &mut painted,
                    window,
                    cx,
                );
                // The cursor names what a press under the pointer does.
                if hitbox.is_hovered(window) {
                    let at = window.mouse_position();
                    let cursor = if painted.iter().any(|(_, badge)| badge.contains(&at)) {
                        gpui::CursorStyle::PointingHand
                    } else {
                        let local = at - bounds.origin;
                        clip::finish::cursor_for(
                            tool,
                            clip::finish::hit_test(
                                &paint_data.visible_lanes,
                                f32::from(local.x).max(0.0),
                                f32::from(local.y).max(0.0),
                                fps,
                                pps,
                                scroll_x,
                            ),
                        )
                    };
                    window.set_cursor_style(cursor, &hitbox);
                }
                drop(painted);
                let total_lanes = paint_data.total_lanes;
                let total_clips = paint_data.total_clips;
                let first = paint_data.is_first_group;
                state_for_paint.update(cx, |state, _| {
                    if first {
                        state.timeline_paint_stats = TimelinePaintStats {
                            total_lanes,
                            visible_lanes: vis_lanes,
                            total_clips,
                            visible_clips: vis_clips,
                            canvas_elements: 1,
                            paint_calls: calls,
                        };
                    } else {
                        state.timeline_paint_stats.total_lanes += total_lanes;
                        state.timeline_paint_stats.visible_lanes += vis_lanes;
                        state.timeline_paint_stats.total_clips += total_clips;
                        state.timeline_paint_stats.visible_clips += vis_clips;
                        state.timeline_paint_stats.canvas_elements += 1;
                        state.timeline_paint_stats.paint_calls += calls;
                    }
                });
            },
        )
        .size_full(),
    );

    let time_area = clip::finish::attach_group_mouse_listeners(
        time_area,
        canvas_bounds,
        badges,
        shared_lanes,
        fps,
        pps,
        scroll_x,
        tool,
        cx,
    );
    div()
        .id(group_id)
        .w_full()
        .h_full()
        .flex()
        .flex_row()
        .relative()
        .overflow_hidden()
        .on_scroll_wheel(cx.listener(move |this, event: &ScrollWheelEvent, _, cx| {
            let ctrl_or_alt =
                event.modifiers.control || event.modifiers.platform || event.modifiers.alt;
            if ctrl_or_alt || event.modifiers.shift {
                return;
            }
            let delta = event.delta.pixel_delta(px(24.0));
            let dx = f32::from(delta.x);
            let dy = f32::from(delta.y);
            let trackpad = matches!(event.delta, ScrollDelta::Pixels(_));
            if trackpad && dx.abs() > dy.abs() {
                return;
            }
            if dy.abs() > 0.01 {
                cx.stop_propagation();
                this.state.update(cx, |state, _| {
                    let max_scroll = (total_content_h - group_viewport_h).max(0.0);
                    if is_audio {
                        state.scroll_audio_lanes(-dy, max_scroll);
                    } else {
                        state.scroll_video_lanes(-dy, max_scroll);
                    }
                });
                cx.notify();
            }
        }))
        .child(
            div()
                .w(px(outline_w))
                .h_full()
                .relative()
                .overflow_hidden()
                .border_r_1()
                .border_color(theme.border)
                .bg(theme.surface)
                .children(headers),
        )
        .child(time_area)
}
