//! Peak: 1 helpers from [`super`].
//!
//! Split out of `preview.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;

/// Two peak bars and a clip light, for the transport row.
///
/// Full scale is the width of the strip, so a bar that reaches the end is at 0
/// dBFS: the last thing an operator needs while mixing is arithmetic. Clicking
/// the strip acknowledges a clip.
pub(super) fn peak_meter(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    const WIDTH: f32 = 56.0;
    let reading = state.read(cx).meter();
    let bar = |level: f32, id: &'static str| {
        div()
            .id(id)
            .w(px(WIDTH))
            .h(px(3.))
            .rounded(px(1.))
            .bg(theme.wash(0.12))
            .child(
                div()
                    .w(px((WIDTH * level.clamp(0.0, 1.0)).round()))
                    .h_full()
                    .rounded(px(1.))
                    .bg(if reading.clipped {
                        theme.danger
                    } else {
                        theme.accent
                    }),
            )
    };
    div()
        .id("preview-meter")
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .justify_center()
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .when_some(crate::tooltip::builder("preview-meter"), |el, tip| {
            el.with_tip(tip)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.clear_clip());
                cx.notify();
            }),
        )
        .child(bar(reading.peaks[0], "preview-meter-l"))
        .child(bar(reading.peaks[1], "preview-meter-r"))
}
