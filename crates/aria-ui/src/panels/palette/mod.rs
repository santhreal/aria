//! Command palette (Ctrl+Shift+P).
//!
//! The panel draws what [`AppState::palette_hits`] offers: the command registry,
//! every menu row the commands did not already name, the Effect menu's rows
//! among them, and every generator and clip type in the model's catalogs. The
//! panel holds no table of its own: the rows live in [`crate::commands`] and the
//! offer in [`crate::app::palette`], so the keymap, the menu bar and the effect
//! catalog reach a row without going through this file.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::palette::{Offered, PaletteOffer};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::fluent::FluentBuilder;
use crate::theme::Theme;

#[cfg(test)]
#[path = "scroll_tests.rs"]
mod scroll_tests;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let query = state.read(cx).palette_query.clone();
    let theme = Theme::of(cx);
    let hits = state.read(cx).palette_hits();
    let highlight = state.read(cx).palette_highlight();
    let scroll = state.read(cx).palette_scroll.clone();
    let keys = state.read(cx).ctx.context_keys();
    let mut rows = Vec::with_capacity(hits.len());
    for (at, offer) in hits.iter().enumerate() {
        rows.push(
            row(&theme, offer.clone(), keys, at, highlight == Some(at), cx).into_any_element(),
        );
    }
    if rows.is_empty() {
        rows.push(
            div()
                .flex()
                .items_center()
                .h(px(Theme::ROW_HEIGHT))
                .px(px(Theme::SPACE_SM))
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("No command matches")
                .into_any_element(),
        );
    }
    // The palette takes every key while it is open, so its box always holds
    // focus.
    let query_row =
        crate::components::text_box("palette-query", query, "Type to filter…", true, &theme);
    // One row height and no gap, as a menu's rows: the palette is the menus
    // filtered, and reads at their pitch.
    let list = div().flex().flex_col().children(rows);
    dialog_surface(
        "palette",
        DialogKind::Palette,
        "Command Palette",
        state,
        cx,
        DialogBody::new(list)
            .header(query_row)
            .track_scroll(&scroll),
    )
}

/// One offer. A plugin command draws its icon, and is dimmed while its
/// enablement condition is false under `keys`; choosing it then reports the
/// condition it needs.
fn row(
    theme: &Theme,
    offer: PaletteOffer,
    keys: aria_plugin::ContextKeys,
    index: usize,
    highlighted: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let source = offer.source.map(|menu| menu.label());
    let (icon, enabled) = match &offer.action {
        Offered::Plugin(spec) => (
            spec.icon.as_ref().map(crate::icons::plugin_icon_path),
            spec.enabled(keys),
        ),
        Offered::ClipType {
            icon, placeable, ..
        } => (*icon, *placeable),
        _ => (None, true),
    };
    // The palette is drawn by the Shell: a gpui hover style here would notify
    // it and rebuild every panel body, where a fade redraws the chrome alone.
    let key = crate::motion::MotionKey::new("palette-row", index);
    let rest = if highlighted {
        theme.surface_raised_hover
    } else {
        theme.wash(0.0)
    };
    div()
        .id(offer.id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .h(px(Theme::ROW_HEIGHT))
        .flex_none()
        .rounded(px(Theme::CONTROL_RADIUS))
        .when(highlighted, |el| {
            el.debug_selector(|| "palette-highlight".into())
        })
        .text_size(Theme::TEXT_UI)
        .text_color(if enabled {
            theme.text
        } else {
            theme.text_faint
        })
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(crate::motion::hover_blend(key, rest, theme.glass_hover()))
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
                .child(div().min_w_0().truncate().child(offer.label)),
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
