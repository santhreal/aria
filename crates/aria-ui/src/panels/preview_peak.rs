//! The Viewer's peak strip: a bar per output channel, in the control row.

use super::*;
use crate::app::meter::meter_fraction;

/// Two peak bars on the meter scale the Audio Meters panel draws: the left end
/// of a bar is -60 dBFS and the right end 0 dBFS. Both bars draw red from the
/// first full-scale block until a click on the strip, or on the Audio Meters
/// clip light, clears the clip.
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
            .debug_selector(move || id.to_owned())
            .w(px(WIDTH))
            .h(px(3.))
            .rounded(px(1.))
            .bg(theme.wash(0.12))
            .child(
                div()
                    .debug_selector(move || format!("{id}-fill"))
                    .w(px((WIDTH * meter_fraction(level)).round()))
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
