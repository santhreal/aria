//! The lists the Viewer's control row opens above its chips: the chip's open
//! and close presses, the list drawn over the frame, its headings and rows.
//! The zoom and resolution pickers fill one in each.

use crate::tooltip::WithTip as _;
use gpui::{
    AnyElement, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled, deferred, div, px,
};

use crate::app::AppState;
use crate::app::preview_view::ViewerMenu;
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// Height of a chip, which its list opens above: the control height every
/// control in the Viewer's row shares.
const CHIP_H: f32 = Theme::ICON_HIT;

/// Whether `menu` is the list open above the Viewer.
pub(super) fn is_open(state: &Entity<AppState>, menu: ViewerMenu, cx: &gpui::App) -> bool {
    state.read(cx).viewer_menu == Some(menu)
}

/// The chip named `name`, and `rows` in a list above it while `menu` is open.
/// A press on the chip opens or closes the list; a press anywhere else closes
/// it, and so does a press on a row.
pub(super) fn picker(
    name: &'static str,
    menu: ViewerMenu,
    chip: Stateful<gpui::Div>,
    rows: Option<Vec<AnyElement>>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let open = rows.is_some();
    // The outside press is the wrapper's, not the list's: the chip is outside
    // the list, and a press on it toggles the list rather than closing and
    // reopening it.
    div()
        .relative()
        .when(open, |el| {
            el.on_mouse_down_out(cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.viewer_menu = None);
                cx.notify();
            }))
        })
        .child(
            chip.h(px(CHIP_H))
                .debug_selector(move || name.into())
                .when_some(crate::tooltip::builder(name), |el, tip| el.with_tip(tip))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state
                            .update(cx, |state, _| state.toggle_viewer_menu(menu));
                        cx.notify();
                    }),
                ),
        )
        .when_some(rows, |el, rows| el.child(list(name, rows, theme)))
}

/// The open list, drawn over the frame above the chip.
fn list(name: &'static str, rows: Vec<AnyElement>, theme: &Theme) -> impl IntoElement + use<> {
    let key = crate::motion::MotionKey::new("menu", name);
    let panel = div()
        .id(key.element_id("list"))
        .min_w(px(120.))
        .rounded(px(Theme::PANEL_RADIUS))
        .bg(theme.surface_overlay)
        .border_1()
        .border_color(theme.border)
        .p(px(Theme::SPACE_XS))
        .flex()
        .flex_col()
        // A press on the list is the list's, not the Viewer's drawn under it.
        .occlude()
        .children(rows);
    // Deferred so the list paints over the frame above the control row.
    deferred(
        div()
            .absolute()
            .bottom(px(CHIP_H + Theme::SPACE_XS))
            .left(px(0.))
            .child(crate::frost::frost(crate::motion::menu_in(key, panel))),
    )
}

/// A section label inside a list.
pub(super) fn heading(text: &'static str, theme: &Theme) -> AnyElement {
    div()
        .px(px(Theme::SPACE_SM))
        .pt(px(Theme::SPACE_XS))
        .text_size(Theme::TEXT_UI)
        .text_color(theme.text_faint)
        .child(text)
        .into_any_element()
}

/// One choice. The selected one holds the selected wash. A press closes the
/// list and runs `choose`.
pub(super) fn row(
    id: (&'static str, usize),
    label: impl Into<SharedString>,
    selected: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    choose: impl Fn(&mut AppState, &mut gpui::Context<AppState>) + 'static,
) -> AnyElement {
    let id = gpui::ElementId::from(id);
    let key = crate::motion::MotionKey::new("viewer-menu-row", &id);
    let (rest, hot) = if selected {
        (theme.glass_selected_bg(), theme.element_active)
    } else {
        (theme.wash(0.0), theme.element_hover)
    };
    div()
        .id(id)
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .text_size(Theme::TEXT_TITLE)
        .text_color(if selected {
            theme.text
        } else {
            theme.text_muted
        })
        .bg(crate::motion::hover_blend(key, rest, hot))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(crate::motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                cx.stop_propagation();
                this.state.update(cx, |state, cx| {
                    state.viewer_menu = None;
                    choose(state, cx);
                });
                cx.notify();
            }),
        )
        .child(label.into())
        .into_any_element()
}
