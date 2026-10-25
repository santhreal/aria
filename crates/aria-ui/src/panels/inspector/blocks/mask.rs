//! Mask and chroma-key blocks of the Look tab.
//!
//! Shape buttons come from `aria_models::mask_catalog()` and add a mask to the
//! bottom of the clip's stack. Each mask draws a header with its mode chips and
//! its invert, reorder and remove buttons; the active mask, the one the Viewer
//! handles and the Pen edit, also draws its knob rows from the property catalog
//! group `mask-<index>`. The key swatches come from
//! `surfaces::inspector_pick::KEY_COLORS`. Every button files its pick through
//! `surfaces::inspector_pick`, so this module names no property and sends no
//! command: it cannot drift from the model, and a slider here cannot flatten
//! the block's keyframes.

use crate::components::Wraps as _;
use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    Styled, div, px,
};

use aria_models::{MaskMode, mask_param_group};

use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::surfaces::inspector_pick::{self, KEY_COLORS, Pick};
use crate::theme::Theme;
use crate::types::{UiClip, UiMask};
use gpui::prelude::FluentBuilder;

use super::widgets::{Chip, Columns, chip_row, param_rows, pick_button, picked};

use crate::components::section_label;

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
            Some(
                pick_button(
                    ("mask-shape", choice.id.as_str()),
                    choice.label.clone(),
                    theme,
                    cx,
                    move |state| inspector_pick::apply(state, Pick::Mask(kind)),
                )
                .into_any_element(),
            )
        })
        .collect();

    let active = state.read(cx).active_mask();
    let last = clip.masks.len().saturating_sub(1) as u32;
    let mut stack = Vec::with_capacity(clip.masks.len());
    for (index, mask) in clip.masks.iter().enumerate() {
        let index = index as u32;
        let is_active = active == Some(index);
        stack.push(mask_header(theme, index, mask, is_active, last, cx).into_any_element());
        if is_active {
            stack.extend(param_rows(theme, &mask_param_group(index), state, cx));
        }
    }

    let chroma = clip.chroma.map(|chroma| chroma.rgb);
    let chroma_rows = param_rows(theme, "chroma", state, cx);
    let key_color = chroma.map_or(SharedString::new_static("Off"), |rgb| {
        KEY_COLORS
            .iter()
            .find(|(_, _, preset)| *preset == rgb)
            .map_or_else(
                || SharedString::from(format!("#{:02x}{:02x}{:02x}", rgb[0], rgb[1], rgb[2])),
                |(_, label, _)| SharedString::new_static(label),
            )
    });
    let mut keys: Vec<_> = KEY_COLORS
        .iter()
        .map(|(id, label, rgb)| {
            let rgb = *rgb;
            Chip::choice(*label, chroma == Some(rgb), |label| {
                pick_button(("chroma-key-color", *id), label, theme, cx, move |state| {
                    inspector_pick::apply(state, Pick::Chroma(rgb))
                })
                .into_any_element()
            })
        })
        .collect();
    if chroma.is_some() {
        keys.push(Chip::other(pick_button(
            "chroma-clear",
            "Remove key",
            theme,
            cx,
            |state| inspector_pick::apply(state, Pick::ClearChroma),
        )));
    }

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Masks"))
        .when(clip.masks.is_empty(), |this| {
            this.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child("None"),
            )
        })
        .children(stack)
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("Add a mask"),
        )
        .child(
            div()
                .flex()
                .wraps()
                .gap(px(Theme::SPACE_XS))
                .children(shapes),
        )
        .child(section_label(theme, "Chroma key"))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "chroma-key"),
            "Key color",
            Columns::Keyed,
            key_color,
            keys,
        ))
        .when(chroma.is_some(), |this| this.children(chroma_rows))
}

/// One mask's header: its name, its mode chips, and its invert, reorder and
/// remove buttons. Pressing the name makes the mask the active one.
fn mask_header(
    theme: &Theme,
    index: u32,
    mask: &UiMask,
    active: bool,
    last: u32,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let key = MotionKey::new("mask", index);
    let shape = crate::catalog_text::masks()
        .iter()
        .find(|choice| choice.id == mask.kind.id())
        .map_or("Mask", |choice| choice.label.as_str());
    let name = picked(
        format!(
            "Mask {} {shape}{}",
            index + 1,
            if mask.invert { " (inverted)" } else { "" }
        ),
        active,
    );
    let modes: Vec<_> = MaskMode::ALL
        .iter()
        .map(|mode| {
            let mode = *mode;
            components::chip(
                ("mask-mode", key.with("mode").with(mode.id())),
                mode.label(),
                mask.mode == mode,
                ChipSize::Compact,
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| {
                        let pick = Pick::MaskMode { mask: index, mode };
                        if let Err(err) = inspector_pick::apply(state, pick) {
                            state.report_error(&err);
                        }
                    });
                    cx.notify();
                }),
            )
            .into_any_element()
        })
        .collect();
    let mut actions = vec![
        pick_button(
            ("mask-invert", key.with("invert")),
            "Invert",
            theme,
            cx,
            move |state| inspector_pick::apply(state, Pick::ToggleMaskInvert(index)),
        )
        .into_any_element(),
    ];
    if index > 0 {
        actions.push(
            pick_button(
                ("mask-up", key.with("up")),
                "Move up",
                theme,
                cx,
                move |state| {
                    let to = index - 1;
                    inspector_pick::apply(state, Pick::MoveMask { from: index, to })
                },
            )
            .into_any_element(),
        );
    }
    if index < last {
        actions.push(
            pick_button(
                ("mask-down", key.with("down")),
                "Move down",
                theme,
                cx,
                move |state| {
                    let to = index + 1;
                    inspector_pick::apply(state, Pick::MoveMask { from: index, to })
                },
            )
            .into_any_element(),
        );
    }
    actions.push(
        pick_button(
            ("mask-remove", key.with("remove")),
            "Remove",
            theme,
            cx,
            move |state| inspector_pick::apply(state, Pick::RemoveMask(index)),
        )
        .into_any_element(),
    );

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .id(key.element_id("mask-name"))
                .text_size(Theme::TEXT_UI)
                .text_color(if active { theme.text } else { theme.text_muted })
                .cursor(gpui::CursorStyle::PointingHand)
                .child(name)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.set_active_mask(index));
                        cx.notify();
                    }),
                ),
        )
        .child(
            div()
                .flex()
                .wraps()
                .gap(px(Theme::SPACE_XS))
                .children(modes),
        )
        .child(
            div()
                .flex()
                .wraps()
                .gap(px(Theme::SPACE_XS))
                .children(actions),
        )
}
