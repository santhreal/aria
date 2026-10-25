//! Template block of the Info tab: replace media, and the authoring marks.
//!
//! Three engine commands surface here. Replace media repoints a clip at other
//! pooled media while keeping its placement. The slot marks a clip as fillable
//! when the project is used as a template, and the hint is what the placeholder
//! reads. Text editability is offered only on a text clip, which is the only clip
//! the model accepts it on.

use gpui::{IntoElement, ParentElement, Styled, div, px};

use aria_models::MediaId;

use crate::app::clip_template::slot_media_label;
use crate::components::section_label;
use crate::surfaces::clip_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::{UiClip, UiClipKind};
use gpui::prelude::FluentBuilder;

use super::widgets::{
    Chip, Columns, Readout, chip_row, control_line, fact_row, ghosts_under_labels, label_note,
    pick_button, unkeyed_row,
};

pub fn template_block(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let state = gpui::Entity::clone(state);
    let slot = clip.replaceable.clone();
    let choices = state.read(cx).selected_slot_choices();

    // Only pooled media of a compatible kind can replace this clip's source: an
    // audio clip cannot take a picture, and a visual clip cannot take a song.
    let want_audio = clip.kind == UiClipKind::Audio;
    let pool: Vec<_> = state
        .read(cx)
        .store
        .media
        .iter()
        .filter(|item| !item.missing && (item.kind == UiClipKind::Audio) == want_audio)
        .filter(|item| Some(item.id) != clip.media_id)
        .map(|item| (item.id, item.name.clone()))
        .collect();

    // One line per item, as a list reads, each label on the label column.
    let replace_rows: Vec<_> = pool
        .into_iter()
        .map(|(id, name)| {
            ghosts_under_labels(
                Columns::Free,
                [pick_button(
                    crate::motion::MotionKey::new("inspector-replace-media", id),
                    name,
                    theme,
                    cx,
                    move |state| clip_pick::apply(state, Pick::ReplaceMedia(MediaId::from_raw(id))),
                )
                .into_any_element()],
            )
            .into_any_element()
        })
        .collect();

    let accepts_chips: Vec<_> = choices
        .into_iter()
        .map(|(kind, label)| {
            let selected = slot.as_ref().is_some_and(|s| s.accepts == kind);
            Chip::choice(label, selected, |shown| {
                pick_button(
                    ("template-accepts", label),
                    shown,
                    theme,
                    cx,
                    move |state| clip_pick::apply(state, Pick::SlotAccepts(kind)),
                )
                .into_any_element()
            })
        })
        .collect();

    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Template"))
        .when(clip.media_id.is_some() && !replace_rows.is_empty(), |el| {
            el.child(label_note(Columns::Free, theme.text_faint, "Replace media"))
                .children(replace_rows)
        })
        .child(ghosts_under_labels(
            Columns::Free,
            [pick_button(
                "inspector-slot-toggle",
                if slot.is_some() {
                    "Clear template slot"
                } else {
                    "Mark as template slot"
                },
                theme,
                cx,
                |state| clip_pick::apply(state, Pick::ToggleReplaceable),
            )
            .into_any_element()],
        ))
        .when_some(slot, |el, slot| {
            let clear = slot.label.is_some().then(|| {
                pick_button(
                    "inspector-slot-hint-clear",
                    "Clear hint",
                    theme,
                    cx,
                    |state| clip_pick::apply(state, Pick::ClearSlotLabel),
                )
                .into_any_element()
            });
            el.child(unkeyed_row(
                theme,
                &state,
                crate::motion::MotionKey::new("insp.row", "inspector-slot-order"),
                "Fill order",
                Readout::number(slot.order as f32, 0, 1.0),
                Columns::Free,
                |state, by| clip_pick::apply(state, Pick::SlotOrder(by.round() as i32)),
                cx,
            ))
            .child(chip_row(
                theme,
                crate::motion::MotionKey::new("insp.row", "template-accepts"),
                "Accepts",
                Columns::Free,
                slot_media_label(slot.accepts),
                accepts_chips,
            ))
            .child(fact_row(
                theme,
                "Hint",
                slot.label.unwrap_or_else(|| "None".into()),
                Columns::Free,
            ))
            .child(ghosts_under_labels(
                Columns::Free,
                std::iter::once(
                    pick_button("inspector-slot-hint", "Edit hint…", theme, cx, |state| {
                        state.begin_rename(crate::types::RenameTarget::SlotLabel);
                        Ok(())
                    })
                    .into_any_element(),
                )
                .chain(clear),
            ))
        })
        .when(clip.kind == UiClipKind::Text, |el| {
            el.child(control_line(
                theme,
                crate::motion::MotionKey::new("insp.row", "inspector-text-editable"),
                "Text editable in template",
                Columns::Free,
                pick_button(
                    "inspector-text-editable",
                    if clip.text_editable { "On" } else { "Off" },
                    theme,
                    cx,
                    |state| clip_pick::apply(state, Pick::ToggleTextEditable),
                ),
            ))
        })
}
