//! Hit-testing a point on a virtualized lane-group canvas, and the cursor each
//! target under the pointer shows.

use aria_models::{ClipId, ClipParam};
use gpui::CursorStyle;

use super::bar::{self, BarRows, LineHit};
use super::{FADE_GRIP, TRIM, VisibleLane};
use crate::app::property_rows::{PROPERTY_ROW_H, TimelineOutlineRow};
use crate::panels::timeline::{px_to_ticks, ticks_to_px};
use crate::tool_catalog::BodyDrag;
use crate::types::EditorTool;

/// Resolved target under the pointer inside a lane-group canvas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HitTarget {
    TransitionEdge {
        clip_id: u64,
        right: bool,
        duration_ticks: i64,
    },
    TransitionBody {
        clip_id: u64,
    },
    Fade {
        clip_id: u64,
        track_id: u64,
        left: bool,
        fade_ticks: i64,
    },
    ClipEdge {
        clip_id: u64,
        track_id: u64,
        left: bool,
        start_ticks: i64,
        end_ticks: i64,
    },
    ClipBody {
        clip_id: u64,
        track_id: u64,
        start_ticks: i64,
        end_ticks: i64,
        locked: bool,
        in_name_band: bool,
    },
    /// The line a clip bar draws, or one of its keyframe dots. `tick` is the
    /// timeline tick under the pointer, and `start_ticks` and `end_ticks` the
    /// clip's span, which a drag along the lane moves.
    ClipLine {
        clip_id: u64,
        track_id: u64,
        key: Option<i64>,
        tick: i64,
        start_ticks: i64,
        end_ticks: i64,
    },
    EmptyLane {
        track_id: u64,
        row: usize,
        tick: i64,
    },
    Keyframe {
        clip: ClipId,
        param: ClipParam,
        tick: i64,
    },
    PropertyRow {
        clip: ClipId,
        param: ClipParam,
        tick: i64,
    },
    /// A keyframe on a mask's Path row, at its timeline tick.
    MaskPathKey {
        clip: ClipId,
        mask: u32,
        tick: i64,
    },
    /// A mask's Path row away from its keyframes.
    MaskPathRow {
        clip: ClipId,
        mask: u32,
    },
}

