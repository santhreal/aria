//! The About card: what build is running and where it keeps its files.
//!
//! Every fact is read when the card draws, so a plugin mounted while the card
//! is up is counted on the next frame.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::theme::Theme;

/// The rows of the card, label then value.
pub(crate) fn facts(state: &AppState) -> [(&'static str, String); 5] {
    [
        ("Version", env!("CARGO_PKG_VERSION").to_string()),
        ("Plugin ABI", aria_plugin::external::ABI_VERSION.to_string()),
        ("Plugins mounted", state.ctx.mounted().len().to_string()),
        ("Data folder", state.data_dir.display().to_string()),
        ("License", env!("CARGO_PKG_LICENSE").to_string()),
    ]
}

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let rows = facts(state.read(cx));

    let reveal = components::ghost_button("about-data", "Open Data Folder", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            let dir = this.state.read(cx).data_dir.clone();
            crate::app::recents::reveal_in_folder(&dir);
        }),
    );
    let close = components::button("about-close", "Close", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );

    let mut table = div().flex().flex_col().gap(px(Theme::SPACE_XS));
    for (label, value) in rows {
        table = table.child(
            div()
                .flex()
                .gap(px(Theme::SPACE_MD))
                .text_size(Theme::TEXT_TITLE)
                .child(
                    div()
                        .w(px(120.))
                        .flex_none()
                        .text_color(theme.text_faint)
                        .child(label),
                )
                .child(div().flex_1().min_w_0().text_color(theme.text).child(value)),
        );
    }
    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child("Aria, a video editor and compositor."),
        )
        .child(table);
    let body = DialogBody::new(body).action(reveal).action(close);
    dialog_surface("about", DialogKind::About, "About Aria", state, cx, body)
}
