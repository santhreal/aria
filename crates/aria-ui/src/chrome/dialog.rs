//! The card a dialog is drawn on.
//!
//! Shared chrome, not a registry: which overlay draws is
//! [`crate::plugins::overlay_host`]'s answer, and what it looks like once it is
//! on screen is this file's. Every shipped dialog draws through
//! [`dialog_surface`], so "a dialog is leaving" is decided once rather than once
//! per dialog.
//!
//! A card is never larger than the window less `EDGE_CLEARANCE` on each side.
//! The title row and the header controls stay at the top of the card, the action
//! row stays at the bottom, and the content between them scrolls when it does
//! not fit. A card stands centered in the window, or, with
//! [`DialogBody::anchored_top`], at a fixed share of the height under its top.

use std::ops::Range;

use gpui::{
    AnyElement, App, Div, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, Styled, Window, div, px, relative,
};

use crate::app::{AppState, DialogKind};
use crate::plugins::overlay_host::{OverlayWhen, exit_t};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

#[cfg(test)]
#[path = "dialog_tests.rs"]
mod dialog_tests;

/// Width of a dialog card. One number, so a dialog opened over another lands on
/// the same edges instead of resizing under the pointer.
const DIALOG_WIDTH: f32 = 528.0;

/// Width of the label column in [`setting_row`]. The widest label, Maintain
/// Audio Pitch, measures 106.5 px at [`Theme::TEXT_UI`].
pub(crate) const LABEL_WIDTH: f32 = 120.0;

#[cfg(test)]
thread_local! {
    static SETTING_ROWS: std::cell::RefCell<Option<Vec<&'static str>>> =
        const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
fn record_setting_row(label: &'static str) {
    SETTING_ROWS.with_borrow_mut(|rows| {
        if let Some(rows) = rows {
            rows.push(label);
        }
    });
}

#[cfg(test)]
pub(super) fn record_setting_rows() {
    SETTING_ROWS.set(Some(Vec::new()));
}

#[cfg(test)]
pub(super) fn drawn_setting_rows() -> Vec<&'static str> {
    SETTING_ROWS.with_borrow_mut(|rows| rows.take().unwrap_or_default())
}

/// Space kept clear between a card and each window edge. A window smaller than
/// the card plus this margin shrinks the card; the margin stays.
const EDGE_CLEARANCE: f32 = 2.0 * Theme::SPACE_LG;

/// Where the top of a [`DialogBody::anchored_top`] card stands: this share of
/// the height inside the edge clearance lies above it.
const ANCHORED_TOP: f32 = 0.15;

/// What a dialog puts on its card, by region.
///
/// The title row and [`DialogBody::header`] are pinned to the top of the card,
/// the [`DialogBody::action`] row to the bottom. The content fills the space
/// between them and scrolls once the card is at its size limit.
pub struct DialogBody {
    content: Content,
    header: Option<AnyElement>,
    actions: Vec<AnyElement>,
    anchored_top: bool,
}

/// What scrolls between a card's header and its action row.
enum Content {
    /// A `div` with every child laid out.
    Div(Div),
    /// Rows of one height, only those in view laid out.
    Rows(crate::edge_fade::ScrollFrame),
}

/// Elements have no `Debug`; this prints which regions the body has.
impl std::fmt::Debug for DialogBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DialogBody")
            .field("header", &self.header.is_some())
            .field("actions", &self.actions.len())
            .field("rows", &matches!(self.content, Content::Rows(_)))
            .field("anchored_top", &self.anchored_top)
            .finish_non_exhaustive()
    }
}

impl DialogBody {
    /// A body whose scrolling region is `content`. The layout set on `content`
    /// (direction, gap) is kept; the card adds the padding and the scrolling.
    pub fn new(content: Div) -> Self {
        Self {
            content: Content::Div(content),
            header: None,
            actions: Vec::new(),
            anchored_top: false,
        }
    }

