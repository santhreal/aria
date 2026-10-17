//! The Marker Edit dialog (`DialogKind::MarkerEdit`).

use aria_models::{MarkerColor, MarkerKind};
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::{AppState, DialogKind, MarkerEditField};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
use crate::theme::Theme;

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

    let name_field = field_row(
        "Name",
        &form.name,
        "Marker name",
        form.focused == Some(MarkerEditField::Name),
        MarkerEditField::Name,
        &theme,
        cx,
    );

    let comment_field = field_row(
        "Comment",
        &form.comment,
        "Note or review comment",
        form.focused == Some(MarkerEditField::Comment),
        MarkerEditField::Comment,
        &theme,
        cx,
    );

    let duration_field = field_row(
        "Duration (timecode)",
        &form.duration,
        "00:00:00:00",
        form.focused == Some(MarkerEditField::Duration),
        MarkerEditField::Duration,
        &theme,
        cx,
    );

    let color_row = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_muted)
                .child("Colour"),
        )
        .child(div().flex().flex_wrap().gap(px(Theme::SPACE_XS)).children(
            MarkerColor::ALL.into_iter().map(|color| {
                let active = form.color == color;
                let (id, label) = color_meta(color);
                let border = if active { theme.accent } else { theme.border };
                div()
                    .id(id)
                    .px(px(Theme::SPACE_SM))
                    .py(px(Theme::SPACE_XXS))
                    .rounded(px(Theme::CONTROL_RADIUS))
                    .border_1()
                    .border_color(border)
                    .bg(if active {
                        theme.selection
                    } else {
                        theme.input_bg
                    })
                    .cursor_pointer()
                    .flex()
                    .items_center()
                    .gap(px(Theme::SPACE_XS))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.set_marker_edit_color(color));
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(10.))
                            .rounded(px(2.))
                            .bg(theme.marker(color)),
                    )
                    .child(
                        div()
                            .text_size(Theme::TEXT_UI)
                            .text_color(if active { theme.text } else { theme.text_muted })
                            .child(label),
                    )
            }),
        ));

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
        .child(div().flex().flex_wrap().gap(px(Theme::SPACE_XS)).children(
            MarkerKind::ALL.into_iter().map(|kind| {
                let active = form.kind == kind;
                let id = match kind {
                    MarkerKind::Comment => "marker-edit-kind-comment",
                    MarkerKind::Chapter => "marker-edit-kind-chapter",
                    MarkerKind::Segmentation => "marker-edit-kind-segmentation",
                };
                components::chip(id, kind.label(), active, ChipSize::Regular, &theme).on_mouse_down(
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
        .child(name_field)
        .child(comment_field)
        .child(duration_field)
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

#[allow(clippy::too_many_arguments)]
fn field_row(
    label: &'static str,
    value: &str,
    placeholder: &'static str,
    focused: bool,
    field: MarkerEditField,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = match field {
        MarkerEditField::Name => "marker-edit-name",
        MarkerEditField::Comment => "marker-edit-comment",
        MarkerEditField::Duration => "marker-edit-duration",
    };
    let border = if focused { theme.accent } else { theme.border };
    let text = if value.is_empty() && !focused {
        placeholder.to_string()
    } else if focused {
        format!("{value}|")
    } else {
        value.to_string()
    };
    let color = if value.is_empty() && !focused {
        theme.text_muted
    } else {
        theme.text
    };
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
            div()
                .id(id)
                .h(px(28.))
                .px(px(Theme::SPACE_SM))
                .rounded(px(Theme::CONTROL_RADIUS))
                .border_1()
                .border_color(border)
                .bg(theme.input_bg)
                .cursor_pointer()
                .flex()
                .items_center()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.focus_marker_edit_field(Some(field)));
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .text_size(Theme::TEXT_TITLE)
                        .text_color(color)
                        .child(text),
                ),
        )
        .into_any_element()
}

fn color_meta(color: MarkerColor) -> (&'static str, &'static str) {
    match color {
        MarkerColor::Teal => ("marker-edit-color-teal", "Teal"),
        MarkerColor::Blue => ("marker-edit-color-blue", "Blue"),
        MarkerColor::Green => ("marker-edit-color-green", "Green"),
        MarkerColor::Yellow => ("marker-edit-color-yellow", "Yellow"),
        MarkerColor::Orange => ("marker-edit-color-orange", "Orange"),
        MarkerColor::Red => ("marker-edit-color-red", "Red"),
        MarkerColor::Pink => ("marker-edit-color-pink", "Pink"),
        MarkerColor::Purple => ("marker-edit-color-purple", "Purple"),
    }
}
