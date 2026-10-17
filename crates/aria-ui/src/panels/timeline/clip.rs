//! Virtualized clip and lane-group painting on the timeline.
//!
//! Each lane group (video above the divider, audio below) paints its entire
//! time area inside one `gpui::canvas` element. Off-screen lanes are culled by
//! vertical scroll bounds and off-screen clips are culled via binary search on
//! `[view_start, view_end)`, while sub-pixel clips at extreme zoom-out coalesce
//! so paint calls per frame stay strictly bounded.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use gpui::{
    App, Bounds, ContentMask, Corners, Edges, Hsla, Pixels, RenderImage, SharedString, TextAlign,
    TextRun, Window, fill, point, px, quad, size,
};

use crate::panels::timeline::{COLLAPSED_H, ticks_to_px, ticks_to_px_f};
use crate::theme::Theme;
use crate::types::{SelectedEdge, SelectedGap, TrimFeedback, TrimSide, UiClip};

pub(crate) const TRIM: f32 = 8.0;
pub(crate) const FADE_GRIP: f32 = 8.0;
const FILM_H: f32 = crate::filmstrips::CELL_H as f32;

/// One visible lane prepared for painting and hit-testing inside a lane group.
#[derive(Clone)]
pub(crate) struct VisibleLane {
    pub row: usize,
    pub track_id: u64,
    pub y_top: f32,
    pub clip_y_bottom: f32,
    pub y_bottom: f32,
    pub collapsed: bool,
    pub locked: bool,
    pub is_drop_row: bool,
    pub band: Option<(i64, i64)>,
    pub drop_tick: Option<i64>,
    pub clips: Vec<UiClip>,
    pub outline_rows: Vec<crate::app::property_rows::TimelineOutlineRow>,
}

/// Snapshot of data painted by one lane-group canvas in a single pass.
pub(crate) struct LaneGroupPaint {
    pub is_first_group: bool,
    pub total_lanes: usize,
    pub total_clips: usize,
    pub visible_lanes: Vec<VisibleLane>,
    pub pps: f32,
    pub fps: f32,
    pub scroll_x: f32,
    pub playhead: f32,
    pub snap_guide: Option<crate::snap::SnapCandidate>,
    pub selected_edge: Option<SelectedEdge>,
    pub selected_gap: Option<SelectedGap>,
    pub trim_feedback: Option<TrimFeedback>,
    pub waves: Arc<std::collections::HashMap<u64, Vec<f32>>>,
    pub strips: Arc<std::collections::HashMap<u64, Vec<(f32, Arc<RenderImage>)>>>,
    pub reduced: bool,
}

/// Where each clip's fx badge was painted in the last frame, in window
/// pixels, shared by a lane group's canvas and its pointer listeners.
pub(crate) type FxBadges = Rc<RefCell<Vec<(u64, Bounds<Pixels>)>>>;

/// The fill a clip bar is drawn in: its label, or the label's selected fill
/// while the clip is selected.
pub(crate) fn tint(theme: &Theme, clip: &UiClip) -> Hsla {
    theme.label(clip.drawn_label(), clip.selected)
}

/// The colour of the name, icon, and waveform drawn on a clip bar's fill.
pub(crate) fn ink(theme: &Theme, clip: &UiClip) -> Hsla {
    theme.label_ink(clip.drawn_label(), clip.selected)
}

/// Opacity multiplier for a timeline clip bar (`1.0` when enabled, `0.4` when disabled).
pub(crate) fn clip_opacity(clip: &UiClip) -> f32 {
    if clip.enabled { 1.0 } else { 0.4 }
}

