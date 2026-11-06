//! Text tab: a title's content, its type properties, and its text-effect preset.
//!
//! The numeric and color properties come from the catalog rather than from a
//! list here, so the size, the spacing, the fill, the stroke and the shadow are
//! all animatable through the same keyframe-aware path every other block writes
//! through. The Character and Paragraph controls are drawn here once and shared
//! with the Character and Paragraph dock panels, so a title has one set of
//! writes whichever surface edits it.

use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use aria_models::{TextAlignH, TextAlignV, TextCase};

use crate::app::clip_text::TextSwitch;
use crate::motion::MotionKey;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::{RenameTarget, UiClip};

use crate::panels::inspector::blocks::font_list::font_list;
use crate::panels::inspector::blocks::widgets::{
    Chip, Columns, chip_row, choice_chip, color_rows, label_note, param_rows, pick_button,
    switch_line, under_labels,
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
    let effects = crate::catalog_text::text_effects();
    let current = effects
        .iter()
        .find(|choice| active.as_deref() == Some(choice.id.as_str()))
        .map_or(SharedString::new_static("None"), |choice| {
            choice.label.clone()
        });
    let presets: Vec<Chip> = effects
        .iter()
        .map(|choice| {
            let id = choice.id.as_str();
            let on = active.as_deref() == Some(id);
            choice_chip(
                MotionKey::new("text-effect", id),
                choice.label.clone(),
                on,
                theme,
                cx,
                move |state| {
                    let pick = if on {
                        Pick::ClearTextEffect
                    } else {
                        Pick::TextEffect(id.to_string())
                    };
                    clip_pick::apply(state, pick)
                },
            )
        })
        .collect();
    let content = match content {
        Some(content) => label_note(Columns::Keyed, theme.text, content),
        None => label_note(Columns::Keyed, theme.text_faint, "No text"),
    };
    let edit = pick_button("text-edit", "Edit text…", theme, cx, |state| {
        state.begin_rename(RenameTarget::ClipText);
        Ok(())
    });

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(content)
        .child(under_labels(Columns::Keyed, [edit.into_any_element()]))
        .children(character_controls(
            theme,
            INSPECTOR,
            Columns::Keyed,
            state,
            cx,
        ))
        .children(paragraph_controls(
            theme,
            INSPECTOR,
            Columns::Keyed,
            state,
            cx,
        ))
        .child(section_label(theme, "Effect"))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "text-effect"),
            "Preset",
            Columns::Keyed,
            current,
            presets,
        ))
        .child(section_label(theme, "Cloud presets"))
        .child(under_labels(
            Columns::Keyed,
            [div()
                .w_full()
                .child(cloud_presets(theme, state, cx))
                .into_any_element()],
        ))
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

/// The Character controls: the font list, the face switches, the case, the
/// treatments around the glyphs, then the catalog's size, spacing and color
/// rows. Empty when the selection is not a title.
///
/// `surface` names where the controls are drawn and is folded into every key.
/// `columns` places the switch and case lines: the keyed rows' columns on the
/// Text tab, free on the Character panel, as [`paragraph_controls`] places
/// its lines.
pub fn character_controls(
    theme: &Theme,
    surface: &'static str,
    columns: Columns,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let Some(face) = state.read(cx).selected_text_face() else {
        return Vec::new();
    };
    let cases: Vec<Chip> = TextCase::ALL
        .iter()
        .map(|&case| {
            choice_chip(
                MotionKey::new("text-case", case.label()).with(surface),
                case.label(),
                face.case == case,
                theme,
                cx,
                move |state| clip_pick::apply(state, Pick::TextCase(case)),
            )
        })
        .collect();
    let mut out = vec![
        section_label(theme, "Font").into_any_element(),
        font_picker(theme, surface, state, cx).into_any_element(),
        section_label(theme, "Style").into_any_element(),
    ];
    for &switch in &TextSwitch::FACE {
        let on = face.is_on(switch);
        out.push(text_switch(theme, surface, columns, switch, on, cx));
    }
    let case = MotionKey::new("insp.row", "text-case").with(surface);
    let case = chip_row(theme, case, "Case", columns, face.case.label(), cases);
    out.push(case.into_any_element());
    out.push(section_label(theme, "Treatments").into_any_element());
    for &switch in &TextSwitch::TREATMENTS {
        let on = face.is_on(switch);
        out.push(text_switch(theme, surface, columns, switch, on, cx));
    }
    out.push(section_label(theme, "Size and spacing").into_any_element());
    out.extend(param_rows(theme, "text", state, cx));
    out.push(section_label(theme, "Color").into_any_element());
    out.extend(color_rows(theme, "text", state, cx));
    out
}

