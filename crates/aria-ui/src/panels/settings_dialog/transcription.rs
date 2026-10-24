//! The Transcription section: the model catalog and the download timeouts.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::components;
use crate::theme::Theme;

use super::widgets::kv;

/// One row per catalogued Whisper model: its size, its integrity state, and the action
/// that fixes a missing or corrupt file, followed by the download timeouts.
///
/// A corrupt model offers "Re-download" rather than the same "Install" label: the
/// failure is on disk, and the label has to state that the local file is the problem.
///
/// The timeout rows are generated from `TimeoutField::ALL`, so a new timeout appears
/// here without a second edit.
pub(super) fn model_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<gpui::AnyElement> {
    let mut rows: Vec<gpui::AnyElement> = state
        .read(cx)
        .model_rows()
        .into_iter()
        .flat_map(|row| {
            let mib = row.bytes / (1024 * 1024);
            let value = if row.detail.is_empty() {
                format!("{} · {mib} MiB on disk · {}", row.description, row.status)
            } else {
                format!(
                    "{} · {mib} MiB on disk · {}: {}",
                    row.description, row.status, row.detail
                )
            };
            let mut elements = vec![kv(theme, row.id, value).into_any_element()];
            if row.status != "Ready" {
                let model = row.model;
                let label = if row.reinstall {
                    "Re-download"
                } else {
                    "Install"
                };
                elements.push(
                    components::ghost_button(
                        crate::motion::MotionKey::new("model-install", row.id),
                        label,
                        theme,
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.state.update(cx, |state, _| {
                                if let Err(err) = state.install_model(model) {
                                    state.report_error(err);
                                }
                            });
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                );
            }
            elements
        })
        .collect();
    let timeouts = state.read(cx).model_timeouts();
    for (field, id, label) in crate::app::models::TimeoutField::ALL {
        let secs = field.secs(&timeouts);
        rows.push(timeout_row(theme, field, id, label, secs, cx).into_any_element());
    }
    rows
}

/// One row per download timeout: the value and a pair of steppers.
///
/// A timeout is a number set in seconds, and the useful range
/// spans a connect timeout and a half-hour deadline, so the step doubles rather than
/// adding a constant.
fn timeout_row(
    theme: &Theme,
    field: crate::app::models::TimeoutField,
    id: &'static str,
    label: &'static str,
    secs: u64,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let less = components::ghost_button(
        crate::motion::MotionKey::new("timeout-dec", id),
        "Halve",
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                let next = (secs / 2).max(1);
                if let Err(err) = state.set_model_timeout(field, next) {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    );
    let more = components::ghost_button(
        crate::motion::MotionKey::new("timeout-inc", id),
        "Double",
        theme,
    )
    .on_mouse_down(
        MouseButton::Left,
        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
            this.state.update(cx, |state, _| {
                let next = secs.saturating_mul(2);
                if let Err(err) = state.set_model_timeout(field, next) {
                    state.report_error(err);
                }
            });
            cx.notify();
        }),
    );
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_muted)
                .child(SharedString::from(format!("{label} timeout: {secs}s"))),
        )
        .child(less)
        .child(more)
}