/// Paint all visible lanes, clips, waveforms, filmstrips, transitions,
/// selection highlights, guides, and the trim HUD inside `bounds`, returning
/// `(visible_lanes_count, visible_clips_count, paint_calls)`. Each fx badge
/// painted is pushed to `badges`.
pub(crate) fn paint_lane_group(
    bounds: Bounds<Pixels>,
    data: &LaneGroupPaint,
    theme: &Theme,
    badges: &mut Vec<(u64, Bounds<Pixels>)>,
    window: &mut Window,
    cx: &mut App,
) -> (usize, usize, usize) {
    let mut paint_calls = 0usize;
    let mut visible_clips = 0usize;
    let origin_x = f32::from(bounds.origin.x);
    let origin_y = f32::from(bounds.origin.y);
    let view_w = f32::from(bounds.size.width).max(1.0);
    let view_h = f32::from(bounds.size.height).max(1.0);

    window.with_content_mask(Some(ContentMask::from(bounds)), |window| {
        // 1. Background fill for the group canvas.
        paint_solid(
            window,
            &mut paint_calls,
            origin_x,
            origin_y,
            view_w,
            view_h,
            theme.bg,
        );

        for lane in &data.visible_lanes {
            let lane_h = (lane.clip_y_bottom - lane.y_top).max(1.0);
            let full_lane_h = (lane.y_bottom - lane.y_top).max(1.0);
            let lane_y = origin_y + lane.y_top;
            // Selected gap highlight on this lane.
            if let Some(gap) = data.selected_gap
                && gap.track == lane.track_id
            {
                let gx0 =
                    (ticks_to_px(gap.start_ticks, data.fps, data.pps) - data.scroll_x).max(0.0);
                let gx1 =
                    (ticks_to_px(gap.end_ticks, data.fps, data.pps) - data.scroll_x).min(view_w);
                if gx1 > gx0 {
                    paint_bordered(
                        window,
                        &mut paint_calls,
                        origin_x + gx0,
                        lane_y + 2.0,
                        (gx1 - gx0).max(2.0),
                        (lane_h - 4.0).max(2.0),
                        2.0,
                        theme.accent.opacity(0.18),
                        1.0,
                        theme.accent,
                    );
                }
            }

            // Clips on this lane, coalescing sub-pixel unselected bars when zoomed out.
            let mut last_coalesced_end = -10.0f32;
            for clip in &lane.clips {
                visible_clips += 1;
                let x0 = ticks_to_px(clip.timeline_start_ticks, data.fps, data.pps) - data.scroll_x;
                let raw_w = ticks_to_px(
                    (clip.timeline_end_ticks - clip.timeline_start_ticks).max(1),
                    data.fps,
                    data.pps,
                );
                let w = raw_w.max(2.0);
                let x1 = x0 + w;
                if x1 < 0.0 || x0 > view_w {
                    continue;
                }
                if !clip.selected && raw_w < 1.5 && x1 <= last_coalesced_end + 1.5 {
                    continue;
                }
                if raw_w < 1.5 {
                    last_coalesced_end = x1;
                }
                let hits = paint_clip(
                    window,
                    cx,
                    &mut paint_calls,
                    theme,
                    data,
                    lane,
                    clip,
                    origin_x + x0,
                    lane_y,
                    w,
                    lane_h,
                    view_w,
                    origin_x,
                );
                if let Some(fx) = hits.fx {
                    badges.push((clip.id, fx));
                }
            }

            // Transitions: a bar over each cut with the transition's name.
            let rows = bar::BarRows::of(lane_h);
            for clip in &lane.clips {
                if let Some(tr) = &clip.transition {
                    let (low, high) = bar::transition_span(clip, tr);
                    let wx0 = ticks_to_px(low, data.fps, data.pps) - data.scroll_x;
                    let wx1 = ticks_to_px(high, data.fps, data.pps) - data.scroll_x;
                    if wx1 >= 0.0 && wx0 <= view_w {
                        bar::paint_transition(
                            window,
                            cx,
                            &mut paint_calls,
                            theme,
                            &tr.label,
                            origin_x + wx0,
                            origin_x + wx1,
                            lane_y + rows.top,
                            rows.height,
                        );
                    }
                }
            }

            // Marquee band on this lane.
            if let Some((low, high)) = lane.band {
                let mx = ticks_to_px(low, data.fps, data.pps) - data.scroll_x;
                let mw = ticks_to_px((high - low).max(0), data.fps, data.pps).max(1.0);
                paint_bordered(
                    window,
                    &mut paint_calls,
                    origin_x + mx,
                    lane_y,
                    mw,
                    lane_h,
                    0.0,
                    theme.selection,
                    1.0,
                    theme.accent,
                );
            }

            // Media/catalog drop insertion line on this lane.
            if let Some(tick) = lane.drop_tick {
                let dx = ticks_to_px(tick, data.fps, data.pps) - data.scroll_x;
                if dx >= 0.0 && dx <= view_w {
                    paint_solid(
                        window,
                        &mut paint_calls,
                        origin_x + dx,
                        lane_y,
                        2.0,
                        lane_h,
                        theme.cursor,
                    );
                }
            }

            if !lane.outline_rows.is_empty() {
                crate::panels::timeline::property_rows::paint_outline_rows(
                    window,
                    &mut paint_calls,
                    theme,
                    data,
                    &lane.outline_rows,
                    origin_x,
                    origin_y + lane.clip_y_bottom,
                    view_w,
                );
            }

            // Bottom lane divider line (and reorder drop indicator).
            paint_solid(
                window,
                &mut paint_calls,
                origin_x,
                lane_y + full_lane_h - 1.0,
                view_w,
                1.0,
                theme.border,
            );
            if lane.is_drop_row {
                paint_solid(
                    window,
                    &mut paint_calls,
                    origin_x,
                    lane_y,
                    view_w,
                    2.0,
                    theme.accent,
                );
            }
        }

        // Snap guide line across the group.
        if let Some(guide) = data.snap_guide {
            let sx = ticks_to_px(guide.tick, data.fps, data.pps) - data.scroll_x;
            if sx >= 0.0 && sx <= view_w {
                paint_solid(
                    window,
                    &mut paint_calls,
                    origin_x + sx,
                    origin_y,
                    1.0,
                    view_h,
                    theme.snap_guide,
                );
            }
        }

        // Playhead line across the group.
        let px_at = ticks_to_px_f(data.playhead, data.fps, data.pps) - data.scroll_x;
        if px_at >= -2.0 && px_at <= view_w + 2.0 {
            paint_solid(
                window,
                &mut paint_calls,
                origin_x + px_at,
                origin_y,
                1.5,
                view_h,
                theme.playhead,
            );
        }

        // On-screen TrimFeedback HUD pill (drawn in the first group so it appears once).
        if data.is_first_group
            && let Some(fb) = &data.trim_feedback
        {
            paint_trim_hud(
                window,
                cx,
                &mut paint_calls,
                theme,
                fb,
                origin_x,
                origin_y,
                view_w,
            );
        }
    });

    (data.visible_lanes.len(), visible_clips, paint_calls)
}

