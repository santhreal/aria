//! The Home sidebar navigation rail: brand mark, navigation tabs, and rail selection.
//!
//! Home is the first surface of a session, so the rail defines the primary
//! entry points: New, Open, Recents, Samples, and Learn.

use gpui::{
    CursorStyle, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::motion;
use crate::theme::Theme;
use crate::types::HomeRail;

/// Render the Home sidebar navigation rail.
pub(super) fn home_rail(
    theme: &Theme,
    selected: HomeRail,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let tabs = [
        (HomeRail::New, crate::icons::PLUS),
        (HomeRail::Open, crate::icons::FOLDER_OPEN),
        (HomeRail::Recents, crate::icons::CLAPPERBOARD),
        (HomeRail::Samples, crate::icons::IMAGE),
        (HomeRail::Learn, crate::icons::SPARKLES),
    ];
    // The brand is the shipped mark, the same art the packagers turn into the
    // app icon: Home is the first surface of a session, and a placeholder there
    // is the one place the product looks unfinished for free. 24px to sit on the
    // cap height of the word beside it.
    let mark = crate::icons::brand_mark(24.);
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
                .flex_col()
                .gap(px(Theme::SPACE_XS))
                .mb(px(Theme::SPACE_MD))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(Theme::SPACE_SM))
                        .child(mark)
                        .child(
                            div()
                                .text_size(Theme::TEXT_LARGE)
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("Aria"),
                        ),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_muted)
                        .child("Edit video. Keep the timeline honest."),
                ),
        )
        .children(tabs.into_iter().map(|(tab, icon)| {
            let on = tab == selected;
            let key = tab.label();
            let fade = motion::MotionKey::new("home-tab", key);
            let rest = if on {
                theme.glass_selected_bg()
            } else {
                theme.wash(0.0)
            };
            div()
                .id(key)
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
                .child(div().text_size(Theme::TEXT_UI).child(tab.label()))
                .on_mouse_down(
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
