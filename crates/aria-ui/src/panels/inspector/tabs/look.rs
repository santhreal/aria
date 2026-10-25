//! Look tab: the filter catalog and its amount.

use gpui::{IntoElement, ParentElement, SharedString, Styled, div, px};

use crate::components::section_label;
use crate::motion::MotionKey;
use crate::surfaces::inspector_pick::{self, Pick};
use crate::theme::Theme;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{Chip, Columns, chip_row, param_rows, pick_button};

pub fn look_tab(
    theme: &Theme,
    clip: &UiClip,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let applied = clip.filter.as_ref().map(|filter| filter.id.clone());
    let mut chips: Vec<_> = crate::catalog_text::filters()
        .iter()
        .map(|choice| {
            let id = choice.id.to_string();
            let on = applied.as_ref() == Some(&choice.id);
            Chip::choice(choice.label.clone(), on, |label| {
                pick_button(
                    MotionKey::new("look-choice", &choice.id),
                    label,
                    theme,
                    cx,
                    move |state| inspector_pick::apply(state, Pick::Filter(id.clone())),
                )
                .into_any_element()
            })
        })
        .collect();
    if applied.is_some() {
        chips.push(Chip::other(pick_button(
            "look-clear",
            "Clear",
            theme,
            cx,
            |state| inspector_pick::apply(state, Pick::ClearFilter),
        )));
    }
    // The catalog label, as the chip reads; the id stands in for a filter the
    // catalog no longer lists.
    let shown = applied.map_or_else(
        || SharedString::new_static("None"),
        |id| {
            crate::catalog_text::filters()
                .iter()
                .find(|choice| choice.id == id)
                .map_or(id, |choice| choice.label.clone())
        },
    );
    let amount = clip
        .filter
        .is_some()
        .then(|| param_rows(theme, "filter", state, cx))
        .unwrap_or_default();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Look"))
        .child(chip_row(
            theme,
            MotionKey::new("insp.row", "look-filter"),
            "Filter",
            Columns::Keyed,
            shown,
            chips,
        ))
        .children(amount)
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
        .when(
            clip.transition_media == Some(aria_models::TransitionMedia::Picture),
            |el| {
                el.child(
                    crate::panels::inspector::blocks::transition::transition_block(
                        theme, clip, state, cx,
                    ),
                )
            },
        )
}