/// Paint one clip bar, and return where its fx badge was drawn.
#[allow(clippy::too_many_arguments)]
fn paint_clip(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    theme: &Theme,
    data: &LaneGroupPaint,
    lane: &VisibleLane,
    clip: &UiClip,
    x: f32,
    lane_y: f32,
    w: f32,
    lane_h: f32,
    view_w: f32,
    origin_x: f32,
) -> bar::StripHits {
    let alpha = clip_opacity(clip);
    // An offline clip is red whatever its label: the file it places is gone.
    let (fill, ink) = match clip.offline {
        Some(_) => (theme.danger, Theme::ink_on(theme.danger)),
        None => (tint(theme, clip), ink(theme, clip)),
    };
    let color = fill.opacity(alpha);
    // A selected clip takes a 1 px white border in either appearance: the
    // label fill under it is the operator's colour, not a theme surface.
    let border = gpui::white();
    let ring = state::selection_t(clip.id, clip.selected, data.reduced);
    if lane.collapsed {
        paint_bordered(
            window,
            paint_calls,
            x,
            lane_y + 4.0,
            w,
            (COLLAPSED_H - 8.0).max(4.0),
            2.0,
            color,
            if state::showing(ring) { 1.0 } else { 0.0 },
            border.opacity(ring * alpha),
        );
        return bar::StripHits { fx: None };
    }

    let rows = bar::BarRows::of(lane_h);
    let card_y = lane_y + rows.top;
    let card_h = rows.height;
    let _ = state::lock_t(clip.id, clip.locked, data.reduced);

    // Card body in the label fill.
    paint_bordered(
        window,
        paint_calls,
        x,
        card_y,
        w,
        card_h,
        Theme::CONTROL_RADIUS,
        color,
        if state::showing(ring) { 1.0 } else { 0.0 },
        border.opacity(ring * alpha),
    );

    // The content strip: everything under the name strip.
    let content_top = card_y + bar::NAME_STRIP;
    let content_bottom = card_y + card_h - 1.0;
    let content_h = content_bottom - content_top;
    let grips = !clip.locked && w >= 28.0;
    let strip = bar::Strip::of(x, w, origin_x, grips);
    let filmstrip = data
        .strips
        .get(&clip.id)
        .filter(|_| w >= 24.0 && content_h >= 8.0);
    if let Some(file) = &clip.offline {
        bar::paint_offline(
            window,
            cx,
            paint_calls,
            theme,
            file,
            ink.opacity(alpha),
            strip,
            content_top,
            content_bottom,
        );
    } else if let Some(cells) = filmstrip {
        let strip_h = FILM_H.min(content_h);
        let strip_y = content_bottom - strip_h;
        for (cell_x, image) in cells {
            let cx_px = x + *cell_x;
            if cx_px + 60.0 < origin_x || cx_px > origin_x + view_w {
                continue;
            }
            let img_bounds = Bounds {
                origin: point(px(cx_px), px(strip_y)),
                size: size(px(60.0f32.min(x + w - cx_px)), px(strip_h)),
            };
            let _ = window.paint_image(
                img_bounds,
                img_bounds,
                Corners::default(),
                image.clone(),
                0,
                false,
            );
            *paint_calls += 1;
            if !clip.enabled {
                paint_solid(
                    window,
                    paint_calls,
                    cx_px,
                    strip_y,
                    60.0f32.min(x + w - cx_px),
                    strip_h,
                    theme.bg.opacity(0.6),
                );
            }
        }
    } else if let Some(generator) = &clip.generator {
        bar::paint_generator(
            window,
            cx,
            paint_calls,
            theme,
            generator,
            ink.opacity(alpha),
            strip,
            content_top,
            content_bottom,
        );
    }

    // Waveform bars across the content strip of an audio clip.
    if clip.offline.is_none()
        && w >= 12.0
        && content_h >= 4.0
        && let Some(bars) = data.waves.get(&clip.id)
        && !bars.is_empty()
    {
        let wave_h = (content_h - 2.0).max(2.0);
        let wave_bottom = content_bottom - 1.0;
        let max_bars = bars.len().min(96);
        let step = (w / max_bars as f32).max(2.0);
        for (idx, amp) in bars.iter().take(max_bars).enumerate() {
            let bx = x + idx as f32 * step;
            if bx < origin_x || bx > origin_x + view_w {
                continue;
            }
            let bh = (amp.clamp(0.05, 1.0) * wave_h).max(1.5);
            paint_solid(
                window,
                paint_calls,
                bx,
                wave_bottom - bh,
                (step - 1.0).max(1.0),
                bh,
                ink.opacity(0.55 * alpha),
            );
        }
    }

    // The line one property is drawn as, at its value, with keyframe dots on
    // a lane Medium or taller.
    if rows.has_line()
        && w >= 6.0
        && let Some(line) = &clip.line
    {
        bar::paint_line(
            window,
            paint_calls,
            clip,
            line,
            rows,
            lane_y,
            x,
            w,
            (origin_x, origin_x + view_w),
            data.fps,
            data.pps,
            ink.opacity(0.85 * alpha),
            lane_h >= bar::DOTS_FROM,
        );
    }

    // Fade handles on unlocked clips with sufficient width.
    if grips {
        let fi = ticks_to_px(clip.fade_in_ticks.max(0), data.fps, data.pps);
        let fo = ticks_to_px(clip.fade_out_ticks.max(0), data.fps, data.pps);
        paint_bordered(
            window,
            paint_calls,
            (x + TRIM + fi).min(x + w - FADE_GRIP),
            card_y + 2.0,
            FADE_GRIP,
            FADE_GRIP,
            2.0,
            theme.wash(0.25),
            1.0,
            theme.border_strong.opacity(0.55),
        );
        paint_bordered(
            window,
            paint_calls,
            (x + w - TRIM - fo - FADE_GRIP).max(x),
            card_y + 2.0,
            FADE_GRIP,
            FADE_GRIP,
            2.0,
            theme.wash(0.25),
            1.0,
            theme.border_strong.opacity(0.55),
        );
    }
    // Clip-attached markers (point flags and range bars along the top edge).
    if !clip.markers.is_empty() && w >= 6.0 {
        for marker in &clip.markers {
            let swatch = theme.marker(marker.color).opacity(alpha);
            let mx0 = (origin_x + ticks_to_px(marker.tick, data.fps, data.pps) - data.scroll_x)
                .clamp(x, x + w);
            if marker.duration_ticks > 0 {
                let mx1 = (origin_x
                    + ticks_to_px(
                        marker.tick.saturating_add(marker.duration_ticks),
                        data.fps,
                        data.pps,
                    )
                    - data.scroll_x)
                    .clamp(mx0, x + w);
                let mw = (mx1 - mx0).max(5.0).min((x + w - mx0).max(1.0));
                paint_bordered(
                    window,
                    paint_calls,
                    mx0,
                    card_y + 1.0,
                    mw,
                    5.0,
                    1.5,
                    swatch.opacity(0.45 * alpha),
                    1.0,
                    swatch,
                );
            } else {
                let left = (mx0 - 2.5).clamp(x, (x + w - 5.0).max(x));
                paint_solid(window, paint_calls, left, card_y + 1.0, 5.0, 5.0, swatch);
            }
        }
    }

    // Selected edge bracket (`3px` accent bar on the active trim edge).
    if let Some(edge) = data.selected_edge
        && edge.clip == clip.id
    {
        let ex = match edge.side {
            TrimSide::In => x,
            TrimSide::Out => (x + w - 3.0).max(x),
        };
        paint_solid(window, paint_calls, ex, card_y, 3.0, card_h, theme.warning);
    }

    if w >= 24.0 && x + w >= origin_x && x <= origin_x + view_w {
        bar::paint_name_strip(
            window,
            cx,
            paint_calls,
            theme,
            clip,
            ink,
            alpha,
            strip,
            card_y,
        )
    } else {
        bar::StripHits { fx: None }
    }
}

