//! Transcript rows: the empty state and one line of the conversation.
//!
//! Split out of `ai_panel.rs` to stay under the file ceiling. A pure move; both
//! items are `pub(super)` so the parent reaches them by name.

use super::*;

/// What the panel says before anything has been asked. Names the project,
/// because the assistant answers about this one and not in general.
pub(super) fn empty(theme: &Theme, project: &str) -> impl IntoElement {
    div()
        .text_size(px(13.))
        .text_color(theme.text_faint)
        .child(format!("Ask Aria about {project}"))
}

/// One line of the conversation, coloured by who said it and copyable on its own.
///
/// A row copies itself rather than only the whole conversation: the useful thing
/// to paste is usually one answer, and selecting text inside a GPUI element is
/// not a thing this app has.
pub(super) fn bubble(
    theme: &Theme,
    entry: &UiAgentEntry,
    index: usize,
    // Whether tokens are still arriving into this row.
    live: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    // A caret at the end of the text while it grows: breathing rather than
    // blinking, on the same pulse the panel's loader uses, so one turn shows one
    // rhythm instead of two.
    let caret = if live {
        let phase = crate::motion::pulse_delta(&crate::motion::ZERON_PULSE, cx.entity_id(), cx);
        crate::motion::pulse_opacity(phase)
    } else {
        0.0
    };
    let (tint, text) = match entry.kind {
        AgentEntryKind::You => (theme.wash(0.10), theme.text),
        AgentEntryKind::Assistant => (theme.wash(0.06), theme.text),
        AgentEntryKind::Reasoning => (theme.wash(0.03), theme.text_muted),
        AgentEntryKind::Action => (theme.wash(0.06), theme.accent),
        AgentEntryKind::Error => (theme.wash(0.06), theme.danger),
    };
    let content = entry.content.clone();
    crate::motion::fade_in(
        crate::motion::MotionKey::new("ai", &entry.id),
        div()
            .p(px(Theme::SPACE_SM))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(tint)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme.text_faint)
                            .child(entry.kind.label()),
                    )
                    .child(
                        crate::components::ghost_button(
                            crate::motion::MotionKey::new("ai-copy", index),
                            "Copy",
                            theme,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    content.to_string(),
                                ));
                                this.state.update(cx, |state, _| {
                                    state.status_message = Some("Message copied".into());
                                });
                                cx.notify();
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(3.))
                    .text_size(px(13.))
                    .text_color(text)
                    .child(entry.content.clone())
                    .when(live, |el| {
                        el.child(
                            div()
                                .w(px(2.))
                                .h(px(13.))
                                .rounded(px(1.))
                                .bg(theme.caret.opacity(caret)),
                        )
                    }),
            ),
    )
}
