//! The Generators tab's clip types: one row per type a plugin lists, drawn in
//! the type's colour and icon. Clicking places the type at the playhead;
//! dragging carries it to a lane. A type whose generator cannot draw it shows
//! why, and placing it reports the same reason.

use super::*;
use crate::catalog_text::ClipTypeChoice;
use crate::motion::RiseOnHover as _;
use crate::theme::Theme;

pub(super) fn clip_type_grid(
    theme: &Theme,
    rows: &[ClipTypeChoice],
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .children(
            rows.iter()
                .enumerate()
                .map(|(index, row)| clip_type_row(theme, row, index, cx)),
        )
        .into_any_element()
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
    let placeable = row.unavailable.is_none();
    let text = if placeable {
        motion::mix(theme.text_muted, theme.text, lit)
    } else {
        theme.text_faint
    };
    let caption = match &row.unavailable {
        Some(reason) => div()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.warning)
            .child(reason.clone()),
        None => div()
            .text_size(Theme::TEXT_UI)
            .text_color(motion::mix(theme.text_faint, theme.text_muted, lit))
            .child("Click to place at the playhead, or drag onto a lane"),
    };
    let click_id = id.clone();
    let tile = div()
        .id(id.clone())
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(CursorStyle::PointingHand)
        .bg(motion::hover_blend(key, theme.wash(0.0), theme.wash(0.06)))
        .rise_on_hover(2.0, lit, motion::hover_listener(key))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
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
                        .text_size(Theme::TEXT_UI)
                        .text_color(text)
                        .child(row.label.clone()),
                ),
        )
        .child(caption)
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
    crate::pending::arriving(motion::MotionKey::new("clip-type-in", &id), index, tile)
}