/// The Paragraph controls: where the lines sit across the frame, where the
/// block sits down it, and whether long lines wrap. Empty when the selection is
/// not a title.
///
/// `columns` places the lines: the keyed rows' columns on the Text tab, beside
/// the rows that key; free on the Paragraph panel, which draws no keyed rows,
/// so a label stands at the panel's edge and its value at the other.
pub fn paragraph_controls(
    theme: &Theme,
    surface: &'static str,
    columns: Columns,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    let Some(face) = state.read(cx).selected_text_face() else {
        return Vec::new();
    };
    let across: Vec<Chip> = TextAlignH::ALL
        .iter()
        .map(|&align| {
            choice_chip(
                MotionKey::new("text-align-h", align.label()).with(surface),
                align.label(),
                face.align_h == align,
                theme,
                cx,
                move |state| clip_pick::apply(state, Pick::TextAlignH(align)),
            )
        })
        .collect();
    let down: Vec<Chip> = TextAlignV::ALL
        .iter()
        .map(|&align| {
            choice_chip(
                MotionKey::new("text-align-v", align.label()).with(surface),
                align.label(),
                face.align_v == align,
                theme,
                cx,
                move |state| clip_pick::apply(state, Pick::TextAlignV(align)),
            )
        })
        .collect();
    let across = chip_row(
        theme,
        MotionKey::new("insp.row", "text-align-h").with(surface),
        "Across",
        columns,
        face.align_h.label(),
        across,
    );
    let down = chip_row(
        theme,
        MotionKey::new("insp.row", "text-align-v").with(surface),
        "Down",
        columns,
        face.align_v.label(),
        down,
    );
    let wrap = face.is_on(TextSwitch::Wrap);
    vec![
        section_label(theme, "Alignment").into_any_element(),
        across.into_any_element(),
        down.into_any_element(),
        section_label(theme, "Lines").into_any_element(),
        text_switch(theme, surface, columns, TextSwitch::Wrap, wrap, cx),
    ]
}

/// The line of `switch` on `surface` in `columns`: its label, and a switch on
/// while the selected title wears it. A press toggles it on every selected
/// title. Every [`TextSwitch`] is drawn through here, so an on/off property of
/// a title reads as a switch on every surface.
fn text_switch(
    theme: &Theme,
    surface: &'static str,
    columns: Columns,
    switch: TextSwitch,
    on: bool,
    cx: &gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    switch_line(
        theme,
        MotionKey::new("insp.row", "text-switch")
            .with(switch.label())
            .with(surface),
        switch.label(),
        columns,
        MotionKey::new("text-switch", switch.label()).with(surface),
        Some(on),
        cx,
        move |state| clip_pick::apply(state, Pick::TextSwitch(switch)),
    )
    .into_any_element()
}

/// The typeface picker: the system default, which an empty family means in the
/// file, then every installed family, the one in use on the selected plate.
/// Clicking a name sets it on every selected title.
fn font_picker(
    theme: &Theme,
    surface: &'static str,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let families = state.read(cx).font_families();
    let current = state.read(cx).selected_font().unwrap_or_default();
    font_list(
        theme,
        MotionKey::new("font", surface),
        families,
        Some("System default"),
        Some(&current),
        cx,
        |state, family| clip_pick::apply(state, Pick::Font(family.to_owned())),
    )
}
