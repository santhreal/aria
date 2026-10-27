//! The Generators tab's clip types: one row per type a plugin lists, drawn in
//! the type's colour and icon. Clicking places the type at the playhead;
//! dragging carries it to a lane. A type whose generator cannot draw it shows
//! why under its name, and placing it reports the same reason.

use super::*;
use crate::catalog_text::ClipTypeChoice;
use crate::theme::Theme;

/// One row per clip type, for a [`super::grid::entry_list`].
pub(super) fn clip_type_rows(
    theme: &Theme,
    rows: &[ClipTypeChoice],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| clip_type_row(theme, row, index, cx))
        .collect()
}

fn clip_type_row(
    theme: &Theme,
    row: &ClipTypeChoice,
    index: usize,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = row.id.clone();
    let key = motion::MotionKey::new("sidebar.clip-type", &id);
    let lit = motion::hover_t(key);
    let text = if row.unavailable.is_none() {
        motion::mix(theme.text_muted, theme.text, lit)
    } else {
        theme.text_faint
    };
    let click_id = id.clone();
    let entry = super::grid::entry_frame(theme, key, false)
        .on_hover(motion::hover_listener(key))
        .child(
            div()
                .flex_none()
                .size(px(10.))
                .rounded(px(2.))
                .bg(theme.label(aria_models::LabelColor::from(row.colour), false)),
        )
        .children(
            row.icon
                .map(|path| crate::icons::icon(path).size(px(14.)).text_color(text)),
        )
        .child(
            div()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_color(text)
                        .child(row.label.clone()),
                )
                .children(
                    row.unavailable
                        .clone()
                        .map(|reason| div().text_color(theme.warning).child(reason)),
                ),
        )
        .on_drag(
            CatalogDrag {
                kind: CatalogDragKind::ClipType,
                id: id.clone(),
                label: row.label.clone(),
            },
            move |drag, _offset, _window, cx| {
                let label = drag.label.clone();
                cx.new(|_| DragGhost { label })
            },
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.add_clip_type(&click_id) {
                        state.report_error(&err);
                    }
                });
                spawn_preview(this.state.clone(), cx);
                cx.notify();
            }),
        );
    crate::pending::arriving(motion::MotionKey::new("clip-type-in", &id), index, entry)
}
