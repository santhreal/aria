//! The Viewer's tab row: Program first, then one tab per open source with its
//! close button. Drawn only while a Source tab is open: with Program alone
//! there is nothing to switch to, and the row would stack a second tab row
//! with a single tab under the Viewer's dock tab.

use super::*;
use crate::theme::Theme;

/// The tab row over the Viewer's picture, or `None` while no Source tab is
/// open.
pub(super) fn tab_row(
    state: &Entity<AppState>,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<impl IntoElement + use<>> {
    let read = state.read(cx);
    if read.sources.tabs().is_empty() {
        return None;
    }
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
    let program =
        crate::components::underline_tab("viewer-tab-program", "Program", focus.is_none(), theme)
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
        crate::components::underline_tab(
            ("viewer-tab-source", index),
            label,
            focus == Some(index),
            theme,
        )
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
            crate::components::icon_button(("viewer-tab-close", index), crate::icons::CLOSE, theme)
                .size(px(Theme::ICON_HIT_ROW))
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
    Some(
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
            .children(sources),
    )
}
