//! Application bar: Home chrome, the menu titles, and the editor tool bar.
//!
//! The bar's frame lives here. What the menu titles drop is drawn by
//! `menu_view`, and the strip under them by `tool_strip`, so the layout and the
//! contents are separate files.

use crate::tooltip::WithTip as _;
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
#[path = "chrome/menu_animation.rs"]
mod menu_animation;
#[path = "chrome/menu_effect.rs"]
pub(crate) mod menu_effect;
#[path = "chrome/menu_guards.rs"]
pub(crate) mod menu_guards;
#[path = "chrome/menu_host.rs"]
pub mod menu_host;
#[path = "chrome/menu_items.rs"]
pub mod menu_items;
#[path = "chrome/menu_markers.rs"]
mod menu_markers;
#[path = "chrome/menu_sequence.rs"]
pub(crate) mod menu_sequence;
#[path = "chrome/menu_view.rs"]
mod menu_view;
#[path = "chrome/native_menus.rs"]
pub mod native_menus;
#[path = "chrome/plugin_menus.rs"]
pub(crate) mod plugin_menus;
#[path = "chrome/tool_strip.rs"]
pub(crate) mod tool_strip;
#[path = "chrome/window_frame.rs"]
pub(crate) mod window_frame;

use window_frame::Frame;

/// Menu labels, for the tests that assert every menu names a real command.
#[cfg(test)]
pub(crate) use menu_items::labels_for;

const MENU_H: f32 = 32.0;

pub fn home_bar(theme: &Theme, window: &mut Window, cx: &mut Context<Shell>) -> impl IntoElement {
    let frame = Frame::of(window);
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
                .text_size(Theme::TEXT_HEADING)
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(theme.text)
                .child("Aria"),
        )
        .child(drag_strip("home-drag", frame, cx))
        .when(frame.draws_controls(), |el| {
            el.child(caption_buttons(theme, window, cx))
        })
}

/// A concrete element rather than `impl IntoElement`: the shell stages this in
/// on open, and an animation needs an element it can restyle.
pub fn editor_bar(
    theme: &Theme,
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id("app-bar")
        .w_full()
        .relative()
        .flex()
        .flex_col()
        .bg(theme.glass())
        .border_b_1()
        .border_color(theme.border)
        .child(menu_bar(theme, state, window, cx))
        .child(tool_strip::tool_bar(theme, state, cx))
}

fn menu_bar(
    theme: &Theme,
    state: &Entity<AppState>,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    // A drop a button opened is drawn by the button, so the titles see no open
    // drop and the bar takes no dismissal for it.
    let open = {
        let state = state.read(cx);
        state.open_menu.filter(|_| state.menu_owner.is_none())
    };
    let frame = Frame::of(window);
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
        // A press anywhere outside the bar and the drop closes an open drop and
        // still lands where it was aimed, as outside a context menu. A press on
        // a title stays inside, so the title toggles or switches the drop
        // itself, and a press on the drop or its flyout is the row's.
        .when(open.is_some(), |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, window, cx| {
                if menu_view::pointer_on_drop(window) {
                    return;
                }
                this.state.update(cx, |state, _| state.close_menu());
                cx.notify();
            }))
        })
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
        // The crumb sits centred between two empty stretches, which are where
        // the window is moved from.
        .child(drag_strip("bar-drag-left", frame, cx))
        .child(breadcrumb(theme, state, cx))
        .child(drag_strip("bar-drag-right", frame, cx))
        .when(frame.draws_controls(), |el| {
            el.child(caption_buttons(theme, window, cx))
        })
}

/// An empty stretch of a bar: where the window is moved from when the app
/// draws its frame.
///
/// Under a client frame a drag moves the window, a double click maximizes or
/// restores it, and a right click opens the window menu. A press arms the move
/// and the first pointer travel starts it, so a click is not taken by the
/// compositor. Under Windows the stretch is the caption, and the system gives
/// the same gestures and snapping.
pub(crate) fn drag_strip(
    id: &'static str,
    frame: Frame,
    cx: &mut Context<Shell>,
) -> gpui::Stateful<gpui::Div> {
    let strip = div().id(id).flex_1().h_full();
    match frame {
        Frame::Platform => strip,
        Frame::Caption => strip.window_control_area(gpui::WindowControlArea::Drag),
        Frame::Client(_) => strip
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|shell, event: &MouseDownEvent, window, _| {
                    shell.window_move = event.click_count < 2;
                    if event.click_count >= 2 {
                        window.zoom_window();
                    }
                }),
            )
            .on_mouse_down(MouseButton::Right, |event, window, _| {
                if window.window_controls().window_menu {
                    window.show_window_menu(event.position);
                }
            }),
    }
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
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(Theme::SPACE_XS))
        .text_size(Theme::TEXT_UI)
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
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    // A compositor that cannot minimize or maximize a window gets no button
    // for it.
    let controls = window.window_controls();
    // Closing the window is leaving the session, so it asks about unsaved
    // work the way Ctrl+Q does; the shell quits once the answer is in.
    let close = cx.listener(|shell: &mut Shell, _: &MouseDownEvent, _, cx| {
        shell.state.update(cx, |state, _| state.request_quit());
        cx.notify();
    });
    div()
        .flex()
        .flex_row()
        .gap(px(Theme::SPACE_XS))
        .when(controls.minimize, |el| {
            el.child(caption_btn(
                "caption-min",
                crate::icons::MINUS,
                theme,
                |_, window, _| window.minimize_window(),
            ))
        })
        .when(controls.maximize, |el| {
            el.child(caption_btn(
                "caption-max",
                crate::icons::SQUARE,
                theme,
                |_, window, _| window.zoom_window(),
            ))
        })
        .child(caption_btn(
            "caption-close",
            crate::icons::CLOSE,
            theme,
            close,
        ))
}

/// One window control. Three glyphs with no words beside them, so each names
/// itself from the tooltip table like every other icon-only control.
fn caption_btn(
    id: &'static str,
    icon: &'static str,
    theme: &Theme,
    on_click: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
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
        .with_tip(crate::tooltip::deferred_builder(id))
        .on_hover(motion::hover_listener(key))
        .child(crate::icons::icon(icon).size(px(8.)))
        .on_mouse_down(MouseButton::Left, on_click)
}
