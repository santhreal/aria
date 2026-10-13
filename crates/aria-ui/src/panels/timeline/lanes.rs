//! Lane bodies: clips, the rubber band, and collapsed summaries.

use crate::fluent::FluentBuilder;
use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use aria_models::TrackId;

use super::clip as timeline_clip;
use crate::motion;
use crate::theme::Theme;
use crate::types::{CatalogDrag, UiTrack};

use super::glide;
use super::headers::track_header;
use super::ruler::{playhead_line, snap_glow, snap_guide_line, transition_wedge};
use super::{COLLAPSED_H, px_to_ticks, ticks_to_px};
use crate::drag_hint::{self as drop_hint, DropHint};

pub(super) fn lane(
    theme: &Theme,
    track: &UiTrack,
    row: usize,
    dragging: bool,
    drop_row: Option<usize>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    view_start: i64,
    view_end: i64,
    // The panel's measured width. The right edge fades only while the sequence
    // runs past what is on screen, so a guess here would soften an edge with
    // nothing beyond it.
    viewport: f32,
    playhead: f32,
    snap_guide: Option<crate::snap::SnapCandidate>,
    // The rubber band's tick span, when it covers this lane.
    band: Option<(i64, i64)>,
    expanded: &[u64],
    waves: &std::collections::HashMap<u64, Vec<f32>>,
    strips: &std::collections::HashMap<u64, Vec<(f32, std::sync::Arc<gpui::RenderImage>)>>,
    // A resize still under the pointer draws where the pointer is, so the height
    // must not lag it; a collapse or a release is free to travel.
    preview_height: Option<f32>,
    reduced: bool,
    // The clip an edit has just created, if it is still settling.
    landing: Option<u64>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // A collapsed lane draws one flat bar per clip instead of a card: the cut
    // pattern is what a collapsed lane is for, and a 20px card would only be an
    // unreadable label.
    let clips: Vec<_> = if track.collapsed {
        track
            .clips
            .iter()
            .filter(|clip| {
                clip.timeline_end_ticks > view_start && clip.timeline_start_ticks < view_end
            })
            .map(|clip| {
                // A collapsed lane's bars replace the cards that were there a
                // moment ago, so they arrive rather than appear.
                motion::fade_in(
                    motion::MotionKey::new("summary", clip.id),
                    summary_bar(theme, clip, pps, fps, scroll_x, reduced),
                )
                .into_any_element()
            })
            .collect()
    } else {
        track
            .clips
            .iter()
            .filter(|clip| {
                clip.timeline_end_ticks > view_start && clip.timeline_start_ticks < view_end
            })
            .map(|clip| {
                let is_expanded = expanded.contains(&clip.id);
                let bars = waves.get(&clip.id).cloned().unwrap_or_default();
                let cells = strips.get(&clip.id).cloned().unwrap_or_default();
                let settling = landing == Some(clip.id);
                timeline_clip::render(
                    theme,
                    clip,
                    pps,
                    fps,
                    scroll_x,
                    is_expanded,
                    reduced,
                    settling,
                    bars,
                    cells,
                    cx,
                )
                .into_any_element()
            })
            .collect()
    };
    let wedges: Vec<_> = track
        .clips
        .iter()
        .filter_map(|clip| {
            let transition = clip.transition.as_ref()?;
            Some(
                transition_wedge(
                    theme,
                    clip.id,
                    clip.timeline_end_ticks,
                    transition.duration_ticks,
                    transition.label.clone(),
                    pps,
                    fps,
                    scroll_x,
                    cx,
                )
                .into_any_element(),
            )
        })
        .collect();
    let collapsed = track.collapsed;
    let target_h = if collapsed {
        COLLAPSED_H
    } else {
        preview_height.unwrap_or(track.height).max(28.0)
    };
    let lane_id = track.id;
    // The drawn height travels; the height a resize commits does not. A collapse
    // and an expand are the same one value moving between its two ends, so both
    // directions animate from the same key.
    let height = glide::glide(
        motion::MotionKey::new("lane-h", lane_id),
        target_h,
        glide::LANE,
        reduced || preview_height.is_some(),
    );
    // The magnet breathes only while it holds something.
    let glow = snap_guide.map(|_| snap_glow(cx)).unwrap_or_default();
    // A pool or catalog drag over this lane draws where its clip would land.
    let drop_tick = drop_hint::lane_tick(lane_id);
    let fade_left = scroll_x > 0.0;
    let sequence_px = track
        .clips
        .iter()
        .map(|clip| ticks_to_px(clip.timeline_end_ticks, fps, pps))
        .fold(0.0f32, f32::max);
    let fade_right = sequence_px > scroll_x + viewport;
    div()
        .h(px(height))
        .w_full()
        .flex()
        .flex_row()
        .relative()
        .border_b_1()
        .border_color(theme.border)
        // The lower border is the grab handle: 5px of it, over the border line,
        // so the lane resizes from the edge the operator can already see. A
        // collapsed lane has no height to drag — expanding restores the one it
        // was given.
        .when(!collapsed, |el| {
            let grip = motion::MotionKey::new("lane-grip", lane_id);
            el.child(
                div()
                    .id(("lane-grip", lane_id))
                    .absolute()
                    .bottom(px(-2.))
                    .left_0()
                    .right_0()
                    .h(px(5.))
                    .cursor(gpui::CursorStyle::ResizeUpDown)
                    // The grip is invisible until the pointer is near it, and it
                    // arrives rather than blinks on.
                    .bg(motion::hover_blend(
                        grip,
                        theme.border_strong.opacity(0.0),
                        theme.border_strong,
                    ))
                    .on_hover(motion::hover_listener(grip))
                    .on_mouse_down(
                        gpui::MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            this.start_lane_resize(lane_id, f32::from(event.position.y), target_h);
                            cx.notify();
                        }),
                    ),
            )
        })
        .child(track_header(theme, track, row, dragging, drop_row, cx))
        .child({
            let track_id = track.id;
            let accent = theme.accent;
            let area = div()
                .flex_1()
                .h_full()
                .relative()
                .overflow_hidden()
                .bg(theme.bg)
                .children(clips)
                .children(wedges)
                .child(playhead_line(theme, playhead, pps, fps, scroll_x))
                .when_some(snap_guide, |el, guide| {
                    el.child(snap_guide_line(theme, guide, pps, fps, scroll_x, glow))
                })
                // Where the drag in flight would put its clip. The line fades in
                // on the frame the lane is entered, so a drag that crosses three
                // lanes does not flicker a hard bar on each.
                .when_some(drop_tick, |el, tick| {
                    el.child(motion::fade_quick(
                        motion::MotionKey::new("lane-drop", lane_id),
                        insertion_line(theme, tick, pps, fps, scroll_x),
                    ))
                })
                // A drop listener is not given the pointer, so the tick is captured
                // while the drag moves over this lane.
                .on_drag_move(cx.listener(
                    move |this, event: &gpui::DragMoveEvent<CatalogDrag>, _, cx| {
                        let x = f32::from(event.event.position.x - event.bounds.origin.x);
                        let tick = px_to_ticks(x + scroll_x, fps, pps);
                        this.set_catalog_drop(track_id, tick);
                        if drop_hint::set(DropHint::Lane {
                            track: track_id,
                            tick,
                        }) {
                            cx.notify();
                        }
                    },
                ))
                .drag_over::<CatalogDrag>(move |style, _, _, _| style.bg(accent.opacity(0.12)))
                .on_drop(cx.listener(move |this, drag: &CatalogDrag, _, cx| {
                    drop_hint::clear();
                    let target = this.take_catalog_drop();
                    let tick = target
                        .filter(|(id, _)| *id == track_id)
                        .map_or(0, |(_, tick)| tick);
                    this.state.update(cx, |state, _| {
                        if let Err(err) = state.drop_catalog_on_track(
                            drag,
                            Some(TrackId::from_raw(track_id)),
                            tick,
                        ) {
                            state.report_error(&err);
                        }
                    });
                    crate::app::spawn_preview(this.state.clone(), cx);
                    cx.notify();
                }))
                // Empty lane space is the rubber band's surface: a clip card stops
                // propagation, so a press that reaches here is a press on nothing.
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        if !this.state.read(cx).tool.selects() {
                            return;
                        }
                        let Some(left) = this.state.read(cx).timeline_content_left() else {
                            return;
                        };
                        let tick = super::pointer_ticks(event.position.x, left, scroll_x, fps, pps);
                        this.state.update(cx, |state, _| {
                            state.clear_selection();
                            state.begin_marquee(track_id, tick);
                        });
                        cx.notify();
                    }),
                )
                .on_mouse_move(
                    cx.listener(move |this, event: &gpui::MouseMoveEvent, _, cx| {
                        if this.state.read(cx).marquee.is_none() {
                            return;
                        }
                        let Some(left) = this.state.read(cx).timeline_content_left() else {
                            return;
                        };
                        let tick = super::pointer_ticks(event.position.x, left, scroll_x, fps, pps);
                        let changed = this
                            .state
                            .update(cx, |state, _| state.drag_marquee(track_id, tick));
                        if changed {
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _: &gpui::MouseUpEvent, _, cx| {
                        if this.state.read(cx).marquee.is_none() {
                            return;
                        }
                        this.state.update(cx, |state, _| {
                            let count = state.commit_marquee();
                            if count > 0 {
                                state.status_message = Some(format!("Selected {count} clip(s)"));
                            }
                        });
                        cx.notify();
                    }),
                )
                .when_some(band, |el, (low, high)| {
                    el.child(marquee_band(theme, low, high, pps, fps, scroll_x))
                });
            // Dragged pool media lands on the lane it was released over, which
            // is this one: the rule lives beside the edge strips it contrasts
            // with rather than in this builder.
            let target = super::drop::lane_target(area, track_id, scroll_x, fps, pps, accent, cx);
            // Clips are drawn at an offset rather than scrolled, so the fade is
            // told what the offset means: soft on the left once the view has
            // moved off zero, soft on the right while this lane still has clip
            // beyond the view.
            crate::edge_fade::edge_faded(Theme::SPACE_LG, false, false, target)
                .fade_left(fade_left)
                .fade_right(fade_right)
        })
}