/// Hit-test a local `(local_x, local_y)` point against `visible_lanes`.
pub(crate) fn hit_test(
    visible_lanes: &[VisibleLane],
    local_x: f32,
    local_y: f32,
    fps: f32,
    pps: f32,
    scroll_x: f32,
) -> Option<HitTarget> {
    let lane = visible_lanes
        .iter()
        .find(|l| local_y >= l.y_top && local_y <= l.y_bottom)
        .or_else(|| {
            if local_y < 0.0 {
                visible_lanes.first()
            } else {
                visible_lanes.last()
            }
        })?;
    let tick = px_to_ticks((local_x + scroll_x).max(0.0), fps, pps);

    if local_y >= lane.clip_y_bottom && !lane.outline_rows.is_empty() {
        let idx = ((local_y - lane.clip_y_bottom) / PROPERTY_ROW_H).floor() as usize;
        if let Some(TimelineOutlineRow::Property(prop_row)) = lane.outline_rows.get(idx) {
            for mark in &prop_row.keys {
                let kx = ticks_to_px(mark.tick, fps, pps) - scroll_x;
                if (local_x - kx).abs() <= 6.0 {
                    return Some(HitTarget::Keyframe {
                        clip: prop_row.clip,
                        param: prop_row.param,
                        tick: mark.tick,
                    });
                }
            }
            return Some(HitTarget::PropertyRow {
                clip: prop_row.clip,
                param: prop_row.param,
                tick,
            });
        }
        if let Some(TimelineOutlineRow::MaskPath(path_row)) = lane.outline_rows.get(idx) {
            let (clip, mask) = (path_row.clip, path_row.mask);
            for mark in &path_row.keys {
                let kx = ticks_to_px(mark.tick, fps, pps) - scroll_x;
                if (local_x - kx).abs() <= 6.0 {
                    return Some(HitTarget::MaskPathKey {
                        clip,
                        mask,
                        tick: mark.tick,
                    });
                }
            }
            return Some(HitTarget::MaskPathRow { clip, mask });
        }
        return Some(HitTarget::EmptyLane {
            track_id: lane.track_id,
            row: lane.row,
            tick,
        });
    }
    // 1. Transitions take priority over the clips they cover.
    for clip in &lane.clips {
        if let Some(tr) = &clip.transition {
            let (low, high) = bar::transition_span(clip, tr);
            let wx0 = ticks_to_px(low, fps, pps) - scroll_x;
            let wx1 = (ticks_to_px(high, fps, pps) - scroll_x).max(wx0 + 4.0);
            if local_x >= wx0 && local_x <= wx1 {
                if local_x <= wx0 + 6.0 {
                    return Some(HitTarget::TransitionEdge {
                        clip_id: clip.id,
                        right: false,
                        duration_ticks: tr.duration_ticks,
                    });
                }
                if local_x >= wx1 - 6.0 {
                    return Some(HitTarget::TransitionEdge {
                        clip_id: clip.id,
                        right: true,
                        duration_ticks: tr.duration_ticks,
                    });
                }
                return Some(HitTarget::TransitionBody { clip_id: clip.id });
            }
        }
    }
    let lane_h = (lane.clip_y_bottom - lane.y_top).max(1.0);
    let rows = BarRows::of(lane_h);
    let name_bottom = lane.y_top + rows.top + bar::NAME_STRIP;

    // 2. Clips on this lane (reverse order so topmost / later clip wins on overlap).
    for clip in lane.clips.iter().rev() {
        let x0 = ticks_to_px(clip.timeline_start_ticks, fps, pps) - scroll_x;
        let w = ticks_to_px(
            (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1),
            fps,
            pps,
        )
        .max(2.0);
        let x1 = x0 + w;
        if local_x < x0 || local_x > x1 {
            continue;
        }
        if !lane.collapsed && !clip.locked {
            let edge_w = TRIM.min(w * 0.25).max(2.0);
            if local_x <= x0 + edge_w {
                return Some(HitTarget::ClipEdge {
                    clip_id: clip.id,
                    track_id: lane.track_id,
                    left: true,
                    start_ticks: clip.timeline_start_ticks,
                    end_ticks: clip.timeline_end_ticks,
                });
            }
            if local_x >= x1 - edge_w {
                return Some(HitTarget::ClipEdge {
                    clip_id: clip.id,
                    track_id: lane.track_id,
                    left: false,
                    start_ticks: clip.timeline_start_ticks,
                    end_ticks: clip.timeline_end_ticks,
                });
            }
            if w >= 28.0 && local_y <= name_bottom {
                let fi = ticks_to_px(clip.fade_in_ticks.max(0), fps, pps);
                let fo = ticks_to_px(clip.fade_out_ticks.max(0), fps, pps);
                let fi_x = (x0 + TRIM + fi).min(x1 - FADE_GRIP);
                let fo_x = (x1 - TRIM - fo - FADE_GRIP).max(x0);
                if local_x >= fi_x && local_x <= fi_x + FADE_GRIP {
                    return Some(HitTarget::Fade {
                        clip_id: clip.id,
                        track_id: lane.track_id,
                        left: true,
                        fade_ticks: clip.fade_in_ticks.max(0),
                    });
                }
                if local_x >= fo_x && local_x <= fo_x + FADE_GRIP {
                    return Some(HitTarget::Fade {
                        clip_id: clip.id,
                        track_id: lane.track_id,
                        left: false,
                        fade_ticks: clip.fade_out_ticks.max(0),
                    });
                }
            }
            if let Some(on) =
                bar::line_hit(clip, lane.y_top, lane_h, x0, local_x, local_y, fps, pps)
            {
                return Some(HitTarget::ClipLine {
                    clip_id: clip.id,
                    track_id: lane.track_id,
                    key: match on {
                        LineHit::Key(tick) => Some(tick),
                        LineHit::Line => None,
                    },
                    tick,
                    start_ticks: clip.timeline_start_ticks,
                    end_ticks: clip.timeline_end_ticks,
                });
            }
        }
        return Some(HitTarget::ClipBody {
            clip_id: clip.id,
            track_id: lane.track_id,
            start_ticks: clip.timeline_start_ticks,
            end_ticks: clip.timeline_end_ticks,
            locked: clip.locked,
            in_name_band: local_y <= name_bottom,
        });
    }

    Some(HitTarget::EmptyLane {
        track_id: lane.track_id,
        row: lane.row,
        tick,
    })
}

/// Cursor style for the hovered hit target under `tool`.
pub(crate) fn cursor_for(tool: EditorTool, hit: Option<HitTarget>) -> CursorStyle {
    if tool.pans() {
        return CursorStyle::OpenHand;
    }
    if tool.zooms() || tool.cuts() || tool.track_select().is_some() {
        return CursorStyle::PointingHand;
    }
    if tool.pens() {
        return CursorStyle::Crosshair;
    }
    match hit {
        Some(
            HitTarget::TransitionEdge { .. } | HitTarget::Fade { .. } | HitTarget::ClipEdge { .. },
        ) => CursorStyle::ResizeLeftRight,
        Some(HitTarget::ClipLine { .. }) => CursorStyle::ResizeUpDown,
        Some(HitTarget::ClipBody { locked: false, .. }) => match tool.body_drag() {
            Some(BodyDrag::Slip | BodyDrag::Slide) => CursorStyle::ResizeLeftRight,
            None => CursorStyle::PointingHand,
        },
        Some(HitTarget::ClipBody { locked: true, .. }) => CursorStyle::OperationNotAllowed,
        Some(HitTarget::TransitionBody { .. }) => CursorStyle::PointingHand,
        Some(HitTarget::Keyframe { .. } | HitTarget::MaskPathKey { .. }) => {
            CursorStyle::PointingHand
        }
        _ => CursorStyle::Arrow,
    }
}
