//! The Timeline File dialog: what the last timeline import or export could
//! not carry as it was.
//!
//! The dialog for [`crate::app::timeline_files`]. A run that met anything it
//! could not carry opens it, after an install prompt the same drop opened; a
//! clean run opens nothing, and the status line states the run. The lines come
//! from the plugin that read or wrote the file, in the order it met them.
//! Closing the dialog drops the report.

use aria_core::count::counted;
use gpui::{
    Entity, InteractiveElement, MouseButton, MouseDownEvent, ParentElement, SharedString, Styled,
    div, px,
};

use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::theme::Theme;

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let content = div().flex().flex_col().gap(px(Theme::SPACE_XS));
    let content = match &state.read(cx).timeline_report {
        Some(report) => content
            .child(
                div()
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text)
                    .child(report.title.clone()),
            )
            .child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .child(SharedString::from(format!(
                        "{} placed or written the nearest way",
                        counted(report.lines.len(), "item", "items")
                    ))),
            )
            .children(report.lines.iter().map(|line| {
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.warning)
                    .child(line.clone())
            })),
        None => content.child(
            div()
                .text_size(Theme::TEXT_UI)
                .text_color(theme.text_faint)
                .child("Nothing was placed or written the nearest way"),
        ),
    };
    let close = components::button("timeline-report-close", "Close", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    dialog_surface(
        "timeline-report",
        DialogKind::TimelineReport,
        "Timeline File",
        state,
        cx,
        DialogBody::new(content).action(close),
    )
}
