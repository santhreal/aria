//! Look tab: the filter catalog and its amount.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use crate::panels::inspector::blocks::widgets::{kv_row, param_rows, section_label};

pub fn look_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let catalog: Vec<_> = crate::catalog_text::filters()
        .iter()
        .map(|choice| {
            let id = choice.id.to_string();
            components::ghost_button(
                crate::motion::MotionKey::new("look-choice", &choice.id),
                choice.label.clone(),
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    let id = id.clone();
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::Filter(id)) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element()
        })
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Look"))
        .when_some(clip.filter.clone(), |el, filter| {
            el.child(kv_row(theme, "Id", filter.id))
                .children(param_rows(theme, "filter", state, cx))
                .child(
                    components::ghost_button("look-clear", "Clear", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::ClearFilter) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
        })
        .when(clip.filter.is_none(), |el| {
            el.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.text_faint)
                    .child("No look"),
            )
        })
        .child(section_label(theme, "Catalog"))
        .children(catalog)
        .child(crate::panels::inspector::blocks::grade::grade_block(
            theme, clip, state, cx,
        ))
        .child(crate::panels::inspector::blocks::mask::mask_block(
            theme, clip, state, cx,
        ))
        .child(crate::panels::inspector::blocks::matte::matte_block(
            theme, clip, state, cx,
        ))
        .child(crate::panels::inspector::blocks::styles::styles_block(
            theme, clip, state, cx,
        ))
        .child(crate::panels::inspector::blocks::animation::animation_block(theme, clip, state, cx))
        .when(clip.can_transition, |el| {
            el.child(
                crate::panels::inspector::blocks::transition::transition_block(theme, clip, cx),
            )
        })
}
