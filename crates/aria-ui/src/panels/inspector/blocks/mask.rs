//! Mask and chroma-key blocks of the Look tab.
//!
//! Shape buttons come from `aria_models::mask_catalog()` and the key swatches
//! from `surfaces::inspector_pick::KEY_COLORS`. The knob rows are generated from
//! the property catalog by group (`mask`, `chroma`), and every button files its
//! pick through `surfaces::inspector_pick`, so this module names no property and
//! calls no setter: it cannot drift from the model, and a slider here cannot
//! flatten the block's keyframes.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::inspector_pick::{self, KEY_COLORS, Pick};
use crate::theme::Theme;
use crate::types::UiClip;

use super::widgets::{param_rows, section_label};

pub fn mask_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let shapes: Vec<_> = crate::catalog_text::masks()
        .iter()
        .filter_map(|choice| {
            // A row whose id names no shape would be a catalog that disagrees
            // with the shapes, which `catalog_text` asserts is impossible: this
            // draws what the catalog offers rather than deciding what a shape is.
            let kind = aria_models::MaskKind::from_id(choice.id.as_str())?;
            let selected = clip.mask.map(|mask| mask.kind) == Some(kind);
            Some(
                components::ghost_button(
                    choice.id.as_str(),
                    if selected {
                        gpui::SharedString::from(format!("• {}", choice.label))
                    } else {
                        choice.label.clone()
                    },
                    theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = inspector_pick::apply(state, Pick::Mask(kind)) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                )
                .into_any_element(),
            )
        })
        .collect();

    let mask = clip.mask;
    let knobs = param_rows(theme, "mask", state, cx);
    let chroma = clip.chroma;
    let chroma_rows = param_rows(theme, "chroma", state, cx);

    let keys: Vec<_> = KEY_COLORS
        .iter()
        .map(|(id, label, rgb)| {
            let rgb = *rgb;
            components::ghost_button(*id, *label, theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = inspector_pick::apply(state, Pick::Chroma(rgb)) {
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
        .child(section_label(theme, "Mask"))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.text_faint)
                .child(match mask {
                    Some(mask) => format!(
                        "{}{}",
                        crate::catalog_text::masks()
                            .iter()
                            .find(|choice| choice.id == mask.kind.id())
                            .map(|choice| choice.label.as_str())
                            .unwrap_or("Mask"),
                        if mask.invert { " (inverted)" } else { "" }
                    ),
                    None => "None".to_string(),
                }),
        )
        .children(shapes)
        .when(mask.is_some(), |this| {
            this.children(knobs)
                .child(
                    components::ghost_button("mask-invert", "Invert", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) =
                                    inspector_pick::apply(state, Pick::ToggleMaskInvert)
                                {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    components::ghost_button("mask-clear", "Remove mask", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = inspector_pick::apply(state, Pick::ClearMask) {
                                    state.report_error(&err);
                                }
                            });
                            cx.notify();
                        }),
                    ),
                )
        })
        .child(section_label(theme, "Chroma key"))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme.text_faint)
                .child(match chroma {
                    Some(chroma) => format!(
                        "{:02X}{:02X}{:02X}",
                        chroma.rgb[0], chroma.rgb[1], chroma.rgb[2]
                    ),
                    None => "Off".to_string(),
                }),
        )
        .children(keys)
        .when(chroma.is_some(), |this| {
            this.children(chroma_rows).child(
                components::ghost_button("chroma-clear", "Remove key", theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            if let Err(err) = inspector_pick::apply(state, Pick::ClearChroma) {
                                state.report_error(&err);
                            }
                        });
                        cx.notify();
                    }),
                ),
            )
        })
}
