//! The project report: what the sequence carries.
//!
//! Two tables the model already answered and nothing drew:
//!
//! * the pool and sequence counts (`media_count`, `bin_count`, `marker_count`,
//!   `clip_count`, transitions, soloed lane kinds),
//! * one row per per-clip question, with how many clips answer it: a speed
//!   ramp put on one clip three hundred clips ago was findable only by
//!   scrubbing.
//!
//! Every row comes from a catalog: [`aria_models::project_facts`] and
//! [`aria_models::clip_facts`]. Nothing here names a fact or counts them, so a
//! row added to either table draws here with no edit. The counts are taken when
//! the report is opened and again on any edit that lands while it is open.
//! Drawing the report asks the session for nothing. The Info panel states what
//! the last frame cost.
//!
//! A clip-question row is a control, not a readout: clicking it selects and
//! seeks to the next clip that answers it, wrapping at the end, so repeated
//! presses walk every match. A row nothing answers is drawn dim and reads
//! `None` rather than being hidden: "no clip carries a speed ramp" is an answer.
//!
//! It is a dialog drawn on the shared card, titled and closed like the others.
//! The Window menu offers it once the project has a media source or a clip
//! (`project_report_available()`), so an empty project does not get a column
//! of zeroes.

use gpui::{
    CursorStyle, Entity, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::project_report::{ClipFactRow, ProjectFactRow};
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components::{self, readout, readout_tinted, section_label};
use crate::motion;
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

pub(crate) fn render(
    state: &Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let theme = Theme::of(cx);
    let facts = state.read(cx).project_fact_rows();
    let clip_facts = state.read(cx).clip_fact_rows();
    let drawn = state.read(cx).selected_content_size();

    let content = div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .child(section_label(&theme, "Project").pt_0())
        .children(facts.iter().map(|row| fact_row(&theme, row)))
        .child(section_label(&theme, "Clips carrying"))
        .children(
            clip_facts
                .iter()
                .copied()
                .map(|row| clip_fact_row(&theme, row, cx)),
        )
        // Only a generated clip has content to measure, so the section appears
        // with the selection rather than sitting empty beside a media clip.
        .when_some(drawn, |content, (width, height)| {
            content
                .child(section_label(&theme, "Selected content"))
                .child(readout(
                    &theme,
                    "Drawn size",
                    format!("{width}×{height} px"),
                ))
        });
    let close = components::button("project-report-close", "Close", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let body = DialogBody::new(content).action(close);
    dialog_surface(
        "project-report",
        DialogKind::ProjectReport,
        "Project Report",
        state,
        cx,
        body,
    )
}

fn fact_row(theme: &Theme, row: &ProjectFactRow) -> gpui::Div {
    let tint = if row.empty {
        theme.text_faint
    } else {
        theme.text
    };
    readout_tinted(theme, row.label, row.value.clone(), tint)
}

/// One per-clip question. Clicking walks to the next clip that answers it.
///
/// The hover wash runs `SPACE_XS` past the column on both sides and
/// `SPACE_XXS` past the row above and below it, so the label and the count stay
/// on the same edges as the project rows above them and every row of the card
/// keeps the same pitch.
fn clip_fact_row(
    theme: &Theme,
    row: ClipFactRow,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let key = motion::MotionKey::new("project-report", row.id);
    let empty = row.count == 0;
    let fact_id = row.id;
    let (value, tint) = if empty {
        (SharedString::new_static("None"), theme.text_faint)
    } else {
        (SharedString::from(row.count.to_string()), theme.accent)
    };
    readout_tinted(theme, row.label, value, tint)
        .id(motion::MotionKey::new("fact", row.id).element_id("fact"))
        .mx(px(-Theme::SPACE_XS))
        .px(px(Theme::SPACE_XS))
        .my(px(-Theme::SPACE_XXS))
        .py(px(Theme::SPACE_XXS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            key,
            theme.wash(0.0),
            theme.element_hover,
        ))
        .when(!empty, |el| {
            el.cursor(CursorStyle::PointingHand)
                .active(|style| style.opacity(0.92))
                .on_hover(motion::hover_listener(key))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                        this.state
                            .update(cx, |state, _| state.go_to_clip_with_fact(fact_id));
                        cx.notify();
                    }),
                )
        })
        .into_any_element()
}

#[cfg(test)]
#[path = "project_report_tests.rs"]
mod tests;
