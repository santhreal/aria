//! The card a dialog is drawn on.
//!
//! Shared chrome, not a registry: which overlay draws is
//! [`crate::plugins::overlay_host`]'s answer, and what it looks like once it is
//! on screen is this file's. Every shipped dialog draws through
//! [`dialog_surface`], so "a dialog is leaving" is decided once rather than once
//! per dialog.

use gpui::{AnyElement, Entity, IntoElement, ParentElement, Styled, div, px};

use crate::app::{AppState, DialogKind};
use crate::plugins::overlay_host::{OverlayWhen, exit_t};
use crate::shell::Shell;
use crate::theme::Theme;

/// Width of a dialog card. One number, so a dialog opened over another lands on
/// the same edges instead of resizing under the pointer.
const DIALOG_WIDTH: f32 = 480.0;

/// The scrim, the frosted card, and whichever of the entry and the exit the
/// dialog is in this frame.
///
/// Every shipped dialog draws through this, so "a dialog is leaving" is decided
/// once rather than once per dialog. The frost sits on the card and not on the
/// scrim: the blur is what separates the card from the editor behind it, and a
/// blurred scrim would only smear the editor.
pub fn dialog_surface(
    id: &'static str,
    kind: DialogKind,
    title: impl Into<String>,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<Shell>,
    body: impl IntoElement,
) -> AnyElement {
    let theme = Theme::of(cx);
    let exit = exit_t(OverlayWhen::Dialog(kind), state, cx);
    // Reduced motion schedules no animation frames, so nothing would arrive to
    // finish a fade with: a leaving dialog is simply gone.
    if exit.is_some() && cx.reduce_motion() {
        return div().into_any_element();
    }
    let card = div()
        .w(px(DIALOG_WIDTH))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_dialog)
        .border_1()
        .border_color(theme.border)
        .child(
            div()
                .px(px(Theme::SPACE_LG))
                .py(px(Theme::SPACE_MD))
                .border_b_1()
                .border_color(theme.border)
                .text_size(px(14.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .child(div().p(px(Theme::SPACE_LG)).child(body));
    // Entry and exit take different element ids. GPUI keys animation state by
    // id, so a shared one would hand the exit the finished entry's clock and the
    // exit would never draw a frame. The two helpers name their own kind, so one
    // identity is enough.
    let card = match exit {
        Some(t) => crate::motion::menu_out(id, t, card).into_any_element(),
        None => crate::motion::dialog_in(id, card).into_any_element(),
    };
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .bg(theme.scrim().opacity(1.0 - exit.unwrap_or(0.0)))
        .child(crate::frost::frost(card))
        .into_any_element()
}
