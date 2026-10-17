//! Landing dragged pool media on the timeline.
//!
//! A pool row used to place its media on whichever lane of the right kind came
//! first, at the playhead, however far from there the pointer was. A drag now
//! names both: the lane it is released over, and the tick under the pointer.
//!
//! Two targets, and the difference between them is the whole rule:
//!
//! - A **lane** takes the clip onto itself. It never creates a lane, so a drop
//!   that misses by a few pixels cannot grow the sequence.
//! - An **edge strip**, above the first lane and below the last, is the only
//!   media drop that adds one.
//!
//! The tick is captured on drag-move rather than read on drop, because a drop
//! listener is not given the pointer. The magnet applies exactly as it does to a
//! clip drag: a drop lands on a cut, a marker, or the playhead when it is close
//! enough to one.

use crate::fluent::FluentBuilder;
use gpui::{Div, InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::drag_hint::{self as drop_hint, DropHint};
use crate::motion;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::types::MediaDrag;

/// How near a magnet a dropped clip has to land to be taken by it, in pixels.
/// The same figure a clip drag uses, converted per zoom level by the caller.
const SNAP_PX: f32 = 8.0;

/// The snap threshold in ticks at this zoom, so the magnet feels the same width
/// however far the timeline is zoomed in.
fn snap_ticks(fps: f32, pps: f32) -> i64 {
    if pps <= 0.0 {
        return 0;
    }
    (((SNAP_PX / pps) * fps.max(1.0)).round() as i64).max(1)
}

/// Make `el` a lane-group canvas container that accepts dragged pool media and
/// catalog items, resolving the target lane from the pointer's vertical offset.
pub(crate) fn group_target(
    el: Div,
    lanes: std::rc::Rc<Vec<(u64, f32, f32)>>,
    scroll_x: f32,
    fps: f32,
    pps: f32,
    accent: gpui::Hsla,
    cx: &mut gpui::Context<Shell>,
) -> Div {
    let lanes_media = lanes.clone();
    let lanes_catalog = lanes;
    el.on_drag_move(
        cx.listener(move |this, event: &gpui::DragMoveEvent<MediaDrag>, _, cx| {
            let x = f32::from(event.event.position.x - event.bounds.origin.x);
            let y = f32::from(event.event.position.y - event.bounds.origin.y);
            let Some(track_id) = lane_at_y(&lanes_media, y) else {
                return;
            };
            let tick = crate::panels::timeline::px_to_ticks(x + scroll_x, fps, pps);
            this.set_media_drop(track_id, tick);
            if drop_hint::set(DropHint::Lane {
                track: track_id,
                tick,
            }) {
                cx.notify();
            }
        }),
    )
    .drag_over::<MediaDrag>(move |style, _, window, _| {
        let alt = window.modifiers().alt;
        style.bg(accent.opacity(if alt { 0.28 } else { 0.12 }))
    })
    .on_drop(cx.listener(move |this, drag: &MediaDrag, window, cx| {
        drop_hint::clear();
        let media = drag.media;
        let from_source = drag.from_source;
        let insert = window.modifiers().secondary();
        let replacing = window.modifiers().alt && !from_source;
        let Some((track_id, tick)) = this.take_media_drop() else {
            return;
        };
        this.state.update(cx, |state, _| {
            let media = aria_models::MediaId::from_raw(media);
            let track = aria_models::TrackId::from_raw(track_id);
            if replacing {
                match state.replace_media_at(media, track, tick) {
                    Ok(true) => return,
                    Ok(false) => {}
                    Err(err) => {
                        state.report_error(&err);
                        return;
                    }
                }
            }
            let (tick, _) = state.snap_at(tick, None, snap_ticks(fps, pps), false);
            let dropped = if from_source {
                state.drop_source_on_timeline(Some(track), tick, insert)
            } else {
                state.drop_media_on_timeline(media, Some(track), tick)
            };
            if let Err(err) = dropped {
                state.report_error(&err);
            }
        });
        crate::app::spawn_preview(this.state.clone(), cx);
        cx.notify();
    }))
    .on_drag_move(cx.listener(
        move |this, event: &gpui::DragMoveEvent<crate::types::CatalogDrag>, _, cx| {
            let x = f32::from(event.event.position.x - event.bounds.origin.x);
            let y = f32::from(event.event.position.y - event.bounds.origin.y);
            let Some(track_id) = lane_at_y(&lanes_catalog, y) else {
                return;
            };
            let tick = crate::panels::timeline::px_to_ticks(x + scroll_x, fps, pps);
            this.set_catalog_drop(track_id, tick);
            if drop_hint::set(DropHint::Lane {
                track: track_id,
                tick,
            }) {
                cx.notify();
            }
        },
    ))
    .drag_over::<crate::types::CatalogDrag>(move |style, _, _, _| style.bg(accent.opacity(0.12)))
    .on_drop(
        cx.listener(move |this, drag: &crate::types::CatalogDrag, _, cx| {
            drop_hint::clear();
            let Some((track_id, tick)) = this.take_catalog_drop() else {
                return;
            };
            this.state.update(cx, |state, _| {
                if let Err(err) = state.drop_catalog_on_track(
                    drag,
                    Some(aria_models::TrackId::from_raw(track_id)),
                    tick,
                ) {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }),
    )
}

fn lane_at_y(lanes: &[(u64, f32, f32)], y: f32) -> Option<u64> {
    lanes
        .iter()
        .find(|(_, top, bottom)| y >= *top && y < *bottom)
        .or_else(|| lanes.last())
        .map(|(id, _, _)| *id)
}

/// The strip above the first lane or below the last: the only media drop that
/// adds a lane.
///
/// Thin and unpainted until a drag is over it. It spans the header column too,
/// so the tick is measured from the clip area's left edge rather than the
/// strip's own origin.
pub(crate) fn edge_strip(
    theme: &Theme,
    top: bool,
    scroll_x: f32,
    header_w: f32,
    fps: f32,
    pps: f32,
    cx: &mut gpui::Context<Shell>,
) -> impl IntoElement {
    let accent = theme.accent;
    div()
        .id(if top {
            "lane-edge-top"
        } else {
            "lane-edge-bottom"
        })
        .w_full()
        .h(px(Theme::SPACE_SM))
        .flex_none()
        // Where the new lane's clip would start. Faded in, and it slides with
        // the pointer because the line is one element that moves rather than one
        // per position.
        .when_some(drop_hint::edge_tick(top), |el, tick| {
            let x = crate::panels::timeline::ticks_to_px(tick, fps, pps) - scroll_x + header_w;
            el.relative().overflow_hidden().child(motion::fade_quick(
                if top {
                    "edge-drop-top"
                } else {
                    "edge-drop-bottom"
                },
                div()
                    .absolute()
                    .top_0()
                    .bottom_0()
                    .left(px(x))
                    .w(px(2.))
                    .bg(accent),
            ))
        })
        .on_drag_move(
            cx.listener(move |this, event: &gpui::DragMoveEvent<MediaDrag>, _, cx| {
                let x = f32::from(event.event.position.x - event.bounds.origin.x) - header_w;
                let tick = crate::panels::timeline::px_to_ticks(x + scroll_x, fps, pps);
                this.set_media_edge_drop(tick);
                if drop_hint::set(DropHint::Edge { top, tick }) {
                    cx.notify();
                }
            }),
        )
        .drag_over::<MediaDrag>(move |style, _, _, _| style.bg(accent.opacity(0.35)))
        .on_drop(cx.listener(move |this, drag: &MediaDrag, window, cx| {
            drop_hint::clear();
            let media = aria_models::MediaId::from_raw(drag.media);
            let from_source = drag.from_source.then(|| window.modifiers().secondary());
            let Some(tick) = this.take_media_edge_drop() else {
                return;
            };
            this.state.update(cx, |state, _| {
                let (tick, _) = state.snap_at(tick, None, snap_ticks(fps, pps), false);
                let dropped = match from_source {
                    Some(insert) => state.drop_source_on_new_lane(top, tick, insert),
                    None => state.drop_media_on_new_lane(media, top, tick),
                };
                if let Err(err) = dropped {
                    state.report_error(&err);
                }
            });
            crate::app::spawn_preview(this.state.clone(), cx);
            cx.notify();
        }))
}
