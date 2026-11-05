//! The Home sidebar rail: wordmark, the New and Open actions, then the panes.
//!
//! New and Open run at the press, as the Start tiles and the shortcuts do, so
//! starting or opening a project from Home takes one press. Recents, Samples
//! and Learn switch the pane beside the rail.

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::motion;
use crate::surfaces::home_pick::Pick;
use crate::theme::Theme;
use crate::types::HomeRail;

use super::cards::press;

/// Render the Home sidebar rail.
pub(super) fn home_rail(
    theme: &Theme,
    selected: HomeRail,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let actions = [
        ("New", crate::icons::PLUS, Pick::NewProject),
        ("Open", crate::icons::FOLDER_OPEN, Pick::PickProject),
    ];
    // The brand is the shipped wordmark, the full logo: Home is the first
    // surface of a session, and a placeholder there is the one place the
    // product looks unfinished for free. The titlebar keeps the compact mark.
    let wordmark = crate::icons::wordmark(20.);
    div()
        .id("home-rail")
        .w(px(crate::settings::SIDEBAR_MIN))
        .h_full()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_LG))
        .bg(theme.glass())
        .border_r_1()
        .border_color(theme.border)
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .mb(px(Theme::SPACE_MD))
                .text_color(theme.text)
                .child(wordmark),
        )
        .children(actions.into_iter().map(|(label, icon, pick)| {
            rail_row(label, icon, false, theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| press(state, pick.clone()));
                    cx.notify();
                }),
            )
        }))
        .child(div().h(px(Theme::SPACE_MD)))
        .children(HomeRail::ALL.into_iter().map(|tab| {
            let icon = match tab {
                HomeRail::Recents => crate::icons::HISTORY,
                HomeRail::Samples => crate::icons::IMAGE,
                HomeRail::Learn => crate::icons::SPARKLES,
            };
            rail_row(tab.label(), icon, tab == selected, theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, cx| {
                        state.set_home_rail(tab);
                        state.persist_settings_later(cx);
                    });
                    cx.notify();
                }),
            )
        }))
}

/// One rail row: its icon and label, lit while `on`.
fn rail_row(
    label: &'static str,
    icon: &'static str,
    on: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let fade = motion::MotionKey::new("home-tab", label);
    let rest = if on {
        theme.glass_selected_bg()
    } else {
        theme.wash(0.0)
    };
    div()
        .id(label)
        .debug_selector(move || format!("home-rail-{label}"))
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .h(px(Theme::SPACE_LG + Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .text_color(if on { theme.text } else { theme.text_muted })
        .bg(motion::hover_blend(fade, rest, theme.element_hover))
        .on_hover(motion::hover_listener(fade))
        .child(crate::icons::icon(icon))
        .child(div().text_size(Theme::TEXT_UI).child(label))
}