fn paint_trim_hud(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    theme: &Theme,
    fb: &TrimFeedback,
    origin_x: f32,
    origin_y: f32,
    view_w: f32,
) {
    let text = format!(
        "{} · {} · {} · {}",
        fb.tool_label, fb.clip_name, fb.delta_label, fb.detail_label
    );
    let pill_w = (text.len() as f32 * 6.8 + 20.0)
        .min(view_w - 16.0)
        .max(140.0);
    let pill_x = origin_x + ((view_w - pill_w) * 0.5).max(8.0);
    let pill_y = origin_y + 6.0;
    paint_bordered(
        window,
        paint_calls,
        pill_x,
        pill_y,
        pill_w,
        22.0,
        4.0,
        theme.surface_raised,
        1.0,
        theme.accent,
    );
    paint_label(
        window,
        cx,
        paint_calls,
        &text,
        pill_x + 8.0,
        pill_y + 4.0,
        11.0,
        theme.text,
        &theme.font_mono,
    );
}

fn paint_solid(
    window: &mut Window,
    paint_calls: &mut usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    bg: Hsla,
) {
    let bounds = Bounds {
        origin: point(px(x), px(y)),
        size: size(px(w.max(0.0)), px(h.max(0.0))),
    };
    window.paint_quad(fill(bounds, bg));
    *paint_calls += 1;
}

