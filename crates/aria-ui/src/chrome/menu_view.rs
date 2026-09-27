//! The menu drop: a title in the bar, and the panel it opens.
//!
//! Moved out of `chrome.rs` so the bar's frame and the drop are separate files.
//! What a drop contains is decided by [`super::menu_host`] — this file only draws
//! it, animates it in and out, and fades the titles and rows under the pointer.

use std::cell::RefCell;

use gpui::motion::{MotionPolicy, presets};
use gpui::{
    AnyElement, Context, CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use super::MENU_H;
use super::menu_host;
use super::menu_items::MenuItem;
use crate::app::AppState;
use crate::fluent::FluentBuilder;
use crate::motion;
use crate::popover::Popup;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::types::AppMenu;

/// What the bar draws for one menu this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Phase {
    /// Open: drawn, and it takes clicks.
    Open,
    /// On its way out, `0..1` through the exit: drawn, and inert.
    Closing(f32),
}

thread_local! {
    /// The drop as the bar draws it, which is not quite what the state holds.
    ///
    /// `AppState::open_menu` goes to `None` the instant a row is chosen, which
    /// leaves an exit animation nothing to animate. This keeps the menu that was
    /// open alive for exactly one [`presets::MENU_OUT`], so the drop fades out
    /// instead of vanishing. Thread-local because it is frame state, like the
    /// hover fades: no part of the model wants to know.
    static DROP: RefCell<Popup<AppMenu>> = RefCell::new(Popup::default());
}

/// Reconcile the drawn drop with the state, and say which menu owns a drop this
/// frame. Call once per frame, before the titles are built.
///
/// Reduced motion under `policy` drops the exit rather than shortening it: under
/// reduced motion the animation schedules no frames, so a drop left mid-exit
/// would stay on screen until something else asked for a repaint.
pub(super) fn drop_phase(open: Option<AppMenu>, policy: MotionPolicy) -> Option<(AppMenu, Phase)> {
    let span = policy.scale(presets::MENU_OUT.total());
    DROP.with_borrow_mut(|popup| {
        match open {
            Some(menu) if popup.as_open() != Some(&menu) => popup.open(menu),
            Some(_) => {}
            None => {
                popup.begin_close();
                popup.finish_close(span);
            }
        }
        if let Some(menu) = popup.as_open() {
            return Some((*menu, Phase::Open));
        }
        if policy.reduced() {
            return None;
        }
        let since = popup.closing_since()?;
        let menu = *popup.get()?;
        let elapsed = since.elapsed();
        let t = if span.is_zero() {
            1.0
        } else {
            (elapsed.as_secs_f32() / span.as_secs_f32()).clamp(0.0, 1.0)
        };
        Some((menu, Phase::Closing(t)))
    })
}

pub(super) fn menu_drop(
    menu: AppMenu,
    phase: Option<Phase>,
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let label = menu.label();
    let open = phase == Some(Phase::Open);
    let key = motion::MotionKey::new("menu-title", label);
    let rows: Vec<AnyElement> = match phase {
        Some(_) => {
            let rows = {
                let read = state.read(cx);
                // The bar draws the registry, not the shipped tables: a plugin's item
                // is drawn by the same code that draws File > New.
                let items = menu_host::host_of(&read.ctx)
                    .map(|host| host.items(menu, read))
                    .unwrap_or_default();
                items
                    .iter()
                    .copied()
                    .map(|item| (item, super::menu_items::row_caption(&item, read)))
                    .collect::<Vec<_>>()
            };
            rows.into_iter()
                .map(|(item, caption)| menu_row(item, caption, open, theme, cx).into_any_element())
                .collect()
        }
        None => Vec::new(),
    };
    div()
        .id(label)
        .relative()
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(13.))
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        // An open title holds the selected wash; a shut one fades into it.
        .bg(if open {
            theme.glass_selected_bg()
        } else {
            motion::hover_blend(key, theme.wash(0.0), theme.element_hover)
        })
        .on_hover(motion::hover_listener(key))
        .child(label)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.toggle_menu(menu));
                cx.notify();
            }),
        )
        .when_some(phase, |el, phase| {
            el.child(drop_panel(label, phase, theme, rows))
        })
}

/// The panel under a title: `menu_in` on the way in, `menu_out` on the way out.
fn drop_panel(
    label: &'static str,
    phase: Phase,
    theme: &Theme,
    rows: Vec<AnyElement>,
) -> impl IntoElement {
    let panel = div()
        .id(label)
        .min_w(px(200.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        .on_mouse_down(MouseButton::Left, |_: &MouseDownEvent, _, _| {})
        .children(rows);
    let animated = match phase {
        Phase::Open => motion::menu_in(motion::MotionKey::new("menu", label), panel),
        Phase::Closing(t) => motion::menu_out(motion::MotionKey::new("menu", label), t, panel),
    };
    div()
        .absolute()
        .top(px(MENU_H))
        .left(px(0.))
        .child(crate::frost::frost(animated))
}

/// One row. `live` is false while the drop fades out: a click landing inside the
/// exit would run the row a second time.
fn menu_row(
    item: MenuItem,
    caption: String,
    live: bool,
    theme: &Theme,
    cx: &mut Context<Shell>,
) -> impl IntoElement {
    let (id, run) = (item.id, item.run);
    let label = caption;
    let key = motion::MotionKey::new("menu-row", id);
    let click = cx.listener(move |this, _: &MouseDownEvent, _, cx| {
        this.state.update(cx, |state, cx| {
            run(id, state, cx);
            state.close_menu();
        });
        cx.notify();
    });
    div()
        .id(id)
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(px(13.))
        .text_color(theme.text)
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .child(label)
        .when(live, |el| el.on_mouse_down(MouseButton::Left, click))
}
