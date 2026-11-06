//! The AI section: the source, the model, the protocol, the reasoning summary,
//! the autonomy level, the typed endpoint and key, and where the agent directory
//! is.
//!
//! Every chip row is generated from the enum's own list in `aria-settings`, so a new
//! source or autonomy level appears here without an edit. The status line is
//! `aria-ai`'s own validation, so the dialog and the assistant cannot disagree about
//! what counts as configured. The model list and the two provider probes live in
//! [`models`], beside this file: they are the only rows here that talk to a
//! server, and keeping them apart is what stops a chip row from growing a
//! network call.

use aria_core::count::counted;
use aria_settings::{AiApiProtocol, AiSource, Autonomy, ReasoningSummary};
use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement, Styled,
    div, px,
};

use crate::app::AppState;
use crate::app::settings_form::SettingsField;
use crate::components;
use crate::theme::Theme;

use super::value_row::value_row;
use super::widgets::{choice_row, kv, row_label};

// Beside this file rather than in a directory: the module tree is still
// `settings_dialog::ai::models`.
#[path = "ai_models.rs"]
mod models;

/// One typed value. Clicking focuses it; the shell routes the keys, Enter saves, Esc
/// leaves the saved value alone. A secret shows its length, never its text. A
/// plugin field no plugin declares any more draws nothing.
pub(super) fn field_row(
    theme: &Theme,
    state: &Entity<AppState>,
    field: SettingsField,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let Some(look) = state.read(cx).setting_look(field) else {
        return div().into_any_element();
    };
    let (value, focused) = state.read(cx).setting_draft(field);
    let editing = state.read(cx).settings_form.draft.editing(focused);
    let value = if look.kind == aria_plugin::SettingKind::Secret && !focused && !value.is_empty() {
        let count = value.chars().count();
        format!("Stored, {}", counted(count, "character", "characters"))
    } else {
        value
    };
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(row_label(theme, look.label))
        .child(
            components::text_box(
                ("settings-field", field.id()),
                &value,
                look.placeholder,
                editing,
                theme,
            )
            .flex_1(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state
                    .update(cx, |state, _| state.focus_setting(Some(field)));
                cx.notify();
            }),
        )
        .into_any_element()
}

pub(super) fn ai_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let (source, protocol, summary, autonomy) = {
        let ai = &state.read(cx).cloud_settings().ai;
        (
            ai.source,
            ai.api_protocol,
            ai.reasoning_summary,
            ai.autonomy,
        )
    };
    let status = state.read(cx).ai_status();
    let agent_dir = state.read(cx).agent_dir();
    let agent_label = agent_dir.display().to_string();
    // A turn in flight is the one AI fact that changes while this dialog is
    // open, and the only one a static status line cannot tell you: "configured"
    // reads the same whether the provider is answering or hung.
    let asking = state.read(cx).assistant.running.then(|| {
        crate::loaders::pending_row(
            "ai-turn-running",
            "Waiting on the provider",
            theme,
            cx.entity_id(),
            cx,
        )
        .into_any_element()
    });
    let reveal = components::ghost_button("agent-dir-reveal", "Reveal agent files", theme)
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                this.state.update(cx, |state, _| {
                    state.reveal_agent_dir();
                });
                cx.notify();
            }),
        )
        .into_any_element();
    let mut rows = vec![choice_row(
        theme,
        "ai-source",
        "Source",
        &AiSource::ALL,
        AiSource::label,
        source,
        cx,
        |state, source, _| state.set_ai_source(source),
    )];
    // The order Tab walks the typed fields is the order they are drawn in.
    rows.extend(SettingsField::AI.map(|field| field_row(theme, state, field, cx)));
    rows.extend([
        choice_row(
            theme,
            "ai-protocol",
            "Protocol",
            &AiApiProtocol::ALL,
            AiApiProtocol::label,
            protocol,
            cx,
            |state, protocol, _| state.set_ai_protocol(protocol),
        ),
        choice_row(
            theme,
            "ai-reasoning",
            "Reasoning",
            &ReasoningSummary::ALL,
            ReasoningSummary::label,
            summary,
            cx,
            |state, summary, _| state.set_reasoning_summary(summary),
        ),
        choice_row(
            theme,
            "ai-autonomy",
            "Autonomy",
            &Autonomy::ALL,
            Autonomy::label,
            autonomy,
            cx,
            |state, autonomy, _| state.set_autonomy(autonomy),
        ),
        kv(theme, "Status", status).into_any_element(),
        value_row(theme, "Agent files", agent_label, [reveal]),
        kv(theme, "Limits", state.read(cx).agent_limits()).into_any_element(),
        kv(theme, "Can do", state.read(cx).agent_capabilities()).into_any_element(),
        kv(theme, "Active", state.read(cx).agent_phases()).into_any_element(),
    ]);
    // The model list and the probes sit under the typed fields, because they
    // answer the question those fields raise: whether this endpoint has this
    // model and will accept this key.
    rows.extend(models::rows(theme, state, cx));
    rows.extend(asking);
    rows
}

/// What this build does with every table the configuration file records.
///
/// Its own section rather than a row of the AI one: a schema version belongs to
/// the file, not to the assistant, and the one time it matters is when a
/// configuration written by another build will not load.
pub(super) fn schema_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    models::schema_rows(theme, state, cx)
}
