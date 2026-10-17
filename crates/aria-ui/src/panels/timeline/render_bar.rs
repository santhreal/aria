//! The 3px render-readiness bar along the bottom of the timeline ruler.

use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, px};

use crate::app::{RenderQuality, RenderSegment};
use crate::theme::Theme;

use super::ticks_to_px;

/// Height of the render bar strip at the bottom of the timeline ruler.
pub(super) const RENDER_BAR_H: f32 = 3.0;

/// Paint the `3px` render bar strip over `[0.0, viewport]` from `segments`.
pub(super) fn render_bar(
    theme: &Theme,
    segments: &[RenderSegment],
    pps: f32,
    fps: f32,
    scroll_x: f32,
    viewport: f32,
) -> impl IntoElement {
    let mut children = Vec::new();
    for seg in segments {
        let x0 = (ticks_to_px(seg.start_tick, fps, pps) - scroll_x).max(0.0);
        let x1 = (ticks_to_px(seg.end_tick, fps, pps) - scroll_x).min(viewport);
        if x1 <= x0 {
            continue;
        }
        let color = match seg.quality {
            RenderQuality::Green => theme.success,
            RenderQuality::Yellow => theme.warning,
            RenderQuality::Red => theme.danger,
        };
        children.push(
            div()
                .absolute()
                .left(px(x0))
                .bottom_0()
                .w(px((x1 - x0).max(1.0)))
                .h(px(RENDER_BAR_H))
                .bg(color),
        );
    }
    div()
        .id("render-bar")
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(RENDER_BAR_H))
        .children(children)
}
