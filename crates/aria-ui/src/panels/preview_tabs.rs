//! The Viewer's tab row: Program first, one tab per open source with its close
//! button, and at the right end the display button, a wrench holding View >
//! Viewer Display Settings (split, rulers, guides, safe margins, grids) under
//! itself.

use super::*;
use crate::theme::Theme;

/// One tab: an underline and full-strength text when it has the keys. A tab
/// with no room for its label ends it in an ellipsis.
fn tab(
    key: motion::MotionKey,
    label: gpui::SharedString,
    active: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let lit = motion::hover_t(key);
    div()
        .id(key.element_id("viewer-tab"))
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .border_b_2()
        .border_color(if active {
            theme.accent
        } else {
            theme.wash(0.0)
        })
        .bg(motion::mix(
            theme.wash(if active { 0.08 } else { 0.0 }),
            theme.element_hover,
            lit,
        ))
        .text_size(Theme::TEXT_UI)
        .text_color(motion::mix(
            if active { theme.text } else { theme.text_muted },
            theme.text,
            lit,
        ))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(motion::hover_listener(key))
        .child(div().min_w_0().truncate().child(label))
}

/// The tab row over the Viewer's picture.
pub(super) fn tab_row(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let read = state.read(cx);
    let focus = read.sources.focus_index();
    // A tab linked to a timeline clip reads "Clip:", so it is not mistaken for
    // the pool's tab on the same take, whose marks trim nothing.
    let labels: Vec<gpui::SharedString> = read
        .sources
        .tabs()
        .iter()
        .map(|view| {
            let kind = if view.clip.is_some() {
                "Clip"
            } else {
                "Source"
            };
            format!("{kind}: {}", view.name).into()
        })
        .collect();
    let program_key = motion::MotionKey::new("viewer-tab", "program");
    let program = tab(program_key, "Program".into(), focus.is_none(), theme)
        .flex_none()
        .debug_selector(|| "viewer-tab-program".into())
        .when_some(crate::tooltip::builder("viewer-tab-program"), |el, tip| {
            el.with_tip(tip)
        })
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| state.show_program());
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        );
    let sources = labels.into_iter().enumerate().map(|(index, label)| {
        let key = motion::MotionKey::new("viewer-tab", index);
        tab(key, label, focus == Some(index), theme)
            .debug_selector(move || format!("viewer-tab-source-{index}"))
            .when_some(crate::tooltip::builder("viewer-tab-source"), |el, tip| {
                el.with_tip(tip)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.focus_source_tab(index));
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    cx.stop_propagation();
                    this.state
                        .update(cx, |state, _| state.close_source_tab(index));
                    cx.notify();
                }),
            )
            .child(
                crate::components::icon_button(
                    ("viewer-tab-close", index),
                    crate::icons::CLOSE,
                    theme,
                )
                .flex_none()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        this.state
                            .update(cx, |state, _| state.close_source_tab(index));
                        cx.notify();
                    }),
                ),
            )
            .into_any_element()
    });
    div()
        .debug_selector(|| "viewer-tab-row".into())
        .h(px(Theme::TAB_ROW_HEIGHT))
        .flex_none()
        .flex()
        .items_center()
        .px(px(Theme::SPACE_XS))
        .gap(px(Theme::SPACE_XXS))
        .border_b_1()
        .border_color(theme.border)
        .bg(theme.surface_header)
        .child(program)
        .children(sources)
        .child(div().flex_1())
        .child(crate::chrome::tool_strip::display_btn(
            "viewer-display-settings",
            crate::chrome::menu_items::VIEWER_DISPLAY,
            theme,
            state,
            cx,
        ))
}