#[allow(clippy::too_many_arguments)]
fn paint_bordered(
    window: &mut Window,
    paint_calls: &mut usize,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    radius: f32,
    bg: Hsla,
    border_w: f32,
    border_color: Hsla,
) {
    let bounds = Bounds {
        origin: point(px(x), px(y)),
        size: size(px(w.max(0.0)), px(h.max(0.0))),
    };
    window.paint_quad(quad(
        bounds,
        Corners::all(px(radius)),
        bg,
        Edges::all(px(border_w)),
        border_color,
        gpui::BorderStyle::default(),
    ));
    *paint_calls += 1;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn paint_label(
    window: &mut Window,
    cx: &mut App,
    paint_calls: &mut usize,
    text: &str,
    x: f32,
    y: f32,
    font_px: f32,
    color: Hsla,
    family: &SharedString,
) {
    if text.is_empty() {
        return;
    }
    let shared = SharedString::from(text.to_owned());
    let run = TextRun {
        len: shared.len(),
        font: gpui::Font {
            family: family.clone(),
            features: gpui::FontFeatures::default(),
            fallbacks: None,
            weight: gpui::FontWeight::NORMAL,
            style: gpui::FontStyle::Normal,
        },
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
        tracking: Default::default(),
    };
    let shaped = window
        .text_system()
        .shape_line(shared, px(font_px), &[run], None);
    let _ = shaped.paint(
        point(px(x), px(y)),
        px(font_px + 4.0),
        TextAlign::Left,
        None,
        window,
        cx,
    );
    *paint_calls += 1;
}

mod bar;
pub(crate) mod finish;
mod state;
