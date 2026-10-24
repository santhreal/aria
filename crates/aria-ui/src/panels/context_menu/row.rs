//! Row: the one builder every context menu row is drawn by.
//!
//! The row is the menu bar's row in a context menu's card: one row height, the
//! lead column [`crate::components::menu_lead`] draws, then the caption, so a
//! check mark, a plugin's icon and a plain row keep one caption column.

use super::*;
use crate::theme::Theme;
use gpui::StatefulInteractiveElement;

/// One menu row, with its check mark drawn while `checked` holds.
///
/// `id` and `label` are owned: a catalog entry's text comes from a data file
/// now, so a caller cannot hand over a `&'static str` for it.
pub(super) fn item(
    id: impl Into<motion::ControlId>,
    label: impl Into<gpui::SharedString>,
    checked: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::AnyElement {
    row(id, label, checked, None, theme, cx, action)
}

/// One menu row with the icon at the asset path `icon` in its lead column.
pub(super) fn item_with_icon(
    id: impl Into<motion::ControlId>,
    label: impl Into<gpui::SharedString>,
    icon: Option<&'static str>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::AnyElement {
    row(id, label, false, icon, theme, cx, action)
}

fn row(
    id: impl Into<motion::ControlId>,
    label: impl Into<gpui::SharedString>,
    checked: bool,
    icon: Option<&'static str>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    action: impl Fn(&mut AppState) + 'static,
) -> gpui::AnyElement {
    let label = label.into();
    // The row id doubles as the fade key: it is already unique per row, and a
    // shared key would make one hovered row light up several.
    let id = id.into();
    let fade = id.motion("ctx-item");
    div()
        .id(id.element("ctx-item"))
        .px(px(Theme::SPACE_SM))
        .h(px(Theme::ROW_HEIGHT))
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            fade,
            theme.wash(0.0),
            theme.glass_hover(),
        ))
        .on_hover(motion::hover_listener(fade))
        .child(crate::components::menu_lead(checked, icon, theme))
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
