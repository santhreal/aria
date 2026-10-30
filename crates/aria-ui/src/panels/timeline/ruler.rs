//! The time half of the timeline header: the ruler and its markers, the work
//! area bar under it, the render bar along the bottom, the playhead's head
//! (`crate::timeline_playhead`), and the snap guide across all three.

use gpui::motion::presets;
use gpui::prelude::FluentBuilder;
use gpui::{
    Bounds, CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, TextAlign, TextRun, canvas,
    div, fill, point, px, size,
};

use aria_models::{Rational, TimeDisplay};

use crate::app::work_area::WorkAreaGrip;
use crate::motion;
use crate::theme::Theme;
use crate::transport::ticks_to_px_f;
use crate::types::UiSequence;

use super::media::visible_clips_slice;
use super::{RULER_H, RULER_TICKS_H, ruler_label_chars, ruler_scale, ticks_to_px};

/// Height of an unlabelled tick, standing on the ticks' bottom edge.
const MINOR_TICK_H: f32 = 6.0;
/// Height of a marker on the ruler. A marker stands on the ticks' bottom edge,
/// below the labels, so every label reads clear of the markers at its time.
const MARKER_H: f32 = 10.0;
/// Where the work area bar starts under the ticks, and how tall it is.
const WORK_AREA_TOP: f32 = RULER_TICKS_H + 2.0;
const WORK_AREA_H: f32 = 8.0;
/// Width of the grip at each end of the work area bar.
const WORK_AREA_GRIP_W: f32 = 4.0;
/// How deep the ruler fades at an edge with content past it.
pub(super) const RULER_FADE: f32 = Theme::SPACE_LG;

