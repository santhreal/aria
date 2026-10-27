//! Text tab: a title's content, its type properties, and its text-effect preset.
//!
//! The numeric and color properties come from the catalog rather than from a
//! list here, so the size, the spacing, the fill, the stroke and the shadow are
//! all animatable through the same keyframe-aware path every other block writes
//! through. The Character and Paragraph controls are drawn here once and shared
//! with the Character and Paragraph dock panels, so a title has one set of
//! writes whichever surface edits it.

use crate::components::Wraps as _;
use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::{TextAlignH, TextAlignV, TextCase};

use crate::app::clip_text::TextSwitch;
use crate::components::{self, ChipSize};
use crate::motion::MotionKey;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::{RenameTarget, UiClip};
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{
    Columns, color_rows, param_rows, pick_button, picked, switch_line,
};

use crate::components::section_label;

/// The surface name of the inspector's Text tab. The dock panels pass their own,
/// so a control drawn in both places keeps its hover state apart.
const INSPECTOR: &str = "insp-text";

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
    // The active one is lit, as every chip in force is; pressing it clears it.
    let presets = chip_wrap(
        crate::catalog_text::text_effects()
            .iter()
            .map(|choice| {
                let id = choice.id.as_str();
                let on = active.as_deref() == Some(id);
                pick_chip(
                    theme,
                    MotionKey::new("text-effect", id),
                    choice.label.clone(),
                    on,
                    move || {
                        if on {
                            Pick::ClearTextEffect
                        } else {
                            Pick::TextEffect(id.to_string())
                        }
                    },
                    cx,
                )
            })
            .collect(),
    );

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Text"))
        .when_some(content.clone(), |el, content| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text)
                    .child(content),
            )
        })
        .when(content.is_none(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child("No text"),
            )
        })
        .child(div().flex().child(pick_button(
            "text-edit",
            "Edit text…",
            theme,
            cx,
            |state| {
                state.begin_rename(RenameTarget::ClipText);
                Ok(())
            },
        )))
        .children(character_controls(theme, INSPECTOR, state, cx))
        .children(paragraph_controls(theme, INSPECTOR, state, cx))
        .child(section_label(theme, "Effect"))
        .child(presets)
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
                format!("{} · {}", preset.name, preset.category)
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
                .text_size(Theme::TEXT_UI)
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

/// The Character controls: the font, the face switches, the case, the
/// treatments around the glyphs, then the catalog's size, spacing and colour
/// rows. Empty when the selection is not a title.
///
/// `surface` names where the controls are drawn and is folded into every key.
pub fn character_controls(
    theme: &Theme,
    surface: &'static str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let Some(face) = state.read(cx).selected_text_face() else {
        return Vec::new();
    };
    let switches = |set: &[TextSwitch], cx: &mut gpui::Context<crate::shell::Shell>| {
        chip_wrap(
            set.iter()
                .map(|&switch| {
                    pick_chip(
                        theme,
                        MotionKey::new("text-switch", switch.label()).with(surface),
                        switch.label(),
                        face.is_on(switch),
                        move || Pick::TextSwitch(switch),
                        cx,
                    )
                })
                .collect(),
        )
    };
    let face_row = switches(&TextSwitch::FACE, cx);
    let treatments = switches(&TextSwitch::TREATMENTS, cx);
    let cases = chip_wrap(
        TextCase::ALL
            .iter()
            .map(|&case| {
                pick_chip(
                    theme,
                    MotionKey::new("text-case", case.label()).with(surface),
                    case.label(),
                    face.case == case,
                    move || Pick::TextCase(case),
                    cx,
                )
            })
            .collect(),
    );
    let mut out = vec![
        section_label(theme, "Font").into_any_element(),
        font_picker(theme, surface, state, cx).into_any_element(),
        section_label(theme, "Style").into_any_element(),
        face_row,
        cases,
        section_label(theme, "Treatments").into_any_element(),
        treatments,
        section_label(theme, "Size and spacing").into_any_element(),
    ];
    out.extend(param_rows(theme, "text", state, cx));
    out.push(section_label(theme, "Color").into_any_element());
    out.extend(color_rows(theme, "text", state, cx));
    out
}

