//! The Viewer's tab row: Program first, then one tab per open source with its
//! close button. Drawn only while a Source tab is open: with Program alone
//! there is nothing to switch to, and the row would stack a second tab row
//! with a single tab under the Viewer's dock tab.

use super::*;
use crate::theme::Theme;

/// One tab: an underline and full-strength text when it has the keys. A tab
/// with no room for its label ends it in an ellipsis. The underline is drawn
/// over the tab's bottom edge, so an active label stands on the same centre
/// line as the others.
fn tab(
    key: motion::MotionKey,
    label: gpui::SharedString,
    active: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let lit = motion::hover_t(key);
    div()
        .id(key.element_id("viewer-tab"))
        .relative()
        .min_w_0()
        .h_full()
        .flex()
        .items_center()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
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
        .when(active, |el| {
            el.child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(Theme::TAB_UNDERLINE))
                    .bg(theme.accent),
            )
        })
}

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