/// The magnet's confirmation, as a 0..1 breath.
pub(super) fn snap_glow(cx: &mut gpui::Context<crate::shell::Shell>) -> f32 {
    let view = cx.entity_id();
    gpui::motion::loaders::pulse_wave(motion::pulse_delta(&presets::ZERON_PULSE, view, cx))
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ruler(
    theme: &Theme,
    sequence: &UiSequence,
    playhead: Option<f32>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    viewport: f32,
    view_start: i64,
    view_end: i64,
    snap_guide: Option<crate::snap::SnapCandidate>,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let glow = snap_guide.map(|_| snap_glow(cx)).unwrap_or_default();
    let sequence_px = ticks_to_px(sequence.duration_ticks, fps, pps);
    let fade_left = scroll_x > 0.0;
    let fade_right = sequence_px > scroll_x + viewport;
    let mark = state.read(cx).timeline_playhead.ruler_mark(
        playhead.map(|at| ticks_to_px_f(at, fps, pps) - scroll_x),
        RULER_FADE,
        (fade_left, fade_right),
        theme.playhead,
    );

    div()
        .id("ruler")
        .h(px(RULER_H))
        .w_full()
        .flex()
        .flex_row()
        .border_b_1()
        .border_color(theme.border)
        .child(super::header::header_controls(theme, state, cx))
        .child(
            crate::edge_fade::edge_faded(
                RULER_FADE,
                ruler_body(
                    theme,
                    RulerTime {
                        rate: sequence.frame_rate,
                        display: sequence.time_display,
                        last_tick: sequence.duration_ticks,
                    },
                    mark,
                    pps,
                    fps,
                    scroll_x,
                    viewport,
                    view_start,
                    view_end,
                    snap_guide,
                    glow,
                    state,
                    cx,
                ),
            )
            .fade_left(fade_left)
            .fade_right(fade_right),
        )
}

/// How the ruler counts the sequence: its rate, its time display, and the
/// last tick a label has to fit.
#[derive(Clone, Copy)]
struct RulerTime {
    rate: Rational,
    display: TimeDisplay,
    last_tick: i64,
}

#[allow(clippy::too_many_arguments)]
fn ruler_body(
    theme: &Theme,
    time: RulerTime,
    mark: impl IntoElement,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    viewport: f32,
    view_start: i64,
    view_end: i64,
    snap_guide: Option<crate::snap::SnapCandidate>,
    glow: f32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .id("ruler-body")
        .debug_selector(|| "timeline-ruler".into())
        .flex_1()
        .h_full()
        .relative()
        .overflow_hidden()
        // Only a press on the ruler scrubs: a drag that began on the work
        // area bar, a marker, or a clip and crosses the ruler leaves the
        // playhead where it is. The shell carries the scrub past the ruler.
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                // The playhead to the frame under window `x`; a move that
                // stays on the playhead's frame scrubs nothing.
                let scrub =
                    move |this: &mut crate::shell::Shell,
                          x: gpui::Pixels,
                          moved: bool,
                          cx: &mut gpui::Context<crate::shell::Shell>| {
                        let Some(left) = this.state.read(cx).timeline_content_left() else {
                            return;
                        };
                        let ticks = super::pointer_ticks(x, left, scroll_x, fps, pps);
                        if moved && this.state.read(cx).playhead.value == ticks {
                            return;
                        }
                        this.state
                            .update(cx, |state, _| state.scrub_playhead_on_ruler(ticks));
                        crate::app::spawn_preview(this.state.clone(), cx);
                        this.redraw(crate::shell::Moved::Playhead, cx);
                    };
                // Escape puts the playhead back where the press found it.
                let from = this.state.read(cx).playhead.value;
                scrub(this, event.position.x, false, cx);
                this.hold(crate::shell::Hold::new(
                    move |this, event, cx| scrub(this, event.position.x, true, cx),
                    |_, _, _| {},
                    move |this, cx| {
                        if this.state.read(cx).playhead.value == from {
                            return;
                        }
                        this.state
                            .update(cx, |state, _| state.scrub_playhead_on_ruler(from));
                        crate::app::spawn_preview(this.state.clone(), cx);
                        this.redraw(crate::shell::Moved::Playhead, cx);
                    },
                ));
            }),
        )
        .child(ruler_ticks(theme, time, pps / fps.max(1.0), scroll_x))
        .children({
            let (visible, beats) = {
                let app = state.read(cx);
                let visible: Vec<_> = app
                    .store
                    .sequence
                    .markers
                    .iter()
                    .take_while(|marker| {
                        ticks_to_px(marker.tick, fps, pps) - scroll_x <= viewport + 8.0
                    })
                    .filter_map(|marker| {
                        let (left, width, is_range) = marker_ruler_span(
                            marker.tick,
                            marker.duration_ticks,
                            fps,
                            pps,
                            scroll_x,
                        );
                        (left + width >= -8.0 && left <= viewport + 8.0).then_some((
                            left,
                            width,
                            is_range,
                            marker.tick,
                            marker.id,
                            marker.color,
                        ))
                    })
                    .collect();
                let mut beats: Vec<i64> = app
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .flat_map(|track| visible_clips_slice(&track.clips, view_start, view_end))
                    .flat_map(|clip| clip.beat_ticks.iter().copied())
                    .filter(|tick| {
                        let x = ticks_to_px(*tick, fps, pps) - scroll_x;
                        x >= -8.0 && x <= viewport + 8.0
                    })
                    .collect();
                beats.sort_unstable();
                beats.dedup();
                (visible, beats)
            };
            let mut flags = Vec::with_capacity(visible.len() + beats.len());
            for (left, width, is_range, tick, id, color) in visible {
                let swatch = theme.marker(color);
                flags.push(
                    div()
                        .id(gpui::ElementId::named_usize("marker", id as usize))
                        .debug_selector(move || format!("ruler-marker-{id}"))
                        .absolute()
                        .left(px(left))
                        .top(px(RULER_TICKS_H - MARKER_H))
                        .w(px(width))
                        .h(px(MARKER_H))
                        .bg(if is_range {
                            swatch.opacity(0.42)
                        } else {
                            swatch
                        })
                        .when(is_range, |el| el.border_1().border_color(swatch))
                        .cursor(CursorStyle::PointingHand)
                        .active(|style| style.opacity(0.92))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                this.state.update(cx, |state, _| state.set_playhead(tick));
                                crate::app::spawn_preview(this.state.clone(), cx);
                                cx.notify();
                            }),
                        )
                        .into_any_element(),
                );
            }
            // Beats stand on the ticks' bottom edge, above the work area bar.
            for tick in beats {
                let x = ticks_to_px(tick, fps, pps) - scroll_x;
                flags.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(RULER_TICKS_H - 6.0))
                        .w(px(1.))
                        .h(px(6.))
                        .bg(theme.text_faint)
                        .into_any_element(),
                );
            }
            flags
        })
        .children(work_area_bar(
            theme, state, pps, fps, scroll_x, viewport, cx,
        ))
        .child(super::render_bar::render_bar(
            theme,
            &state.read(cx).render_bar(),
            pps,
            fps,
            scroll_x,
            viewport,
        ))
        .child(mark)
        .when_some(snap_guide, |el, guide| {
            el.child(snap_guide_line(theme, guide, pps, fps, scroll_x, glow))
        })
}