/// Where the drag in flight would put its clip: the tick it would start at.
///
/// A hairline plus a cap at the top, so it reads as an insertion point rather
/// than one more magnet guide — the guide says what a drag locked onto, this
/// says where a drop would land.
pub(super) fn insertion_line(
    theme: &Theme,
    tick: i64,
    pps: f32,
    fps: f32,
    scroll_x: f32,
) -> gpui::Div {
    let x = ticks_to_px(tick, fps, pps) - scroll_x;
    div()
        .absolute()
        .left(px(x))
        .top_0()
        .bottom_0()
        .w(px(2.))
        // A drop cursor, not a playhead: same shape, different promise, so it
        // carries the cursor token rather than the accent.
        .bg(theme.cursor)
        .child(
            div()
                .absolute()
                .left(px(-Theme::SPACE_XS / 2.0))
                .top_0()
                .w(px(Theme::SPACE_SM))
                .h(px(Theme::SPACE_XS))
                .rounded(px(1.))
                .bg(theme.accent),
        )
}

/// The rubber band, drawn on every lane it covers.
pub(super) fn marquee_band(
    theme: &Theme,
    low: i64,
    high: i64,
    pps: f32,
    fps: f32,
    scroll_x: f32,
) -> impl IntoElement {
    let left = ticks_to_px(low, fps, pps) - scroll_x;
    let width = ticks_to_px((high - low).max(0), fps, pps);
    div()
        .absolute()
        .left(px(left))
        .top_0()
        .bottom_0()
        .w(px(width.max(1.0)))
        .bg(theme.selection)
        .border_1()
        .border_color(theme.accent)
}

/// One clip on a collapsed lane: its span and its colour, nothing else.
///
/// The colour comes from `timeline_clip::tint`, so a labelled clip reads the
/// same collapsed as it does expanded.
pub(super) fn summary_bar(
    theme: &Theme,
    clip: &crate::types::UiClip,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    reduced: bool,
) -> gpui::Div {
    let span = (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1);
    let left = (clip.timeline_start_ticks as f32 / fps) * pps - scroll_x;
    let width = ((span as f32 / fps) * pps).max(2.0);
    // The ring strengthens instead of snapping on, the same way the card's does.
    let ring = glide::glide(
        motion::MotionKey::new("summary-ring", clip.id),
        if clip.selected { 1.0 } else { 0.0 },
        glide::EDIT,
        reduced,
    );
    div()
        .absolute()
        .left(px(left))
        .top(px(4.))
        .h(px(COLLAPSED_H - 8.0))
        .w(px(width))
        .rounded(px(2.))
        .bg(timeline_clip::tint(theme, clip))
        .border_1()
        .border_color(theme.text.opacity(ring))
}
