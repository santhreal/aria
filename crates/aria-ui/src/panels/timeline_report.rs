//! Timeline file: what the last timeline import or export could not carry as
//! it was.
//!
//! The panel for [`crate::app::timeline_files`]. It is shown while a report
//! holds anything: a clean run leaves no report, and the status line states
//! the run. The lines come from the plugin that read or wrote the file, in
//! the order it met them.

use gpui::{
    Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::components::ghost_button;
use crate::motion;
use crate::theme::Theme;

/// Whether the panel has anything to draw.
pub(crate) fn has_report(state: &AppState) -> bool {
    state.timeline_report.is_some()
}

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let (title, lines) = match &state.read(cx).timeline_report {
        Some(report) => (report.title.clone(), report.lines.clone()),
        None => (SharedString::default(), Vec::new()),
    };
    let summary = match lines.len() {
        1 => "1 item placed or written the nearest way".to_owned(),
        count => format!("{count} items placed or written the nearest way"),
    };

    motion::fade_in(
        "timeline-report-panel",
        div()
            .flex()
            .flex_col()
            .gap(px(Theme::SPACE_XS))
            .p(px(Theme::SPACE_SM))
            .rounded(px(Theme::CONTROL_RADIUS))
            .bg(theme.surface_raised)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.text)
                    .child(title),
            )
            .child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child(SharedString::from(summary)),
            )
            .children(lines.into_iter().map(|line| {
                div()
                    .text_size(Theme::TEXT_TITLE)
                    .text_color(theme.warning)
                    .child(line)
                    .into_any_element()
            }))
            .child(div().flex().child(
                ghost_button("timeline-report-dismiss", "Dismiss", &theme).on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.dismiss_timeline_report());
                        cx.notify();
                    }),
                ),
            )),
    )
    .into_any_element()
}
