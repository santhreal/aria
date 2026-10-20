//! Which model, and whether the endpoint will answer.
//!
//! The AI section used to take a typed model id and a key and report nothing
//! else. Both are opaque: a typo in a model id reads exactly like a
//! model that is merely not installed, and a key that does not resolve reads
//! exactly like a key that does, right up until the first prompt fails. Two
//! controls close that:
//!
//! - **A model is a list, not a text field.** Rows come from
//!   [`AppState::ai_model_rows`], which is the curated catalog for the
//!   configured source crossed with what the endpoint reported. Clicking one
//!   writes the setting through the same commit path a typed field uses. The
//!   typed field stays: a custom endpoint serves whatever it serves.
//! - **A connection is tested, with the result on screen.** `Test connection`
//!   and `List models` are the provider's own calls, on the background executor,
//!   and the line under them is what the provider said — including its error,
//!   which names the endpoint and the status.
//!
//! Nothing here contacts anything while rendering. Both requests are clicks, for
//! the same reason the cloud feeds are: a render that started network traffic
//! would repeat it on every repaint.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::AppState;
use crate::app::ai_reach::AiModelRow;
use crate::components;
use crate::theme::Theme;

use super::super::widgets::kv;

/// The model list, the two provider probes, and the line the probe wrote.
pub(super) fn rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let busy = state.read(cx).ai_probe.busy;
    let hint = state.read(cx).ai_models_hint();
    let models = state.read(cx).ai_model_rows();

    let mut out = vec![
        kv(theme, "This model", state.read(cx).ai_model_detail()).into_any_element(),
        probe_buttons(theme, busy, cx),
        // The probe's own words. A loader while a request is in flight, because
        // "Testing…" that never changes is indistinguishable from a hung dialog.
        if busy {
            crate::loaders::pending_row(
                "ai-probe-busy",
                SharedString::from(hint),
                theme,
                cx.entity_id(),
                cx,
            )
            .into_any_element()
        } else {
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(hint))
                .into_any_element()
        },
    ];
    out.extend(models.into_iter().map(|row| model_row(theme, row, cx)));
    out
}

/// The two things worth asking a provider before spending a prompt on it.
fn probe_buttons(
    theme: &Theme,
    busy: bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    div()
        .flex()
        .flex_row()
        .gap(px(Theme::SPACE_XS))
        .child(
            components::ghost_button("ai-test-connection", "Test connection", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    if busy {
                        return;
                    }
                    this.state
                        .update(cx, |state, cx| state.test_ai_connection(cx));
                    cx.notify();
                }),
            ),
        )
        .child(
            components::ghost_button("ai-list-models", "List models", theme).on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    if busy {
                        return;
                    }
                    this.state.update(cx, |state, cx| state.probe_ai_models(cx));
                    cx.notify();
                }),
            ),
        )
        .into_any_element()
}

/// One offerable model: the name, what it is for, and whether it is there.
///
/// A model the endpoint does not have is still offered, marked. Hiding it would
/// hide the instruction: the row states the model to install, and a list that
/// silently omitted it would give no hint where it went.
fn model_row(
    theme: &Theme,
    row: AiModelRow,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = crate::motion::MotionKey::new("ai.model", &row.id);
    let id = key.element_id("ai-model");
    let lit = crate::motion::hover_t(key);
    let name_color = if row.selected {
        theme.text
    } else if row.available {
        crate::motion::mix(theme.text_muted, theme.text, lit)
    } else {
        theme.text_faint
    };
    let picked = row.id.clone();
    div()
        .id(id)
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .cursor(gpui::CursorStyle::PointingHand)
        .active(|style| style.opacity(0.92))
        .bg(crate::motion::hover_blend(
            key,
            theme.wash(if row.selected { 0.09 } else { 0.0 }),
            theme.wash(0.06),
        ))
        .on_hover(crate::motion::hover_listener(key))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(name_color)
                .child(if row.selected {
                    SharedString::from(format!("{} \u{2713}", row.label))
                } else {
                    row.label.clone()
                }),
        )
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child(row.detail),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                let picked = picked.clone();
                this.state.update(cx, |state, _| {
                    if let Err(err) = state.pick_ai_model(&picked) {
                        state.report_error(err);
                    }
                });
                cx.notify();
            }),
        )
        .into_any_element()
}

/// Every persisted settings table and what this build does with the version the
/// file records for it.
///
/// A configuration this build refuses fails to load with one sentence about one
/// table, which is the right refusal and the wrong amount of information. The
/// rows are enumerated from `aria_settings::schema::PersistedShape` at run time,
/// so a table added there appears here without an edit.
pub(super) fn schema_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let mut out = vec![
        kv(
            theme,
            "Config file",
            state.read(cx).settings_schema_status(),
        )
        .into_any_element(),
    ];
    out.extend(
        state
            .read(cx)
            .settings_schema_rows()
            .into_iter()
            .map(|(table, summary)| kv(theme, &table, summary).into_any_element()),
    );
    out
}
