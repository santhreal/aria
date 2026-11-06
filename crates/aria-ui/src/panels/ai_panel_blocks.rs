//! Two blocks of the assistant panel: the permission card and the skill list.
//!
//! Both functions are `pub(super)` and are called from the parent `ai_panel`
//! module.

use aria_core::count::counted;

use super::*;
use crate::theme::Theme;

/// A destructive host call the assistant is blocked on.
///
/// Shown in the transcript rather than as a dialog: it belongs to the turn that
/// asked, and a modal would hide the conversation that explains why. The turn
/// is genuinely waiting — the job thread is parked on this answer — so the card
/// states what would happen rather than which tool was called.
pub(super) fn ask_card(
    theme: &Theme,
    name: &str,
    summary: &str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    div()
        .p(px(Theme::SPACE_SM))
        .rounded(px(Theme::CONTROL_RADIUS))
        .border_1()
        .border_color(theme.warning)
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(gpui::SharedString::from(format!("{name} · needs you"))),
        )
        .child(
            // The summary is also the tail of the transcript's waiting line, so
            // it is written in lower case; the card draws it as a sentence.
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(crate::app::status::as_sentence(summary.to_string())),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap(px(Theme::SPACE_SM))
                .child(
                    crate::components::button("ai-ask-allow", "Allow", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.answer_agent_ask(true));
                            cx.notify();
                        }),
                    ),
                )
                .child(
                    crate::components::ghost_button("ai-ask-deny", "Deny", theme).on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _: &MouseDownEvent, _, cx| {
                            this.state
                                .update(cx, |state, _| state.answer_agent_ask(false));
                            cx.notify();
                        }),
                    ),
                ),
        )
}

/// The skills the assistant is offered, whether each is offered to this
/// conversation, and the catalog more install from.
///
/// The bundled skills and every installed one reach the system prompt —
/// [`aria_ai::extend::load_agent_dir`] reads the agent directory at send time,
/// [`aria_ai::merge_skills`] adds the bundled set, and `read_skill` serves the
/// body. Clicking a row holds one back for this conversation only, so a
/// procedure that does not apply to this task stops competing for the model's
/// attention without being uninstalled for the next one.
pub(super) fn skills_block(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let skills = state.read(cx).assistant_skills();
    let offered = skills
        .iter()
        .filter(|skill| state.read(cx).assistant_skill_enabled(&skill.id))
        .count();
    let total = skills.len();
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    for skill in skills {
        let id = skill.id;
        let on = state.read(cx).assistant_skill_enabled(&id);
        let key = crate::motion::MotionKey::new("ai-skill", &id);
        let selector = format!("ai-skill-switch-{id}");
        // The row presses the switch, as the Plan only label does: the name is
        // the larger target, and the switch states the answer.
        rows.push(
            div()
                .id(key.element_id("ai-skill"))
                .flex()
                .items_center()
                .gap(px(Theme::SPACE_SM))
                .text_size(Theme::TEXT_UI)
                .cursor(gpui::CursorStyle::PointingHand)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .gap(px(Theme::SPACE_XS))
                        .child(
                            div()
                                .flex_none()
                                .text_color(theme.text)
                                .child(gpui::SharedString::from(skill.name)),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_color(theme.text_faint)
                                .child(gpui::SharedString::from(skill.description)),
                        ),
                )
                .child(
                    crate::components::switch(("ai-skill-switch", key), Some(on), theme)
                        .debug_selector(move || selector),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        let id = id.clone();
                        this.state
                            .update(cx, |state, _| state.toggle_assistant_skill(&id));
                        cx.notify();
                    }),
                )
                .into_any_element(),
        );
    }
    div()
        .px(px(Theme::SPACE_MD))
        .py(px(Theme::SPACE_SM))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .border_t_1()
        .border_color(theme.border)
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(gpui::SharedString::from(if total == 0 {
                    "No skills".to_string()
                } else {
                    format!(
                        "{offered} of {} offered to the agent",
                        counted(total, "skill", "skills")
                    )
                })),
        )
        .children(rows)
        .child(crate::panels::cloud::cloud_catalog_block(
            theme,
            crate::cloud::CloudFeed::Skills,
            state,
            cx,
        ))
}

#[cfg(test)]
#[path = "ai_panel_skills_tests.rs"]
mod skills_tests;
