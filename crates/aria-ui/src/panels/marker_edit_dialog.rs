//! The Marker Edit dialog (`DialogKind::MarkerEdit`).

use crate::components::Wraps as _;
use aria_models::{MarkerColor, MarkerKind};
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::{AppState, DialogKind, MarkerEditField};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
use crate::swatch;
use crate::theme::Theme;
use crate::tooltip::WithTip as _;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let form = state.read(cx).marker_edit.clone();

    let cancel = components::ghost_button("marker-edit-cancel", "Cancel", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let apply = components::button("marker-edit-save", "Save", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| state.apply_marker_edit());
            cx.notify();
        }),
    );

    // The order Tab walks the boxes is the order they are drawn in.
    let fields = MarkerEditField::ORDER.map(|field| {
        field_row(
            field,
            form.line(field),
            form.focused == Some(field),
            &theme,
            cx,
        )
    });

    // The colors are swatches, as the label palette's are: the chosen one is
    // ringed, and its name follows the row so the choice reads as words too.
    let swatches = MarkerColor::ALL
        .into_iter()
        .enumerate()
        .map(|(index, color)| {
            let id = ("marker-edit-color", index);
            let chosen = form.color == color;
            swatch::swatch(id, theme.marker(color), chosen, &theme)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.set_marker_edit_color(color));
                        cx.notify();
                    }),
                )
                .with_tip(crate::tooltip::text_builder(color.label()))
        });
    let color_row = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child("Color"),
        )
        .child(
            div()
                .debug_selector(|| "marker-edit-colors".into())
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .children(swatches)
                .child(
                    div()
                        .pl(px(Theme::SPACE_XS))
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(form.color.label()),
                ),
        );

    let kind_row = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child("Kind"),
        )
        .child(div().flex().wraps().gap(px(Theme::SPACE_XS)).children(
            MarkerKind::ALL.into_iter().map(|kind| {
                let active = form.kind == kind;
                let id = match kind {
                    MarkerKind::Comment => "marker-edit-kind-comment",
                    MarkerKind::Chapter => "marker-edit-kind-chapter",
                    MarkerKind::Segmentation => "marker-edit-kind-segmentation",
                };
                components::chip(
                    ("marker-edit-kind", id),
                    kind.label(),
                    active,
                    ChipSize::Regular,
                    &theme,
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.set_marker_edit_kind(kind));
                        cx.notify();
                    }),
                )
            }),
        ));

    let mut content = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .children(fields)
        .child(color_row)
        .child(kind_row);

    if let Some(err) = form.error {
        content = content.child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.danger)
                .child(err),
        );
    }

    let body = DialogBody::new(content).action(cancel).action(apply);
    dialog_surface(
        "marker-edit",
        DialogKind::MarkerEdit,
        "Edit Marker",
        state,
        cx,
        body,
    )
}

fn field_row(
    field: MarkerEditField,
    value: &crate::app::line_text::LineText,
    focused: bool,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let (label, placeholder) = field.caption();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XXS))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child(label),
        )
        .child(
            components::text_box(
                ("marker-edit-field", label),
                value,
                placeholder,
                value.editing(focused),
                theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.focus_marker_edit_field(Some(field)));
                    cx.notify();
                }),
            ),
        )
        .into_any_element()
}

#[cfg(test)]
#[path = "marker_edit_dialog_tests.rs"]
mod tests;
