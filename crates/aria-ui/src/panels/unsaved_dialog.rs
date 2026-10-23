//! The unsaved-changes prompt.
//!
//! Three answers, and no fourth: save and go on, go on anyway, or stay. The
//! panel knows nothing about what it is interrupting — the state holds the
//! parked intent — so a new way of leaving a project reuses this dialog as it
//! stands.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::unsaved::PendingIntent;
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::theme::Theme;

pub fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let (subject, going) = {
        let state = state.read(cx);
        (
            state.unsaved_subject(),
            state.pending_intent().map(PendingIntent::outcome),
        )
    };

    let stay = components::ghost_button("unsaved-cancel", "Stay", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| state.cancel_pending());
            cx.notify();
        }),
    );
    let discard = components::ghost_button("unsaved-discard", "Discard", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.discard_and_continue());
            cx.notify();
        }),
    );
    let save = components::button("unsaved-save", "Save", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| state.save_and_continue());
            cx.notify();
        }),
    );

    let body = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_MD))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(format!("{subject} has changes you have not saved.")),
        )
        .child(
            div()
                .text_size(Theme::TEXT_TITLE)
                .text_color(theme.text_faint)
                .child(match going {
                    // The prompt says what the answer does, because "Discard"
                    // and "Save" both leave this project for one specific
                    // destination.
                    Some(outcome) => format!("Discard leaves them behind for good. {outcome}"),
                    None => "Discard leaves them behind for good.".to_string(),
                }),
        );
    let body = DialogBody::new(body)
        .action(stay)
        .action(discard)
        .action(save);
    dialog_surface(
        "unsaved",
        DialogKind::UnsavedChanges,
        "Unsaved Changes",
        state,
        cx,
        body,
    )
}
