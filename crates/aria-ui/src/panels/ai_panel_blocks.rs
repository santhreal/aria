//! Two blocks of the assistant panel: the permission card and the skill list.
//!
//! Split out of [`super`] to keep that file readable. Pure move — both functions
//! are unchanged, and both are still only called from the panel that owns them.

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
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text)
                .child(gpui::SharedString::from(summary.to_string())),
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

/// Installed skills, whether each is offered to this conversation, and the
/// catalog they install from.
///
/// Installing is what makes a skill available at all —
/// [`aria_ai::extend::load_agent_dir`] reads the agent directory at send time,
/// puts every installed skill in the system prompt, and serves the body through
/// `read_skill`. Clicking a row holds one back for this conversation only, so a
/// procedure that does not apply to this task stops competing for the model's
/// attention without being uninstalled for the next one.
pub(super) fn skills_block(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let installed = state.read(cx).installed_skills();
    let offered = installed
        .iter()
        .filter(|skill| state.read(cx).assistant_skill_enabled(&skill.id))
        .count();
    let mut rows: Vec<gpui::AnyElement> = Vec::new();
    for skill in &installed {
        let id = skill.id.clone();
        let on = state.read(cx).assistant_skill_enabled(&id);
        rows.push(
            div()
                .id(crate::motion::MotionKey::new("ai-skill", &id).element_id("ai-skill"))
                .text_size(Theme::TEXT_UI)
                .text_color(if on { theme.text } else { theme.text_faint })
                .cursor(gpui::CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .hover(|el| el.bg(theme.wash(0.06)))
                .child(gpui::SharedString::from(format!(
                    "{} {} \u{2014} {}",
                    if on { "\u{2713}" } else { "\u{25cb}" },
                    skill.name,
                    skill.description
                )))
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
                .child(gpui::SharedString::from(if installed.is_empty() {
                    "No skills installed".to_string()
                } else {
                    format!(
                        "{offered} of {} offered to the agent",
                        counted(installed.len(), "skill", "skills")
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