/// The work area under the ticks: the In to Out range as a bar, bright when
/// either point is set and faint while the range falls back to the whole
/// sequence. The bar moves both points, each end grip moves one, and a
/// double-click on either sets the range to the sequence. `None` while the
/// range is off screen.
#[allow(clippy::too_many_arguments)]
fn work_area_bar(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    viewport: f32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<gpui::AnyElement> {
    let ((start, end), marked) = {
        let app = state.read(cx);
        (
            app.work_area(),
            app.mark_in.is_some() || app.mark_out.is_some(),
        )
    };
    // Clamped just past the visible width, so a long sequence at a deep zoom
    // never lays out a bar millions of pixels wide; a grip clamped there is
    // drawn off screen, where its end is.
    let reach = WORK_AREA_GRIP_W * 2.0;
    let left = (ticks_to_px(start, fps, pps) - scroll_x).max(-reach);
    let right = (ticks_to_px(end, fps, pps) - scroll_x).min(viewport + reach);
    if right <= left {
        return None;
    }
    let (fill, grip) = if marked {
        (theme.accent.opacity(0.42), theme.accent)
    } else {
        (theme.text_faint.opacity(0.22), theme.text_faint)
    };
    let press = |grip: WorkAreaGrip| {
        cx.listener(move |this, event: &MouseDownEvent, _, cx| {
            cx.stop_propagation();
            let x = f32::from(event.position.x);
            this.state.update(cx, |state, _| {
                if event.click_count >= 2 {
                    state.release_work_area();
                    state.work_area_to_sequence();
                } else {
                    state.press_work_area(grip, x);
                }
            });
            cx.notify();
        })
    };
    let (on_bar, on_start, on_end) = (
        press(WorkAreaGrip::Bar),
        press(WorkAreaGrip::Start),
        press(WorkAreaGrip::End),
    );
    let end_grip = |id: &'static str, on_press| {
        div()
            .id(id)
            .debug_selector(move || id.into())
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(WORK_AREA_GRIP_W))
            .rounded(px(1.0))
            .bg(grip)
            .cursor(CursorStyle::ResizeLeftRight)
            .on_mouse_down(MouseButton::Left, on_press)
    };
    Some(
        div()
            .id("work-area")
            .debug_selector(|| "timeline-work-area".into())
            .absolute()
            .left(px(left))
            .top(px(WORK_AREA_TOP))
            .w(px(right - left))
            .h(px(WORK_AREA_H))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(fill)
            .cursor(CursorStyle::OpenHand)
            .on_mouse_down(MouseButton::Left, on_bar)
            .child(end_grip("work-area-start", on_start).left_0())
            .child(end_grip("work-area-end", on_end).right_0())
            .into_any_element(),
    )
}

