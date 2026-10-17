//! Rulers and guides over the Program view.
//!
//! Drawn from the frame the last paint recorded, the same rectangle the
//! transform handles are placed in, so the numbers and the lines follow the
//! fit, zoom, and pan of the picture. A press on a ruler pulls out a guide and
//! a press on a guide grabs it; the pane's own move and release listeners carry
//! the drag.

use super::*;
use crate::app::viewer_guides::{GuideAxis, RULER_PX, guide_screen, ruler_ticks};
use crate::theme::Theme;

/// Length of a numbered tick, in screen pixels.
const TICK_PX: f32 = 6.0;

/// Half the width of the strip a guide can be grabbed by.
const GRAB_PX: f32 = 3.0;

/// The rulers and the guide lines, positioned inside the Program pane.
pub(super) fn rulers_and_guides(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let read = state.read(cx);
    let guides = &read.viewer_guides;
    let (Some(container), Some(canvas)) = (read.overlay_frame.container, read.overlay_frame.canvas)
    else {
        return Vec::new();
    };
    let size = read.canvas_size();
    let mut out = Vec::new();
    // Hidden guides are kept in the state and drawn by nothing.
    let drawn = if guides.hidden {
        &[][..]
    } else {
        &guides.guides[..]
    };
    for (index, guide) in drawn.iter().enumerate() {
        let Some(screen) = guide_screen(*guide, canvas, size) else {
            continue;
        };
        let line = div()
            .id(gpui::ElementId::named_usize("viewer-guide", index))
            .absolute()
            .flex()
            .justify_center()
            .items_center()
            .cursor(match guide.axis {
                GuideAxis::Vertical => gpui::CursorStyle::ResizeLeftRight,
                GuideAxis::Horizontal => gpui::CursorStyle::ResizeUpDown,
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state
                        .update(cx, |state, _| state.viewer_guides.grab(index));
                    cx.notify();
                }),
            );
        let line = match guide.axis {
            GuideAxis::Vertical => line
                .top_0()
                .bottom_0()
                .left(px(screen - container.x - GRAB_PX))
                .w(px(GRAB_PX * 2.0))
                .child(div().h_full().w(px(1.)).bg(theme.accent)),
            GuideAxis::Horizontal => line
                .left_0()
                .right_0()
                .top(px(screen - container.y - GRAB_PX))
                .h(px(GRAB_PX * 2.0))
                .child(div().w_full().h(px(1.)).bg(theme.accent)),
        };
        out.push(line.into_any_element());
    }
    if guides.rulers {
        let [w, h] = [size[0].max(1) as f32, size[1].max(1) as f32];
        let top = ruler_ticks(
            container.x + RULER_PX,
            container.x + container.w,
            canvas.x,
            canvas.w / w,
        );
        let left = ruler_ticks(
            container.y + RULER_PX,
            container.y + container.h,
            canvas.y,
            canvas.h / h,
        );
        out.push(ruler(GuideAxis::Horizontal, top, container, theme, cx));
        out.push(ruler(GuideAxis::Vertical, left, container, theme, cx));
        // The corner where the two strips meet.
        out.push(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size(px(RULER_PX))
                .bg(theme.surface)
                .border_r_1()
                .border_b_1()
                .border_color(theme.border)
                .into_any_element(),
        );
    }
    out
}

/// One ruler strip. `axis` is the guide a press on it pulls out: the top ruler
/// pulls a horizontal guide and numbers x, the left ruler a vertical guide and
/// numbers y.
fn ruler(
    axis: GuideAxis,
    ticks: Vec<(f32, i64)>,
    container: crate::app::canvas_overlay::Rect,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let label = theme.text_faint;
    let mark = theme.text_muted;
    let strip = div()
        .id(match axis {
            GuideAxis::Horizontal => "viewer-ruler-top",
            GuideAxis::Vertical => "viewer-ruler-left",
        })
        .absolute()
        .overflow_hidden()
        .bg(theme.surface)
        .border_color(theme.border)
        .text_size(Theme::TEXT_MICRO)
        .text_color(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                let at = [f32::from(event.position.x), f32::from(event.position.y)];
                this.state.update(cx, |state, _| state.pull_guide(axis, at));
                cx.notify();
            }),
        );
    match axis {
        GuideAxis::Horizontal => strip
            .top_0()
            .left(px(RULER_PX))
            .right_0()
            .h(px(RULER_PX))
            .border_b_1()
            .cursor(gpui::CursorStyle::ResizeUpDown)
            .children(ticks.into_iter().map(|(screen, value)| {
                div()
                    .absolute()
                    .left(px(screen - container.x - RULER_PX))
                    .bottom_0()
                    .h(px(RULER_PX))
                    .flex()
                    .items_start()
                    .child(
                        div()
                            .w(px(1.))
                            .h(px(TICK_PX))
                            .mt(px(RULER_PX - TICK_PX))
                            .bg(mark),
                    )
                    .child(div().pl(px(2.)).child(value.to_string()))
            }))
            .into_any_element(),
        GuideAxis::Vertical => strip
            .left_0()
            .top(px(RULER_PX))
            .bottom_0()
            .w(px(RULER_PX))
            .border_r_1()
            .cursor(gpui::CursorStyle::ResizeLeftRight)
            .children(ticks.into_iter().map(|(screen, value)| {
                // Digits stacked one per line: a horizontal number does not
                // fit across a ruler this narrow.
                let digits: Vec<String> = value.to_string().chars().map(String::from).collect();
                div()
                    .absolute()
                    .top(px(screen - container.y - RULER_PX))
                    .left_0()
                    .w(px(RULER_PX))
                    .flex()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .pt(px(2.))
                            .pl(px(2.))
                            .line_height(px(8.))
                            .children(digits),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .h(px(1.))
                            .w(px(TICK_PX))
                            .bg(mark),
                    )
            }))
            .into_any_element(),
    }
}
