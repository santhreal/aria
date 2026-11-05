//! The Transcription section: the model catalog and the download timeouts.

use gpui::{Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent};

use crate::app::AppState;
use crate::app::models::TimeoutField;
use crate::components;
use crate::theme::Theme;

use super::value_row::{doubling_row, value_row};

/// One row per catalogued Whisper model: its size and its integrity state, with the
/// action that fixes a missing or corrupt file at the row's end, followed by the
/// download timeouts.
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
        .map(|row| {
            let mib = row.bytes / (1024 * 1024);
            let value = if row.detail.is_empty() {
                format!("{} · {mib} MiB on disk · {}", row.description, row.status)
            } else {
                format!(
                    "{} · {mib} MiB on disk · {}: {}",
                    row.description, row.status, row.detail
                )
            };
            let mut actions = Vec::new();
            if row.status != "Ready" {
                let model = row.model;
                let label = if row.reinstall {
                    "Re-download"
                } else {
                    "Install"
                };
                actions.push(
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
            value_row(theme, row.id, value, actions)
        })
        .collect();
    let timeouts = state.read(cx).model_timeouts();
    for (field, id, label) in TimeoutField::ALL {
        let secs = field.secs(&timeouts);
        rows.push(doubling_row(
            theme,
            ("timeout-dec", id),
            ("timeout-inc", id),
            label,
            (format!("{secs} s"), secs),
            move |state, next| state.set_model_timeout(field, next),
            cx,
        ));
    }
    rows
}
