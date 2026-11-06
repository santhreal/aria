//! Command palette (Ctrl+Shift+P).
//!
//! The panel draws what [`AppState::palette_hits`] offers: the command registry,
//! every menu row the commands did not already name, the Effect menu's rows
//! among them, and every generator and clip type in the model's catalogs. The
//! panel holds no table of its own: the rows live in [`crate::commands`] and the
//! offer in [`crate::app::palette`], so the keymap, the menu bar and the effect
//! catalog reach a row without going through this file.

use std::ops::Range;

use gpui::{
    AnyElement, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::palette::{Offered, PaletteOffer};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

#[cfg(test)]
#[path = "scroll_tests.rs"]
mod scroll_tests;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    // The palette takes every key while it is open, so its box always holds
    // focus.
    let query = &state.read(cx).palette_query;
    let query_row = crate::components::text_box(
        "palette-query",
        query,
        "Type to filter…",
        Some(query.selection()),
        &theme,
    );
    let (count, highlight, list) = {
        let state = state.read(cx);
        (
            state.palette_hits().len(),
            state.palette_highlight(),
            state.palette_list,
        )
    };
    let body = if count == 0 {
        DialogBody::new(
            div().flex().flex_col().child(
                div()
                    .flex()
                    .items_center()
                    .h(px(Theme::ROW_HEIGHT))
                    .px(px(Theme::SPACE_SM))
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child("No command matches"),
            ),
        )
    } else {
        // One row height and no gap, as a menu's rows: the palette is the
        // menus filtered, and reads at their pitch. Only the rows in view are
        // drawn, so a key that moves the highlight lays out a screen of rows
        // and not every command.
        DialogBody::rows(
            ("palette-rows", list),
            count,
            px(Theme::ROW_HEIGHT),
            highlight,
            cx.processor(
                |shell: &mut crate::shell::Shell, range: Range<usize>, _, cx| {
                    rows(shell, range, cx)
                },
            ),
        )
    };
    dialog_surface(
        "palette",
        DialogKind::Palette,
        "Command Palette",
        state,
        cx,
        body.header(query_row).anchored_top(),
    )
}

/// The rows of the hits in `range`.
fn rows(
    shell: &mut crate::shell::Shell,
    range: Range<usize>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let theme = Theme::of(cx);
    let offers: Vec<(usize, PaletteOffer, bool, bool)> = {
        let state = shell.state.read(cx);
        let hits = state.palette_hits();
        let highlight = state.palette_highlight();
        let keys = state.ctx.context_keys();
        range
            .filter_map(|at| {
                let offer = hits.get(at)?;
                let live = offer.runs_now(state, keys);
                Some((at, offer.clone(), live, highlight == Some(at)))
            })
            .collect()
    };
    offers
        .into_iter()
        .map(|(at, offer, live, highlighted)| {
            row(&theme, offer, live, at, highlighted, cx).into_any_element()
        })
        .collect()
}

/// One offer, drawn faint while it does not run now
/// ([`PaletteOffer::runs_now`]): a menu row its menu draws disabled, a plugin
/// command whose enablement condition is false, a clip type that cannot be
/// placed. Choosing it then reports why. A plugin command and a clip type draw
/// their icon.
fn row(
    theme: &Theme,
    offer: PaletteOffer,
    live: bool,
    index: usize,
    highlighted: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let source = offer.source.map(|menu| menu.label());
    let icon = match &offer.action {
        Offered::Plugin(spec) => spec.icon.as_ref().map(crate::icons::plugin_icon_path),
        Offered::ClipType { icon, .. } => *icon,
        _ => None,
    };
    // The palette is drawn by the Shell: a gpui hover style here would notify
    // it and rebuild every panel body, where a fade redraws the chrome alone.
    let key = crate::motion::MotionKey::new("palette-row", index);
    let withheld = (!live).then(|| offer.id.clone());
    div()
        .id(offer.id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .w_full()
        .h(px(Theme::ROW_HEIGHT))
        .rounded(px(Theme::CONTROL_RADIUS))
        .when(highlighted, |el| {
            el.debug_selector(|| "palette-highlight".into())
        })
        .text_size(Theme::TEXT_UI)
        .text_color(if live { theme.text } else { theme.text_faint })
        .cursor(if live {
            CursorStyle::PointingHand
        } else {
            CursorStyle::Arrow
        })
        .when(live, |el| el.active(|style| style.opacity(0.92)))
        .bg(crate::motion::row_fill(key, highlighted, theme))
        .on_hover(crate::motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                crate::commands::dispatch(this, Some(index), window, cx);
            }),
        )
        .child(
            div()
                .flex()
                .items_center()
                .min_w_0()
                .gap(px(Theme::SPACE_SM))
                .children(icon.map(crate::icons::icon))
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .when_some(withheld, |el, id| {
                            el.debug_selector(move || format!("palette-withheld-{id}"))
                        })
                        .child(offer.label),
                ),
        )
        .children(source.map(|menu| {
            div()
                .flex_none()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(menu)
        }))
}

#[cfg(test)]
#[path = "rows_tests.rs"]
mod rows_tests;

#[cfg(test)]
#[path = "withheld_tests.rs"]
mod withheld_tests;
