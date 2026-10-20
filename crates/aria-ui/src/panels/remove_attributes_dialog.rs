//! The Remove Attributes dialog (Edit > Remove Attributes).

use crate::components::Wraps as _;
use aria_models::ClipAttribute;
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, ChipSize};
use crate::theme::Theme;

/// The element id of a group's toggle.
const fn chip_id(attribute: ClipAttribute) -> &'static str {
    match attribute {
        ClipAttribute::Motion => "remove-attr-motion",
        ClipAttribute::Opacity => "remove-attr-opacity",
        ClipAttribute::Crop => "remove-attr-crop",
        ClipAttribute::TimeRemapping => "remove-attr-time-remap",
        ClipAttribute::Volume => "remove-attr-volume",
        ClipAttribute::Pan => "remove-attr-pan",
        ClipAttribute::Effects => "remove-attr-effects",
        ClipAttribute::AudioEffects => "remove-attr-audio-effects",
        ClipAttribute::Grade => "remove-attr-grade",
        ClipAttribute::Mask => "remove-attr-mask",
        ClipAttribute::ChromaKey => "remove-attr-chroma",
    }
}

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let (rows, clips) = {
        let form = &state.read(cx).remove_attributes;
        (form.rows.clone(), form.held.len())
    };

    let cancel = components::ghost_button("remove-attr-cancel", "Cancel", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let apply = components::button("remove-attr-apply", "Remove", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.apply_remove_attributes());
            cx.notify();
        }),
    );

    let toggles = div()
        .flex()
        .wraps()
        .gap(px(Theme::SPACE_XS))
        .children(rows.into_iter().map(|(attribute, ticked)| {
            components::chip(
                ("remove-attr", chip_id(attribute)),
                attribute.label(),
                ticked,
                ChipSize::Regular,
                &theme,
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state
                        .update(cx, |state, _| state.toggle_remove_attribute(attribute));
                    cx.notify();
                }),
            )
        }));
    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_faint)
                .child(format!(
                    "Return the lit attributes of {clips} clip(s) to their defaults, keyframes included."
                )),
        )
        .child(toggles);
    let body = DialogBody::new(body).action(cancel).action(apply);
    dialog_surface(
        "remove-attributes",
        DialogKind::RemoveAttributes,
        "Remove Attributes",
        state,
        cx,
        body,
    )
}
