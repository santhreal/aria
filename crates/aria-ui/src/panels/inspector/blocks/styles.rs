//! Layer-style and LUT blocks of the Look tab.
//!
//! One block per style, from `app::clip_styles::STYLES`; the numeric rows inside
//! it are generated from the property catalog, whose group name *is* the style id
//! (`shadow`, `glow`, `outline`, `background`). So the panel names no property,
//! and a knob edit writes that one property rather than rebuilding the block.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::app::clip_styles::{STYLES, style_enabled};
use crate::cloud::CloudFeed;
use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::inspector_pick::{self, Pick, STYLE_COLORS};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::param_rows;

use crate::components::section_label;

pub fn styles_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let styles = clip.styles;

    let blocks: Vec<_> = STYLES
        .iter()
        .map(|(id, label)| {
            let style = *id;
            let on = style_enabled(&styles, style);
            let knobs = param_rows(theme, style, state, cx);

            let swatches: Vec<_> = STYLE_COLORS
                .iter()
                .map(|(suffix, color_label, rgba)| {
                    let rgba = *rgba;
                    let id: &'static str = match (style, *suffix) {
                        ("shadow", "black") => "shadow-black",
                        ("shadow", "white") => "shadow-white",
                        ("shadow", _) => "shadow-half",
                        ("glow", "black") => "glow-black",
                        ("glow", "white") => "glow-white",
                        ("glow", _) => "glow-half",
                        ("outline", "black") => "outline-black",
                        ("outline", "white") => "outline-white",
                        ("outline", _) => "outline-half",
                        (_, "black") => "background-black",
                        (_, "white") => "background-white",
                        (_, _) => "background-half",
                    };
                    components::ghost_button(("style-color", id), *color_label, theme)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                this.state.update(cx, |state, _| {
                                    if let Err(err) = inspector_pick::apply(
                                        state,
                                        Pick::StyleColor { style, rgba },
                                    ) {
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
                .child(
                    components::ghost_button(
                        ("style-toggle", style),
                        format!("{label}{}", if on { " on" } else { "" }),
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::Style(style)) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .when(on, |this| this.children(knobs).children(swatches))
                .into_any_element()
        })
        .collect();

    let lut = clip.lut.clone();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Layer styles"))
        .children(blocks)
        .child(
            components::ghost_button("styles-clear", "Clear styles", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        if let Err(err) = inspector_pick::apply(state, Pick::ClearStyles) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            ),
        )
        .child(section_label(theme, "LUT"))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(match &lut {
                    Some(lut) => lut.name().to_string(),
                    None => "None".to_string(),
                }),
        )
        .child(
            components::ghost_button("lut-load", "Load .cube…", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.pick_and_load_lut());
                    cx.notify();
                }),
            ),
        )
        .when(lut.is_some(), |this| {
            this.children(param_rows(theme, "lut", state, cx)).child(
                components::ghost_button("lut-clear", "Remove LUT", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = inspector_pick::apply(state, Pick::ClearLut) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
        .child(section_label(theme, "Cloud LUTs"))
        .child(crate::panels::cloud::cloud_catalog_block(
            theme,
            CloudFeed::Luts,
            state,
            cx,
        ))
}
