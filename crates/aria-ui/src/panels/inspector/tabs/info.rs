//! Info tab: read-only clip facts.

use aria_models::MediaId;

use gpui::{IntoElement, ParentElement, Styled, div, px};

use crate::components::section_label;
use crate::theme::Theme;
use crate::timecode::format_ticks;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::{
    Columns, fact_row, kind_label, pick_button, under_labels,
};

pub fn info_tab(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // The pool item the clip plays, by name, and whether its file is gone.
    let source = clip.media_id.and_then(|id| {
        state
            .read(cx)
            .store
            .media
            .iter()
            .find(|item| item.id == id)
            .map(|item| (item.name.clone(), item.missing))
    });
    let media_id = clip
        .media_id
        .filter(|_| source.as_ref().is_some_and(|(_, missing)| *missing));
    let media = source.map_or_else(|| "None".into(), |(name, _)| name);
    let relink = media_id.map(|id| {
        under_labels(
            Columns::Free,
            [
                pick_button("relink-media", "Relink", theme, cx, move |state| {
                    state.pick_and_relink(MediaId::from_raw(id));
                    Ok(())
                })
                .into_any_element(),
            ],
        )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Info"))
        .child(fact_row(theme, "Name", clip.name.clone(), Columns::Free))
        .child(fact_row(
            theme,
            "Kind",
            kind_label(clip.kind),
            Columns::Free,
        ))
        .child(fact_row(
            theme,
            "In",
            format_ticks(clip.timeline_start_ticks, fps_num, fps_den),
            Columns::Free,
        ))
        .child(fact_row(
            theme,
            "Out",
            format_ticks(clip.timeline_end_ticks, fps_num, fps_den),
            Columns::Free,
        ))
        .child(fact_row(theme, "Media", media, Columns::Free))
        .when_some(relink, |el, btn| el.child(btn))
        .child(crate::panels::inspector::blocks::template::template_block(
            theme, clip, state, cx,
        ))
}
