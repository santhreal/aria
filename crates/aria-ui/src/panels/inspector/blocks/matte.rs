//! Track-matte block of the Look tab.
//!
//! Source buttons come from `MatteSource::ALL`, so a new read channel in the
//! model reaches the inspector without a second list here. The stencil is not
//! picked in this block: the renderer reads whatever layer sits directly above,
//! so the block states whether anything is up there rather than offering a lane
//! chooser that cannot bind.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use aria_models::MatteSource;

use crate::components;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use super::widgets::param_rows;

use crate::components::section_label;

pub fn matte_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let matte = clip.matte;
    let stencil_ready = state.read(cx).matte_stencil_ready();

    let sources: Vec<_> = MatteSource::ALL
        .iter()
        .map(|source| {
            let source = *source;
            let selected = matte.map(|m| m.source) == Some(source);
            components::ghost_button(
                ("matte-source", source.id()),
                if selected {
                    format!("• {}", source.label())
                } else {
                    source.label().to_string()
                },
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::Matte(source)) {
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
        .child(section_label(theme, "Track matte"))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(match matte {
                    Some(matte) => format!(
                        "{}{}",
                        matte.source.label(),
                        if matte.invert { " (inverted)" } else { "" }
                    ),
                    None => "Off".to_string(),
                }),
        )
        .children(sources)
        .when(matte.is_some() && !stencil_ready, |this| {
            this.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.warning)
                    .child("Nothing above this clip to cut with"),
            )
        })
        .when_some(matte, |this, _| {
            this.children(param_rows(theme, "matte", state, cx))
                .child(
                    components::ghost_button("matte-invert", "Invert", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) =
                                    inspector_pick::apply(state, Pick::ToggleMatteInvert)
                                {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button("matte-clear", "Remove matte", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::ClearMatte) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
        })
}