    /// A body whose scrolling region is `rows` rows `row` px tall, which
    /// `render` draws by range. Only the rows in view are drawn, so a list of
    /// a thousand rows costs a frame what a list of ten does. Row `reveal` is
    /// brought into view as [`crate::edge_fade::scroll_rows`] brings it, and
    /// the region keeps its place under `id`: a new `id` starts the list at
    /// its top.
    pub fn rows<R: IntoElement>(
        id: impl Into<gpui::ElementId>,
        rows: usize,
        row: gpui::Pixels,
        reveal: Option<usize>,
        render: impl Fn(Range<usize>, &mut Window, &mut App) -> Vec<R> + 'static,
    ) -> Self {
        let frame =
            crate::edge_fade::scroll_rows(id, rows, row, px(Theme::SPACE_LG), reveal, render);
        Self {
            content: Content::Rows(frame),
            ..Self::new(div())
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

    /// Stands the card's top at a fixed height in the window rather than
    /// centering the card, so the card grows and shrinks downward. A dialog
    /// whose content changes as the query changes, such as the palette's list,
    /// keeps its query field under the caret.
    pub fn anchored_top(mut self) -> Self {
        self.anchored_top = true;
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

/// One setting row in a dialog body: `label` in a `LABEL_WIDTH` column on
/// one line, then `value` beside it.
pub fn setting_row(theme: &Theme, label: &'static str, value: impl IntoElement) -> Div {
    #[cfg(test)]
    record_setting_row(label);
    div()
        .debug_selector(move || format!("dialog-setting-{label}"))
        .flex()
        .flex_row()
        .items_start()
        .gap(px(Theme::SPACE_MD))
        .line_height(px(Theme::CONTROL_HEIGHT))
        .child(
            crate::components::row_label(theme, label, LABEL_WIDTH)
                .debug_selector(move || format!("dialog-label-{label}"))
                .line_height(px(Theme::CONTROL_HEIGHT))
                .whitespace_nowrap(),
        )
        .child(value)
}

/// A heading over a group of [`setting_row`]s, on the label column's left
/// edge.
pub fn setting_heading(theme: &Theme, text: &'static str) -> Div {
    div()
        .debug_selector(move || format!("dialog-heading-{text}"))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_muted)
        .child(text)
}

/// One boolean setting row in a dialog body: `label` in the [`setting_row`]
/// label column and the switch `id`, on for `on`, beside it. A press runs
/// `toggle`.
pub fn toggle_row(
    theme: &Theme,
    id: &'static str,
    label: &'static str,
    on: bool,
    toggle: fn(&mut AppState),
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Div {
    setting_row(
        theme,
        label,
        div()
            .h(px(Theme::CONTROL_HEIGHT))
            .flex()
            .items_center()
            .child(
                crate::components::switch(id, Some(on), theme)
                    .debug_selector(|| id.to_owned())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| toggle(state));
                            cx.notify();
                        }),
                    ),
            ),
    )
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
    id: impl Into<crate::motion::ControlId>,
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
        anchored_top,
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
                .debug_selector(|| "dialog-title".into())
                .text_size(Theme::TEXT_HEADING)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .child(title.into()),
        )
        .children(header);
    // Automatic minimum height of a scroll container is zero, so this is the
    // region that gives up height when the card reaches its limit.
    let content = match content {
        Content::Div(content) => crate::edge_fade::scroll_y(
            content
                .id("dialog-content")
                .p(px(Theme::SPACE_LG))
                .debug_selector(|| "dialog-content".into()),
        )
        .into_any_element(),
        // The rows pad themselves inside the frame, so they scroll under the
        // padding as a `div`'s children do. The column passes the height the
        // card leaves it on to the frame.
        Content::Rows(rows) => div()
            .flex()
            .flex_col()
            .min_h_0()
            .debug_selector(|| "dialog-content".into())
            .child(rows)
            .into_any_element(),
    };
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
        .children(footer)
        // Under the anchored spacer the card shrinks into the height left; its
        // automatic minimum would hold it at its content's height instead.
        .when(anchored_top, |card| card.min_h_0());
    // Entry and exit take different element ids. GPUI keys animation state by
    // id, so a shared one would hand the exit the finished entry's clock and the
    // exit would never draw a frame. The two helpers name their own kind, so one
    // identity is enough.
    let card = match exit {
        Some(t) => crate::motion::menu_out(id, t, card).into_any_element(),
        None => crate::motion::dialog_in(id, card).into_any_element(),
    };
    // The padding is the edge clearance, and the card's `max_*_full` resolves
    // against the box inside it. The scrim takes the pointer: a dialog is
    // modal, so neither a hover nor a press reaches the editor behind it.
    let layer = div()
        .absolute()
        .inset_0()
        .flex()
        .p(px(EDGE_CLEARANCE))
        .bg(theme.scrim().opacity(1.0 - exit.unwrap_or(0.0)))
        .occlude();
    let layer = if anchored_top {
        // A column: the spacer takes its share of the height, and the card
        // shrinks into the rest, its scrolling content giving up the height.
        layer
            .flex_col()
            .items_center()
            .child(div().flex_none().h(relative(ANCHORED_TOP)))
    } else {
        layer.items_center().justify_center()
    };
    layer.child(crate::frost::frost(card)).into_any_element()
}
