//! Virtualized lane-group rendering for the video and audio timeline zones.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    Bounds, Entity, HitboxBehavior, InteractiveElement, IntoElement, ParentElement, ScrollDelta,
    ScrollWheelEvent, Styled, canvas, div, px,
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
use crate::types::{TimelinePaintStats, UiClip, UiEditorStore};

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
    playhead: Option<f32>,
    snap_guide: Option<crate::snap::SnapCandidate>,
    reduced: bool,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement + use<> {
    let (outline_w, has_expanded_clips, layer) = {
        let app = state.read(cx);
        (
            app.outline_drawn_width(),
            !app.expanded_clips.is_empty(),
            app.timeline_playhead.clone(),
        )
    };
    let mut visible_lanes = Vec::new();
    let mut headers = Vec::new();
    let mut group_total_clips = 0usize;
    let total_lanes_in_group = row_range.len();
    let row_h = crate::app::property_rows::PROPERTY_ROW_H;

    // Every lane's height before any is placed: the video lanes stand on the
    // divider, so where the first one starts depends on all of them.
    let mut sized = Vec::with_capacity(row_range.len());
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
        let mut outline_rows = if track.collapsed || !has_expanded_clips {
            Vec::new()
        } else {
            state.read(cx).track_outline_rows(track.id)
        };
        // A twirl opens and closes over the lane beat rather than in one step;
        // the rows past the travelling edge are not drawn until it reaches them.
        // The twirl button shows where the rows are going, not where they are.
        let twirled = !outline_rows.is_empty();
        let outline_h = glide::glide(
            motion::MotionKey::new("lane-outline-h", track.id),
            outline_rows.len() as f32 * row_h,
            glide::LANE,
            reduced,
        );
        outline_rows.truncate((outline_h / row_h).ceil() as usize);
        sized.push(SizedLane {
            row,
            target_h,
            clip_h,
            outline_h,
            twirled,
            outline_rows,
        });
    }
    let total_content_h: f32 = sized.iter().map(|lane| lane.clip_h + lane.outline_h).sum();
    let mut y_cursor = first_lane_top(is_audio, group_viewport_h, total_content_h) - scroll_y;
    // A clip drag in flight draws its clips where the release puts them, and a
    // ripple move draws the line it inserts at.
    let (held, held_edit) = {
        let app = state.read(cx);
        (app.held_move.clone(), app.held_edit.clone())
    };
    let mut held_cards = held
        .as_ref()
        .map(|held| held.cards(store.sequence.frame_rate))
        .unwrap_or_default();

    for SizedLane {
        row,
        target_h,
        clip_h,
        outline_h,
        twirled,
        outline_rows,
    } in sized
    {
        let track = &store.sequence.tracks[row];
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
                track.clips.first().map(|clip| (clip.id, twirled)),
                track_dragging,
                drop_row,
                cx,
            )
            .into_any_element(),
        );
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
        let mut clips: Vec<UiClip> = match held_edit.as_deref() {
            // A shift moves clips from off the view into it, so the lane is
            // read as far past the view as the shifts reach.
            Some(edit) if edit.touches(track.id) => {
                let reach = edit.reach(track.id);
                edit.lane(
                    track.id,
                    visible_clips_slice(
                        &track.clips,
                        view_start.saturating_sub(reach),
                        view_end.saturating_add(reach),
                    ),
                    (view_start, view_end),
                    store.sequence.frame_rate,
                )
            }
            Some(_) | None => visible_clips_slice(&track.clips, view_start, view_end).to_vec(),
        };
        if let Some(held) = &held {
            clips.retain(|clip| !held.takes(clip.id));
            // Last, so each card is drawn over the clips it covers.
            clips.extend(held_cards.extract_if(.., |card| {
                card.track_id == track.id
                    && card.timeline_end_ticks > view_start
                    && card.timeline_start_ticks < view_end
            }));
        }
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
            drop_tick: drop_hint::lane_tick(track.id)
                .or_else(|| held.as_ref().and_then(|held| held.inserts_at(track.id))),
            clips,
            outline_rows,
        });
    }

    #[cfg(test)]
    built::lanes(
        &visible_lanes,
        group_viewport_h,
        built::View {
            ticks: (view_start, view_end),
            scroll_x,
            pps,
            fps,
        },
    );
    // Waveform bars and filmstrip cells are built, and decodes asked for, for
    // the lanes drawn in this group and no others: a lane scrolled out of the
    // viewport costs nothing and takes none of the per-frame budget.
    let drawn_rows: Vec<usize> = visible_lanes.iter().map(|lane| lane.row).collect();
    let waves = Arc::new(super::media::clip_bars(
        state,
        store,
        &drawn_rows,
        held_edit.as_deref(),
        pps,
        fps,
        view_start,
        view_end,
        cx,
    ));
    let strips = Arc::new(super::media::clip_cells(
        state,
        store,
        &drawn_rows,
        held_edit.as_deref(),
        pps,
        fps,
        view_start,
        view_end,
        cx,
    ));
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
        held_edge: held_edit.as_ref().and_then(|edit| edit.edge),
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
            move |bounds, window, _| {
                bounds_for_canvas.set(bounds);
                layer.record_lanes(is_audio, bounds, window);
                #[cfg(test)]
                built::canvas(f32::from(bounds.size.width));
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
                        clip::hit::cursor_for(
                            tool,
                            clip::hit::hit_test(
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
            let delta = event.delta.pixel_delta(px(crate::transport::WHEEL_LINE_PX));
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
                this.redraw(crate::shell::Moved::View, cx);
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

/// One lane's drawn heights, measured before the group places any lane.
struct SizedLane {
    row: usize,
    target_h: f32,
    clip_h: f32,
    outline_h: f32,
    twirled: bool,
    outline_rows: Vec<crate::app::property_rows::TimelineOutlineRow>,
}

/// Where a group's first lane starts in a viewport `viewport_h` tall whose
/// lanes take `content_h`, before scrolling.
///
/// Audio lanes hang from the A/V divider at the top of their side. Video lanes
/// number upward from V1 on the divider, so lanes shorter than their side stand
/// at its bottom and the empty space is above the top lane.
fn first_lane_top(is_audio: bool, viewport_h: f32, content_h: f32) -> f32 {
    if is_audio {
        0.0
    } else {
        (viewport_h - content_h).max(0.0)
    }
}

#[cfg(test)]
#[path = "lanes_anchor_tests.rs"]
mod anchor_tests;

/// What a draw built for each lane group, for a test that holds a lane or a
/// clip outside the viewport to zero work.
#[cfg(test)]
pub(super) mod built {
    use std::cell::RefCell;

    use super::VisibleLane;

    /// One lane a group built.
    #[derive(Debug)]
    pub(crate) struct Lane {
        pub row: usize,
        pub y_top: f32,
        pub y_bottom: f32,
        /// The height of the group's viewport the lane was built in.
        pub viewport_h: f32,
        /// The `[start, end)` ticks of each clip built in the lane.
        pub clips: Vec<(i64, i64)>,
        /// The view the lane was built for.
        pub view: View,
    }

    /// The horizontal view a group was built for.
    #[derive(Debug, Clone, Copy)]
    pub(crate) struct View {
        /// The `[start, end)` ticks the group took as on screen.
        pub ticks: (i64, i64),
        /// The pixel of the sequence at the canvas's left edge.
        pub scroll_x: f32,
        /// Pixels per second.
        pub pps: f32,
        /// Ticks per second.
        pub fps: f32,
    }

    thread_local! {
        static LANES: RefCell<Vec<Lane>> = const { RefCell::new(Vec::new()) };
        static MEDIA_ROWS: RefCell<Vec<usize>> = const { RefCell::new(Vec::new()) };
        static CANVASES: RefCell<Vec<f32>> = const { RefCell::new(Vec::new()) };
    }

    pub(in crate::panels::timeline) fn lanes(lanes: &[VisibleLane], viewport_h: f32, view: View) {
        LANES.with_borrow_mut(|built| {
            built.extend(lanes.iter().map(|lane| {
                Lane {
                    row: lane.row,
                    y_top: lane.y_top,
                    y_bottom: lane.y_bottom,
                    viewport_h,
                    clips: lane
                        .clips
                        .iter()
                        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
                        .collect(),
                    view,
                }
            }))
        });
    }

    /// Records the rows waveform bars or filmstrip cells were built for.
    pub(in crate::panels::timeline) fn media_rows(rows: &[usize]) {
        MEDIA_ROWS.with_borrow_mut(|built| built.extend_from_slice(rows));
    }

    /// Records the width a lane group's canvas was laid out at.
    pub(in crate::panels::timeline) fn canvas(width: f32) {
        CANVASES.with_borrow_mut(|laid| laid.push(width));
    }

    /// The widths lane group canvases were laid out at since the last call.
    pub(crate) fn take_canvases() -> Vec<f32> {
        CANVASES.with_borrow_mut(std::mem::take)
    }

    /// The lanes, and the rows given waveform bars or filmstrip cells, built
    /// since the last call.
    pub(crate) fn take() -> (Vec<Lane>, Vec<usize>) {
        (
            LANES.with_borrow_mut(std::mem::take),
            MEDIA_ROWS.with_borrow_mut(std::mem::take),
        )
    }
}
