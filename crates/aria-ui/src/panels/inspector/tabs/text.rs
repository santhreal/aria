//! Text tab: a title's content, its type properties, and its text-effect preset.
//!
//! The numeric properties come from the catalog rather than from a list here, so
//! the size, the spacing, the stroke and the shadow are all animatable through
//! the same keyframe-aware path every other block writes through. They had no
//! surface at all before that: the model carried twelve text properties and the
//! tab drew none of them.

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, px,
};

use crate::components;
use crate::fluent::FluentBuilder;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::{RenameTarget, UiClip};

use crate::panels::inspector::blocks::widgets::{param_rows, section_label};

pub fn text_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let content = clip
        .generator
        .as_ref()
        .map(|generator| generator.content.clone())
        .filter(|content| !content.is_empty());
    let active = clip.text_effect.clone();

    // The chips come from the model catalog, so a new preset needs no edit here.
    // The active one is marked with the same `• ` prefix the transition and style
    // pickers use, rather than a colour only.
    let presets: Vec<_> = crate::catalog_text::text_effects()
        .iter()
        .map(|choice| {
            let id = choice.id.as_str();
            let on = active.as_deref() == Some(id);
            let label = if on {
                gpui::SharedString::from(format!("• {}", choice.label))
            } else {
                choice.label.clone()
            };
            components::ghost_button(id, label, theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state.update(cx, |state, _| {
                            let result = if on {
                                clip_pick::apply(state, Pick::ClearTextEffect)
                            } else {
                                clip_pick::apply(state, Pick::TextEffect(id.to_string()))
                            };
                            if let Err(err) = result {
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
        .child(section_label(theme, "Text"))
        .when_some(content.clone(), |el, content| {
            el.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.text)
                    .child(SharedString::from(content)),
            )
        })
        .when(content.is_none(), |el| {
            el.child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.text_faint)
                    .child("No text"),
            )
        })
        .child(
            components::ghost_button("text-edit", "Edit text…", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.begin_rename(RenameTarget::ClipText));
                    cx.notify();
                }),
            ),
        )
        .child(section_label(theme, "Type"))
        .children(param_rows(theme, "text", state, cx))
        .child(section_label(theme, "Font"))
        .child(font_picker(theme, state, cx))
        .child(section_label(theme, "Effect"))
        .children(presets)
        .child(section_label(theme, "Cloud presets"))
        .child(cloud_presets(theme, state, cx))
}

/// The cloud text presets: the pack catalog, then whatever pack is loaded.
///
/// A catalog entry is a *pack* of presets, so loading one is two steps — download the
/// pack, then apply a preset from it. Collapsing them would hide which pack a title's
/// look came from.
fn cloud_presets(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // The pack behind one handle: a row that needs an owned preset to send takes
    // it from the handle when it is clicked, so drawing the list costs a refcount
    // instead of a copy of every preset and every style in the pack.
    let pack = state.read(cx).text_presets.clone();
    let rows: Vec<_> = pack
        .iter()
        .enumerate()
        .map(|(index, preset)| {
            let id =
                crate::motion::MotionKey::new("text-preset", &preset.id).element_id("text-preset");
            let label = SharedString::from(if preset.category.is_empty() {
                preset.name.clone()
            } else {
                format!("{} — {}", preset.name, preset.category)
            });
            let hover = theme.wash(0.06);
            let text = theme.text;
            let pack = pack.clone();
            div()
                .id(id)
                .px(px(Theme::SPACE_SM))
                .py(px(Theme::SPACE_XS))
                .rounded(px(Theme::CONTROL_RADIUS))
                .cursor(gpui::CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .hover(move |el| el.bg(hover))
                .text_size(px(13.))
                .text_color(text)
                .child(label)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        let Some(preset) = pack.get(index).cloned() else {
                            return;
                        };
                        this.state.update(cx, |state, _| {
                            if let Err(err) = clip_pick::apply(state, Pick::TextPreset(preset)) {
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
        .child(crate::panels::cloud::cloud_catalog_block(
            theme,
            crate::cloud::CloudFeed::TextPresets,
            state,
            cx,
        ))
        .children(rows)
}

/// The typeface picker: a window of installed families, the one in use marked,
/// and a step either way for walking past the window.
///
/// A window rather than the whole list because a host has hundreds of families;
/// `AppState::font_window` centres it on the selection, so stepping scrolls it.
/// Clicking a name sets it, clicking the marked name is inert, and the first row
/// is the system default, which is what an empty family means in the file.
///
/// The rows are the reason this is not a chip strip: each carries its own hover
/// fade, keyed by family, so the pointer highlights one name rather than the
/// block. The picker itself fades in with the tab.
fn font_picker(
    theme: &Theme,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let current = state.read(cx).selected_font().unwrap_or_default();
    let installed = state.read(cx).font_families().len();
    let window = state.read(cx).font_window(9);
    let mut rows: Vec<gpui::AnyElement> = Vec::with_capacity(window.len() + 1);
    rows.push(font_row(
        theme,
        "",
        "System default",
        current.is_empty(),
        cx,
    ));
    for (family, on) in window {
        let label = family.clone();
        rows.push(font_row(theme, &family, &label, on, cx));
    }

    crate::motion::fade_in(
        "font-picker",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XXS))
            .children(rows)
            .child(
                div()
                    .flex()
                    .gap(px(Theme::SPACE_XS))
                    .child(step_button(theme, false, cx))
                    .child(step_button(theme, true, cx)),
            )
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.text_faint)
                    .child(SharedString::from(format!("{installed} fonts installed"))),
            ),
    )
}

/// One family row. `family` is what the file stores, so the default row passes
/// `""` and sets the style back to unset.
fn font_row(
    theme: &Theme,
    family: &str,
    label: &str,
    on: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // Keyed by family, so two rows never share a fade and the key survives the
    // window scrolling under the pointer.
    let key = crate::motion::MotionKey::new("font", family);
    let background = crate::motion::hover_blend(key, theme.surface, theme.surface_raised_hover);
    let owned = family.to_string();
    div()
        .id(key.element_id("font"))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(background)
        .text_size(px(12.))
        .text_color(if on { theme.accent } else { theme.text })
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .on_hover(crate::motion::hover_listener(key))
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let family = owned.clone();
                this.state.update(cx, |state, _| {
                    if let Err(err) = clip_pick::apply(state, Pick::Font(family)) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .child(SharedString::from(if on {
            format!("• {label}")
        } else {
            label.to_string()
        }))
        .into_any_element()
}

fn step_button(
    theme: &Theme,
    forward: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let (id, label) = if forward {
        ("font-next", "Next font")
    } else {
        ("font-prev", "Previous font")
    };
    components::ghost_button(id, label, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = clip_pick::apply(state, Pick::CycleFont { forward }) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}
