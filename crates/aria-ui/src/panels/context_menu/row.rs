//! Row: 1 helper from [`super`].
//!
//! Split out of `context_menu_drawn.rs` to stay under the file ceiling. A pure
//! move. The item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;

/// One menu row.
///
/// `id` and `label` are owned: a catalog entry's text comes from a data file
/// now, so a caller cannot hand over a `&'static str` for it.
pub(super) fn item(
    id: impl Into<motion::ControlId>,
    label: impl Into<gpui::SharedString>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::AnyElement {
    let text = theme.text;
    let label = label.into();
    // The row id doubles as the fade key: it is already unique per row, and a
    // shared key would make one hovered row light up several.
    let id = id.into();
    let fade = id.motion("ctx-item");
    div()
        .id(id.element("ctx-item"))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(13.))
        .text_color(text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            fade,
            theme.wash(0.0),
            theme.glass_hover(),
        ))
        .on_hover(motion::hover_listener(fade))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    action(state);
                    state.close_context();
                });
                cx.notify();
            }),
        )
        .into_any_element()
}
