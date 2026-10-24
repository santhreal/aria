//! Info tab: read-only clip facts.

use aria_models::MediaId;

use gpui::{
    InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled, div, px,
};

use crate::components::{self, readout, section_label};
use crate::theme::Theme;
use crate::timecode::format_ticks;
use crate::types::UiClip;
use gpui::prelude::FluentBuilder;

use crate::panels::inspector::blocks::widgets::kind_label;

pub fn info_tab(
    theme: &Theme,
    clip: &UiClip,
    fps_num: i32,
    fps_den: i32,
    state: &gpui::Entity<crate::app::AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let media = clip
        .media_id
        .map(|id| id.to_string())
        .unwrap_or_else(|| "None".into());
    let media_missing = clip.media_id.is_some_and(|id| {
        state
            .read(cx)
            .store
            .media
            .iter()
            .any(|item| item.id == id && item.missing)
    });
    let media_id = clip.media_id.filter(|_| media_missing);
    let relink = media_id.map(|id| {
        components::ghost_button("relink-media", "Relink", theme).on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.pick_and_relink(MediaId::from_raw(id)));
                cx.notify();
            }),
        )
    });
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(theme, "Info"))
        .child(readout(theme, "Name", clip.name.clone()))
        .child(readout(theme, "Kind", kind_label(clip.kind)))
        .child(readout(
            theme,
            "In",
            format_ticks(clip.timeline_start_ticks, fps_num, fps_den),
        ))
        .child(readout(
            theme,
            "Out",
            format_ticks(clip.timeline_end_ticks, fps_num, fps_den),
        ))
        .child(readout(theme, "Media", media))
        .when_some(relink, |el, btn| el.child(btn))
        .child(crate::panels::inspector::blocks::template::template_block(
            theme, clip, state, cx,
        ))
}