/// The Paragraph controls: where the lines sit across the frame, where the
/// block sits down it, and whether long lines wrap. Empty when the selection is
/// not a title.
pub fn paragraph_controls(
    theme: &Theme,
    surface: &'static str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let Some(face) = state.read(cx).selected_text_face() else {
        return Vec::new();
    };
    let across = chip_wrap(
        TextAlignH::ALL
            .iter()
            .map(|&align| {
                pick_chip(
                    theme,
                    MotionKey::new("text-align-h", align.label()).with(surface),
                    align.label(),
                    face.align_h == align,
                    move || Pick::TextAlignH(align),
                    cx,
                )
            })
            .collect(),
    );
    let down = chip_wrap(
        TextAlignV::ALL
            .iter()
            .map(|&align| {
                pick_chip(
                    theme,
                    MotionKey::new("text-align-v", align.label()).with(surface),
                    align.label(),
                    face.align_v == align,
                    move || Pick::TextAlignV(align),
                    cx,
                )
            })
            .collect(),
    );
    let wrap = switch_line(
        theme,
        MotionKey::new("insp.row", "text-wrap").with(surface),
        TextSwitch::Wrap.label(),
        Columns::Keyed,
        MotionKey::new("text-switch", TextSwitch::Wrap.label()).with(surface),
        Some(face.is_on(TextSwitch::Wrap)),
        cx,
        |state| clip_pick::apply(state, Pick::TextSwitch(TextSwitch::Wrap)),
    );
    vec![
        section_label(theme, "Alignment").into_any_element(),
        across,
        down,
        section_label(theme, "Lines").into_any_element(),
        wrap.into_any_element(),
    ]
}

/// A wrapping row of chips.
fn chip_wrap(chips: Vec<AnyElement>) -> AnyElement {
    div()
        .flex()
        .flex_row()
        .wraps()
        .gap(px(Theme::SPACE_XXS))
        .children(chips)
        .into_any_element()
}

/// One chip that hands `pick` to the registry when pressed. The pick is built
/// on the press, so the chip holds only the copyable choice it stands for.
fn pick_chip(
    theme: &Theme,
    key: MotionKey,
    label: impl Into<SharedString>,
    active: bool,
    pick: impl Fn() -> Pick + 'static,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    components::chip(key, label, active, ChipSize::Compact, theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = clip_pick::apply(state, pick()) {
                        state.report_error(&err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
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
    surface: &'static str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let current = state.read(cx).selected_font().unwrap_or_default();
    let installed = state.read(cx).font_families().len();
    let window = state.read(cx).font_window(9);
    let mut rows: Vec<gpui::AnyElement> = Vec::with_capacity(window.len() + 1);
    rows.push(font_row(
        theme,
        surface,
        "",
        "System default",
        current.is_empty(),
        cx,
    ));
    for (family, on) in window {
        rows.push(font_row(theme, surface, &family, &family, on, cx));
    }

    crate::motion::fade_in(
        MotionKey::new("font-picker", surface),
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XXS))
            .children(rows)
            .child(
                div()
                    .flex()
                    .gap(px(Theme::SPACE_XS))
                    .child(step_button(theme, surface, false, cx))
                    .child(step_button(theme, surface, true, cx)),
            )
            .child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(SharedString::from(
                        aria_core::count::counted(installed, "font installed", "fonts installed")
                            .to_string(),
                    )),
            ),
    )
}

/// One family row. `family` is what the file stores, so the default row passes
/// `""` and sets the style back to unset.
fn font_row(
    theme: &Theme,
    surface: &'static str,
    family: &str,
    label: &str,
    on: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    // Keyed by family, so two rows never share a fade and the key survives the
    // window scrolling under the pointer.
    let key = MotionKey::new("font", family).with(surface);
    let background = crate::motion::hover_blend(key, theme.surface, theme.surface_raised_hover);
    let owned = family.to_string();
    div()
        .id(key.element_id("font"))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(background)
        .text_size(Theme::TEXT_TITLE)
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
        .child(picked(label.to_string(), on))
        .into_any_element()
}

fn step_button(
    theme: &Theme,
    surface: &'static str,
    forward: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let label = if forward {
        "Next font"
    } else {
        "Previous font"
    };
    pick_button(
        MotionKey::new("font-step", label).with(surface),
        label,
        theme,
        cx,
        move |state| clip_pick::apply(state, Pick::CycleFont { forward }),
    )
    .into_any_element()
}
