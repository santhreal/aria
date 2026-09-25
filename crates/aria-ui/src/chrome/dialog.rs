//! The card a dialog is drawn on.
//!
//! Shared chrome, not a registry: which overlay draws is
//! [`crate::plugins::overlay_host`]'s answer, and what it looks like once it is
//! on screen is this file's. Every shipped dialog draws through
//! [`dialog_surface`], so "a dialog is leaving" is decided once rather than once
//! per dialog.
//!
//! A card is never larger than the window less [`EDGE_CLEARANCE`] on each side.
//! The title row and the header controls stay at the top of the card, the action
//! row stays at the bottom, and the content between them scrolls when it does
//! not fit.

use gpui::{
    AnyElement, App, Div, Entity, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::app::{AppState, DialogKind};
use crate::plugins::overlay_host::{OverlayWhen, exit_t};
use crate::theme::Theme;

#[cfg(test)]
#[path = "dialog_tests.rs"]
mod dialog_tests;

/// Width of a dialog card. One number, so a dialog opened over another lands on
/// the same edges instead of resizing under the pointer.
const DIALOG_WIDTH: f32 = 480.0;

/// Space kept clear between a card and each window edge. A window smaller than
/// the card plus this margin shrinks the card; the margin stays.
const EDGE_CLEARANCE: f32 = 2.0 * Theme::SPACE_LG;

/// What a dialog puts on its card, by region.
///
/// The title row and [`DialogBody::header`] are pinned to the top of the card,
/// the [`DialogBody::action`] row to the bottom. The content fills the space
/// between them and scrolls once the card is at its size limit.
pub struct DialogBody {
    content: Div,
    header: Option<AnyElement>,
    actions: Vec<AnyElement>,
}

/// Elements have no `Debug`; this prints which regions the body has.
impl std::fmt::Debug for DialogBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogBody")
            .field("header", &self.header.is_some())
            .field("actions", &self.actions.len())
            .finish_non_exhaustive()
    }
}

impl DialogBody {
    /// A body whose scrolling region is `content`. The layout set on `content`
    /// (direction, gap) is kept; the card adds the padding and the scrolling.
    pub fn new(content: Div) -> Self {
        Self {
            content,
            header: None,
            actions: Vec::new(),
        }
    }

    /// A control pinned under the title, such as a filter or a query field.
    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    /// A control in the pinned action row, right of the ones added before it.
    /// A body with no action draws no action row.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

/// The scrim, the frosted card, and whichever of the entry and the exit the
/// dialog of `kind` is in this frame.
///
/// Every shipped dialog draws through this, so the exit fade is defined once
/// rather than once per dialog.
pub fn dialog_surface(
    id: &'static str,
    kind: DialogKind,
    title: impl Into<String>,
    state: &Entity<AppState>,
    cx: &App,
    body: DialogBody,
) -> AnyElement {
    let exit = exit_t(OverlayWhen::Dialog(kind), state, cx);
    dialog_layer(id, title, exit, cx, body)
}

/// The scrim and the frosted card for a dialog that is `exit` of the way
/// through leaving, or arriving when `exit` is `None`.
///
/// [`dialog_surface`] reads `exit` from the overlay host. A caller drawing a
/// card with no host behind it, such as the component gallery, passes `None`.
///
/// The frost sits on the card and not on the scrim: the blur separates the card
/// from the editor behind it, and a blurred scrim would only smear the editor.
pub fn dialog_layer(
    id: &'static str,
    title: impl Into<String>,
    exit: Option<f32>,
    cx: &App,
    body: DialogBody,
) -> AnyElement {
    let theme = Theme::of(cx);
    // Reduced motion schedules no animation frames, so nothing would arrive to
    // finish a fade with: a leaving dialog is simply gone.
    if exit.is_some() && cx.reduce_motion() {
        return div().into_any_element();
    }
    let DialogBody {
        content,
        header,
        actions,
    } = body;
    let header = div()
        .flex_none()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_LG))
        .py(px(Theme::SPACE_MD))
        .border_b_1()
        .border_color(theme.border)
        .debug_selector(|| "dialog-header".into())
        .child(
            div()
                .text_size(px(14.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .children(header);
    // Automatic minimum height of a scroll container is zero, so this is the
    // region that gives up height when the card reaches its limit.
    let content = content
        .id("dialog-content")
        .overflow_y_scroll()
        .p(px(Theme::SPACE_LG))
        .debug_selector(|| "dialog-content".into());
    let footer = (!actions.is_empty()).then(|| {
        div()
            .flex_none()
            .flex()
            .flex_row()
            .justify_end()
            .gap(px(Theme::SPACE_SM))
            .px(px(Theme::SPACE_LG))
            .py(px(Theme::SPACE_MD))
            .border_t_1()
            .border_color(theme.border)
            .debug_selector(|| "dialog-footer".into())
            .children(actions)
    });
    let card = div()
        .flex()
        .flex_col()
        .w(px(DIALOG_WIDTH))
        .max_w_full()
        .max_h_full()
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_dialog)
        .border_1()
        .border_color(theme.border)
        .debug_selector(|| "dialog-card".into())
        .child(header)
        .child(content)
        .children(footer);
    // Entry and exit take different element ids. GPUI keys animation state by
    // id, so a shared one would hand the exit the finished entry's clock and the
    // exit would never draw a frame. The two helpers name their own kind, so one
    // identity is enough.
    let card = match exit {
        Some(t) => crate::motion::menu_out(id, t, card).into_any_element(),
        None => crate::motion::dialog_in(id, card).into_any_element(),
    };
    // The padding is the edge clearance, and the card's `max_*_full` resolves
    // against the box inside it.
    div()
        .absolute()
        .inset_0()
        .flex()
        .items_center()
        .justify_center()
        .p(px(EDGE_CLEARANCE))
        .bg(theme.scrim().opacity(1.0 - exit.unwrap_or(0.0)))
        .child(crate::frost::frost(card))
        .into_any_element()
}
