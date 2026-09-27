//! Application bar: Home chrome, the menu titles, and the editor tool strip.
//!
//! The bar's frame lives here. What the menu titles drop is drawn by
//! `menu_view`, and the strip under them by `tool_strip`, so the layout and the
//! contents are separate files.

use gpui::{
    App, Context, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::types::AppMenu;

#[path = "chrome/dialog.rs"]
pub mod dialog;
#[path = "chrome/menu_host.rs"]
pub mod menu_host;
#[path = "chrome/menu_items.rs"]
pub mod menu_items;
#[path = "chrome/menu_sequence.rs"]
pub(crate) mod menu_sequence;
#[path = "chrome/menu_view.rs"]
mod menu_view;
#[path = "chrome/native_menus.rs"]
pub mod native_menus;
#[path = "chrome/tool_strip.rs"]
mod tool_strip;

/// Menu labels, for the tests that assert every menu names a real command.
#[cfg(test)]
pub(crate) use menu_items::labels_for;

const MENU_H: f32 = 32.0;

pub fn home_bar(theme: &Theme, window: &mut Window, cx: &mut Context<Shell>) -> impl IntoElement {
    div()
        .id("home-titlebar")
        .h(px(Theme::TITLEBAR_HEIGHT))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px(px(Theme::SPACE_LG))
        .bg(theme.glass())
        .border_b_1()
        .border_color(theme.border)
        .child(
            div()
                .flex_1()
                .text_size(px(14.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child("Aria"),
        )
        .when(cfg!(not(target_os = "macos")), |el| {
            el.child(caption_buttons(theme, window, cx))
        })
}

/// A concrete element rather than `impl IntoElement`: the shell stages this in
/// on open, and an animation needs an element it can restyle. Stateful because
/// the bar carries an id — the menu scrim inside it is hit-tested against it.
pub fn editor_bar(
    theme: &Theme,
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let open = state.read(cx).open_menu;
    div()
        .id("app-bar")
        .w_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.glass())
        .border_b_1()
        .border_color(theme.border)
        .when(open.is_some(), |el| {
            el.child(div().id("menu-scrim").absolute().inset_0().on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.close_menu());
                    cx.notify();
                }),
            ))
        })
        .child(menu_bar(theme, state, window, cx))
        .child(tool_strip::tool_bar(theme, state, cx))
}

fn menu_bar(
    theme: &Theme,
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let open = state.read(cx).open_menu;
    // Once per frame: the drop that is fading out is no longer in the state, so
    // the bar is what remembers it for the length of the exit.
    let drop = menu_view::drop_phase(open, cx.motion_policy());
    div()
        .id("menu-bar")
        .h(px(MENU_H))
        .w_full()
        .flex()
        .flex_row()
        .items_center()
        .px(px(Theme::SPACE_MD))
        .gap(px(Theme::SPACE_XS))
        // The shipped mark, the same art the packagers turn into the app icon.
        // 18px sits on the cap height of the menu words beside it in a 32px bar.
        .child(crate::icons::brand_mark(18.))
        .children(
            AppMenu::ALL
                .into_iter()
                .map(|menu| {
                    let phase = drop.and_then(|(owner, phase)| (owner == menu).then_some(phase));
                    menu_view::menu_drop(menu, phase, theme, state, cx).into_any_element()
                })
                .collect::<Vec<_>>(),
        )
        .child(breadcrumb(theme, state, cx))
        .when(cfg!(not(target_os = "macos")), |el| {
            el.child(caption_buttons(theme, window, cx))
        })
}

fn breadcrumb(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let name = state.read(cx).store.project_name.clone();
    let dirty = state.read(cx).store.is_dirty;
    let key = motion::MotionKey::new("crumb", "home");
    div()
        .id("breadcrumb")
        .flex()
        .flex_1()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(Theme::SPACE_XS))
        .text_size(px(13.))
        .child(
            div()
                .id("crumb-home")
                .text_color(motion::hover_blend(key, theme.text_muted, theme.text))
                .cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .child("Home")
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| state.request_go_home());
                        cx.notify();
                    }),
                ),
        )
        .child(div().text_color(theme.text_faint).child("/"))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .text_color(theme.text)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .when(dirty, |el| {
                    el.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(theme.accent))
                })
                .child(name),
        )
}

fn caption_buttons(
    theme: &Theme,
    _window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_row()
        .gap(px(Theme::SPACE_XS))
        .child(caption_btn(
            "caption-min",
            crate::icons::MINUS,
            theme,
            cx,
            |window, _cx| window.minimize_window(),
        ))
        .child(caption_btn(
            "caption-max",
            crate::icons::SQUARE,
            theme,
            cx,
            |window, _cx| window.zoom_window(),
        ))
        .child(caption_btn(
            "caption-close",
            crate::icons::CLOSE,
            theme,
            cx,
            |window, _cx| window.remove_window(),
        ))
}

/// One window control. Three glyphs with no words beside them, so each names
/// itself from the tooltip table like every other icon-only control.
fn caption_btn(
    id: &'static str,
    icon: &'static str,
    theme: &Theme,
    _cx: &mut Context<Shell>,
    on_click: fn(&mut Window, &mut App),
) -> impl IntoElement {
    let key = motion::MotionKey::new("caption", id);
    div()
        .id(id)
        .w(px(12.))
        .h(px(12.))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(motion::hover_blend(
            key,
            theme.border_strong,
            theme.text_muted,
        ))
        .text_color(motion::hover_blend(key, theme.border_strong, theme.text))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .tooltip(crate::tooltip::deferred_builder(id))
        .on_hover(motion::hover_listener(key))
        .child(crate::icons::icon(icon).size(px(8.)))
        .on_mouse_down(MouseButton::Left, move |_: &MouseDownEvent, window, cx| {
            on_click(window, cx)
        })
}
