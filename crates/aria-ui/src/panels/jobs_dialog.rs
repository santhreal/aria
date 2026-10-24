//! The jobs board: one row per background job, with progress and cancel,
//! drawn as the Jobs dialog and as the Jobs dock panel.
//!
//! Every row is drawn from a [`crate::app::jobs::UiJob`], so neither surface
//! reasons about lifecycle. Three distinctions the rows preserve, because each
//! was a way for a job surface to lie:
//!
//! - Determinate and indeterminate progress are different. A job reporting nothing gets
//!   no bar, not a bar stuck at zero.
//! - A terminal row stops animating. A finished job that still looks busy is worse than
//!   no row.
//! - Cancel appears only while the registry would accept it. Past the commit boundary
//!   the work finishes regardless, and a button that does nothing is a bug found
//!   only by pressing it.
//!
//! The dialog opens over the panel, so every element id a row builds includes
//! the surface it is drawn on: one job drawn twice is two rows, not one row
//! whose hover and press state the two copies share.

use gpui::{
    AnyElement, InteractiveElement, IntoElement, MouseButton, MouseDownEvent, ParentElement,
    SharedString, StatefulInteractiveElement, Styled, div, px,
};

use crate::app::jobs::UiJob;
use crate::app::{AppState, DialogKind};
use crate::chrome::dialog::{DialogBody, dialog_surface};
use crate::components;
use crate::motion;
use crate::theme::Theme;
use gpui::prelude::FluentBuilder;

/// The surface name of the dialog's rows.
const DIALOG: &str = "board";

/// The surface name of the dock panel's rows.
const PANEL: &str = "jobs-panel";

pub fn render(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> impl IntoElement {
    let theme = Theme::of(cx);
    let done = components::ghost_button("jobs-done", "Done", &theme).on_mouse_down(
        MouseButton::Left,
        cx.listener(|this, _: &MouseDownEvent, _, cx| {
            this.state
                .update(cx, |state, _| state.open_dialog(DialogKind::None));
            cx.notify();
        }),
    );
    let body = job_list(&theme, state, DIALOG, cx);
    dialog_surface(
        "jobs",
        DialogKind::Jobs,
        "Jobs",
        state,
        cx,
        DialogBody::new(body).action(done),
    )
}

/// Window > Jobs: the same rows as the dialog, filling a dock frame.
pub fn render_panel(
    state: &gpui::Entity<AppState>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let theme = Theme::of(cx);
    crate::edge_fade::scroll_y(
        div()
            .id("jobs-panel")
            .debug_selector(|| "jobs-panel".into())
            .size_full()
            .p(px(Theme::SPACE_SM))
            .child(job_list(&theme, state, PANEL, cx)),
    )
    .into_any_element()
}

/// Every job, newest first, or a line saying there are none.
fn job_list(
    theme: &Theme,
    state: &gpui::Entity<AppState>,
    surface: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::Div {
    // The handle: the board is a projection, rebuilt when a job reports.
    let rows = state.read(cx).job_view.clone();
    let list: Vec<_> = rows
        .iter()
        .map(|job| job_row(theme, job, surface, cx))
        .collect();
    div()
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_SM))
        .when(list.is_empty(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_faint)
                    .child("No background jobs."),
            )
        })
        .children(list)
}

fn job_row(
    theme: &Theme,
    job: &UiJob,
    surface: &'static str,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> gpui::AnyElement {
    let id = job.id;
    let status = SharedString::from(job.status_line());
    let detail = job.detail.clone();
    // A running job's status line carries a loader, so "Running · 12.4s" that
    // stopped ticking is distinguishable from one that is still working. A
    // terminal row stops animating: a finished job that still looks busy is
    // worse than no row at all.
    let status_cell = if job.terminal {
        div()
            .text_size(Theme::TEXT_UI)
            .text_color(theme.text_muted)
            .child(status)
            .into_any_element()
    } else {
        crate::loaders::pending_row(
            motion::MotionKey::new("job-status", id).with(surface),
            status,
            theme,
            cx.entity_id(),
            cx,
        )
        .into_any_element()
    };
    // Every live job gets the same bar: a reported fraction fills it, and a job
    // that cannot report one shuttles. The key is the job's, not this surface's,
    // so the board and a panel's own row draw one width for one job instead of
    // two that disagree. Built here because the bar holds the borrow of `cx` it
    // was made with until it is erased.
    let bar = (!job.terminal).then(|| {
        crate::loaders::progress_bar(
            motion::ControlId::Key(motion::MotionKey::new("job-bar", id)),
            job.progress,
            theme,
            cx.entity_id(),
            cx,
        )
        .into_any_element()
    });
    // The same control the panel rows draw, from the same owner: the board is
    // not a second place where a job's stoppability is decided.
    let stop = crate::pending::stop_button(surface, job, theme, cx);
    let key = motion::MotionKey::new("job-row", id).with(surface);
    // An output is a row plus, when it names a path, a way to get to it. Element ids
    // carry the job id: two rows sharing one id is a GPUI state collision, which shows
    // up as the wrong row's button reacting.
    let outputs: Vec<_> = job
        .outputs
        .iter()
        .map(|(name, value)| {
            let is_path = name == "path";
            let reveal_target = value.clone();
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(Theme::SPACE_XS))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text_faint)
                        .child(SharedString::from(format!("{name}: {value}"))),
                )
                .when(is_path, |el| {
                    el.child(
                        components::ghost_button(
                            motion::MotionKey::new("job-reveal", id).with(surface),
                            "Reveal",
                            theme,
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                let target = reveal_target.clone();
                                this.state.update(cx, |state, _| state.reveal_path(&target));
                                cx.notify();
                            }),
                        ),
                    )
                })
                .into_any_element()
        })
        .collect();

    div()
        .id(key.element_id("job-row"))
        .flex()
        .flex_col()
        .gap(px(Theme::SPACE_XS))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        .bg(motion::hover_blend(
            key,
            theme.surface_raised,
            theme.surface_raised_hover,
        ))
        .on_hover(motion::hover_listener(key))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(Theme::SPACE_SM))
                .child(
                    div()
                        .text_size(Theme::TEXT_UI)
                        .text_color(theme.text)
                        .child(job.label.clone()),
                )
                .child(status_cell),
        )
        .children(bar)
        .when(!job.detail.is_empty(), |el| {
            el.child(
                div()
                    .text_size(Theme::TEXT_UI)
                    .text_color(theme.text_muted)
                    .child(detail),
            )
        })
        .children(outputs)
        .children(stop)
        .into_any_element()
}
