//! The Viewer's tab row: Program first, one tab per open source with its close
//! button, and the view toggles at the right end: Split, Rulers, the safe
//! margins, the guides, and the transparency grid.

use super::*;
use crate::components::{ChipSize, chip};
use crate::theme::Theme;

/// One tab: an underline and full-strength text when it has the keys.
fn tab(
    key: motion::MotionKey,
    label: gpui::SharedString,
    active: bool,
    theme: &Theme,
) -> gpui::Stateful<gpui::Div> {
    let lit = motion::hover_t(key);
    div()
        .id(key.element_id("viewer-tab"))
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
        .child(label)
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
    let split = read.sources.split();
    let rulers = read.viewer_guides.rulers;
    let safe = read.safe_margins_on();
    let guides = !read.viewer_guides.hidden;
    let alpha = read.transparency_grid_on();
    let program_key = motion::MotionKey::new("viewer-tab", "program");
    let program = tab(program_key, "Program".into(), focus.is_none(), theme)
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
        .child(toggle_chip(
            "viewer-split",
            "Split",
            split,
            theme,
            cx,
            |state, _| state.toggle_viewer_split(),
        ))
        .child(toggle_chip(
            "viewer-rulers",
            "Rulers",
            rulers,
            theme,
            cx,
            |state, _| state.toggle_rulers(),
        ))
        .child(toggle_chip(
            "viewer-safe",
            "Safe",
            safe,
            theme,
            cx,
            |state, _| state.toggle_safe_margins(),
        ))
        .child(toggle_chip(
            "viewer-guides",
            "Guides",
            guides,
            theme,
            cx,
            |state, _| state.toggle_guides_shown(),
        ))
        .child(toggle_chip(
            "viewer-alpha",
            "Alpha",
            alpha,
            theme,
            cx,
            |state, cx| state.toggle_transparency_grid(cx),
        ))
}

/// One view toggle at the right end of the row, lit while it is on.
fn toggle_chip(
    id: &'static str,
    label: &'static str,
    on: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
    toggle: fn(&mut AppState, &mut gpui::Context<AppState>),
) -> impl IntoElement {
    chip(id, label, on, ChipSize::Compact, theme)
        .debug_selector(move || id.to_string())
        .when_some(crate::tooltip::builder(id), |el, tip| el.with_tip(tip))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, toggle);
                cx.notify();
            }),
        )
}