/// The ruler's ticks and labels, painted in one pass over the visible width.
///
/// The spacing comes from [`ruler_scale`] with the label width measured in
/// the ruler's own font, so labels never overlap at any zoom, and each label
/// reads in the sequence's time display.
fn ruler_ticks(
    theme: &Theme,
    time: RulerTime,
    px_per_tick: f32,
    scroll_x: f32,
) -> impl IntoElement {
    let major_color = theme.border_strong;
    let minor_color = theme.border;
    let label_color = theme.text_faint;
    // Tabular: the label width below is a count of digit advances.
    let font = theme.figures_font(gpui::FontWeight::NORMAL);
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<gpui::Pixels>, (), window, cx| {
            if px_per_tick <= 0.0 {
                return;
            }
            let font_size = Theme::TEXT_UI;
            let text_system = window.text_system().clone();
            let font_id = text_system.resolve_font(&font);
            let digit_px = text_system
                .advance(font_id, font_size, '0')
                .map_or(f32::from(font_size) * 0.6, |advance| {
                    f32::from(advance.width)
                });
            let label_px =
                ruler_label_chars(time.rate, time.display, time.last_tick) as f32 * digit_px;
            let scale = ruler_scale(time.rate, time.display, px_per_tick, label_px);
            let width = f32::from(bounds.size.width);
            let height = f32::from(bounds.size.height);
            // From the labelled tick at or before the left edge, so a label
            // that straddles the edge still draws its visible part.
            let first = (scroll_x / px_per_tick).floor() as i64;
            let mut tick = (first.div_euclid(scale.major) * scale.major).max(0);
            let last = ((scroll_x + width) / px_per_tick).ceil() as i64;
            // A row of ticks and labels in a layer of its own, so it does not
            // grow the index the rest of the timeline is ranked in.
            window.paint_layer(bounds, |window| {
                while tick <= last {
                    let x = tick as f32 * px_per_tick - scroll_x;
                    let major = tick % scale.major == 0;
                    let (top, tall, color) = if major {
                        (0.0, height, major_color)
                    } else {
                        (height - MINOR_TICK_H, MINOR_TICK_H, minor_color)
                    };
                    // A tick on the ruler's first pixel would stand against
                    // the outline column's divider and read as one doubled
                    // line; the divider already marks that time.
                    if x >= 1.0 {
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin + point(px(x), px(top)),
                                size(px(1.0), px(tall)),
                            ),
                            color,
                        ));
                    }
                    if major {
                        let text = SharedString::from(
                            crate::timecode::display_time(tick, time.rate, time.display)
                                .to_string(),
                        );
                        let run = TextRun {
                            len: text.len(),
                            font: font.clone(),
                            color: label_color,
                            background_color: None,
                            underline: None,
                            strikethrough: None,
                            tracking: Default::default(),
                        };
                        let line = text_system.shape_line(text, font_size, &[run], None);
                        let _ = line.paint(
                            bounds.origin + point(px(x + Theme::SPACE_XS), px(1.0)),
                            font_size + px(4.0),
                            TextAlign::Left,
                            None,
                            window,
                            cx,
                        );
                    }
                    tick += scale.minor;
                }
            });
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .w_full()
    .h(px(RULER_TICKS_H))
}

pub(super) fn snap_guide_line(
    theme: &Theme,
    guide: crate::snap::SnapCandidate,
    pps: f32,
    fps: f32,
    scroll_x: f32,
    glow: f32,
) -> impl IntoElement {
    let x = ticks_to_px(guide.tick, fps, pps) - scroll_x;
    let line = div()
        .absolute()
        .left(px(x))
        .top_0()
        .bottom_0()
        .w(px(1.))
        .bg(theme.snap_guide)
        .child(
            div()
                .absolute()
                .left(px(-Theme::SPACE_XS))
                .top_0()
                .bottom_0()
                .w(px(Theme::SPACE_SM + 1.0))
                .bg(theme.snap_guide.opacity(glow * 0.35)),
        )
        .child(motion::menu_in(
            guide.guide_key().with("label"),
            div()
                .absolute()
                .top(px(2.))
                .left(px(3.))
                .px(px(3.))
                .rounded(px(Theme::CONTROL_RADIUS))
                .bg(theme.surface_card)
                .text_size(Theme::TEXT_SMALL)
                .text_color(theme.snap_guide)
                .child(guide.kind.label()),
        ));
    motion::fade_quick(guide.guide_key(), line)
}

/// Pixel geometry `(left_px, width_px, is_range)` for a marker on the timeline ruler.
///
/// Point markers (`duration_ticks <= 0`) draw as a 6px flag centered on `tick`.
/// Range markers (`duration_ticks > 0`) draw as a bar spanning `[tick, tick + duration_ticks]`.
pub(crate) fn marker_ruler_span(
    tick: i64,
    duration_ticks: i64,
    fps: f32,
    pps: f32,
    scroll_x: f32,
) -> (f32, f32, bool) {
    let x0 = ticks_to_px(tick, fps, pps) - scroll_x;
    if duration_ticks > 0 {
        let x1 = ticks_to_px(tick.saturating_add(duration_ticks), fps, pps) - scroll_x;
        let w = (x1 - x0).max(6.0);
        (x0, w, true)
    } else {
        (x0 - 3.0, 6.0, false)
    }
}

#[cfg(test)]
mod tests {
    use super::marker_ruler_span;

    #[test]
    fn duration_marker_draws_as_range_on_ruler() {
        let (point_left, point_w, point_is_range) = marker_ruler_span(24, 0, 24.0, 120.0, 0.0);
        assert!(!point_is_range);
        assert_eq!(point_w, 6.0);
        assert_eq!(point_left, 120.0 - 3.0);

        let (range_left, range_w, range_is_range) = marker_ruler_span(24, 24, 24.0, 120.0, 0.0);
        assert!(range_is_range);
        assert_eq!(range_left, 120.0);
        assert_eq!(range_w, 120.0);
    }
}
