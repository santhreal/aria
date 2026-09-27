//! How a surface arrives: the wait, and the thing that replaces it.
//!
//! Five surfaces start background work — analysis, captions, generation, the
//! cloud feeds, and the scope measurement — and each drew its own static
//! "Running · label" box. A static box is indistinguishable from a stalled
//! one, which is the whole complaint: a wait has to look like a wait. So a row
//! carries a loader while the work is live, and resolves into its settled form
//! through a fade rather than swapping in place.
//!
//! The rows come from the job projection, never from a list a panel keeps: a
//! panel says which jobs are its own and how many to show, and what a row
//! looks like is decided once, here. A new kind of job needs nothing from this
//! file.
//!
//! [`arriving`] is the same idea for a list that was fetched rather than
//! computed — a bin, a catalog, a marker list. The rows fade up in their own
//! order instead of all at once, which is the difference between a list that
//! arrived and a list that blinked.

use gpui::{
    AnimationExt, AnyElement, App, Entity, InteractiveElement, IntoElement, MouseButton,
    MouseDownEvent, ParentElement, SharedString, Styled, div, px,
};

use crate::app::AppState;
use crate::app::jobs::UiJob;
use crate::loaders;
use crate::motion;
use crate::theme::Theme;

/// Cell size of the per-row spinner. Sized to sit on one line of 11px text
/// without changing the row's height.
const ROW_CELL: f32 = 3.0;

/// The rows for the jobs a panel owns, newest first.
///
/// `mine` is the panel's claim on the shared job list and `limit` is how much
/// of the panel it is willing to spend; everything else about the row is the
/// same everywhere, which is why it is not a parameter.
pub fn job_rows(
    theme: &Theme,
    state: &Entity<AppState>,
    limit: usize,
    mine: fn(&UiJob) -> bool,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Vec<AnyElement> {
    // The handle, not the board: the projection is rebuilt when a job reports,
    // not when a frame draws.
    let jobs = state.read(cx).job_view.clone();
    jobs.iter()
        .filter(|job| mine(job))
        .take(limit)
        .map(|job| job_row(theme, job, cx))
        .collect()
}

/// Whether any job a panel owns is still running.
///
/// Panels use this to decide whether to draw a panel-level wait beside their
/// own controls, so the answer is derived from the same projection the rows
/// are, not from a flag a panel maintains beside it.
pub fn any_live(state: &Entity<AppState>, mine: fn(&UiJob) -> bool, cx: &App) -> bool {
    state
        .read(cx)
        .job_view
        .iter()
        .any(|job| mine(job) && !job.terminal)
}

/// The identity of a row's stop control.
///
/// The surface is part of the identity because the jobs board opens *over* the
/// panel that owns the same job: two live rows for one job would otherwise share
/// one element id, and GPUI keys hover and press state by id, so the pointer
/// over one would light the other.
pub(crate) fn stop_id(surface: &str, job: u64) -> motion::ControlId {
    motion::ControlId::Key(motion::MotionKey::new("job-stop", surface).with(job))
}

/// The control that stops a job, and nothing at all for a job that cannot be
/// stopped.
///
/// One owner, so every surface that shows work in flight offers the same word in
/// the same place: a job the operator can watch is a job the operator can stop.
/// Whether it can be is [`UiJob::stoppable`], which is the job's business rather
/// than each surface's, and the registry has the final say — a stop that arrives
/// after the commit boundary is refused and says so.
pub fn stop_button(
    surface: &'static str,
    job: &UiJob,
    theme: &Theme,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Option<AnyElement> {
    if !job.stoppable() {
        return None;
    }
    let id = job.id;
    Some(
        crate::components::ghost_button(stop_id(surface, id), "Stop", theme)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                    this.state.update(cx, |state, _| state.cancel_job(id));
                    cx.notify();
                }),
            )
            .into_any_element(),
    )
}

/// One job.
///
/// A live job leads with a spinner; a settled one leads with a mark in the
/// spinner's place, so the text does not shift sideways when the work ends.
/// The liveness is part of the element id, which is what makes the change a
/// fade: a new id starts the animation over, and the settled row arrives at
/// full opacity instead of appearing at it.
pub fn job_row(
    theme: &Theme,
    job: &UiJob,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let live = !job.terminal;
    let head = SharedString::from(format!("{} · {}", job.status, job.label));
    let detail = job.detail.clone();
    let leader = if live {
        loaders::mini_gradient_spinner(
            motion::MotionKey::new("job-spin", job.id),
            ROW_CELL,
            cx.entity_id(),
            cx,
        )
        .into_any_element()
    } else {
        div()
            .w(px(ROW_CELL * 3.0))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .text_color(theme.text_faint)
            .child("·")
            .into_any_element()
    };

    // A running job carries a bar: the status word and the detail line both stop
    // changing between reports, so a job that has been writing the same frame
    // count for a minute looks the same as one that has stopped. A settled row
    // has nowhere to get to. Built here rather than in the chain below, because
    // the bar holds the borrow of `cx` it was made with until it is erased.
    let bar = live.then(|| {
        loaders::progress_bar(
            motion::ControlId::Key(motion::MotionKey::new("job-bar", job.id)),
            job.progress,
            theme,
            cx.entity_id(),
            cx,
        )
        .into_any_element()
    });

    // A job the operator can watch is a job the operator can stop, on the same
    // row rather than only on the board: hoisted for the same reason the bar is.
    let stop = stop_button("panel", job, theme, cx);

    let row = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(Theme::SPACE_SM))
        .px(px(Theme::SPACE_SM))
        .py(px(Theme::SPACE_XS))
        .rounded(px(Theme::CONTROL_RADIUS))
        // A settled row keeps saying how it ended after the spinner is gone: the
        // status word is three characters wide and a list of them scans as a
        // block, so the fill carries it too.
        .bg(match (live, job.status) {
            (false, "Done") => theme.success_muted,
            (false, "Failed" | "Cancelled") => theme.danger_muted,
            _ => theme.surface_raised,
        })
        .child(leader)
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(Theme::SPACE_XXS))
                .flex_1()
                .min_w_0()
                .child(div().text_size(px(11.)).text_color(theme.text).child(head))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(theme.text_muted)
                        .child(detail),
                )
                .children(bar),
        )
        .children(stop);

    motion::fade_quick(
        crate::motion::MotionKey::new(if live { "job-row-live" } else { "job-row-done" }, job.id),
        row,
    )
    .into_any_element()
}

/// A wait with no job row of its own: a feed being fetched, a frame being
/// measured, a model answering.
///
/// `key` names the wait so two of them on one panel do not share an animation,
/// so the loader restarts when the panel starts waiting on something else, and
/// so the wait can be aged — a fetch that answers immediately shows nothing.
///
/// The same row every other wait in the app draws. It used to be the pulsing
/// block instead, which made a panel-level wait a second thing to learn: the
/// block said "busy" and the row said "busy", at two rates, in two shapes.
pub fn working(
    theme: &Theme,
    key: &str,
    line: impl Into<SharedString>,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> AnyElement {
    let view = cx.entity_id();
    let row = loaders::pending_row(
        motion::MotionKey::new("pending-working", key),
        line,
        theme,
        view,
        cx,
    );
    motion::fade_quick(
        crate::motion::MotionKey::new("pending-working", key),
        div().child(row),
    )
    .into_any_element()
}

/// What a wait resolves into.
///
/// The settled content fades up rather than replacing the loader in one frame.
/// `key` must change when the content does, or the second answer arrives
/// without the fade the first one got.
pub fn settled<E>(key: impl Into<motion::ControlId>, element: E) -> AnyElement
where
    E: Styled + IntoElement + 'static,
{
    motion::fade_in(key, element).into_any_element()
}

/// The largest place in the queue that still buys a delay.
///
/// A bin of four hundred clips must not have its last row arrive seven seconds
/// late; past a dozen rows the stagger has already said everything it has to
/// say, and the rest may as well come with the twelfth.
const STAGGER_DEPTH: u64 = 12;

/// One row of a list that has just appeared.
///
/// The index is the row's place in the list, and the delay it buys is the
/// whole effect: rows arrive in order rather than at once, which is what makes
/// a list read as a list instead of a flash. The key must be per row — one id
/// shared across rows is one animation they would all sit inside, and they
/// would move together.
///
/// The row slides in horizontally rather than vertically, which leaves `top`
/// to the hover lift a list row usually also wants. Two animations writing the
/// same style property is one animation, and the loser is whichever ran first.
pub fn arriving<E>(key: impl Into<motion::ControlId>, index: usize, element: E) -> AnyElement
where
    E: Styled + IntoElement + 'static,
{
    let spec = gpui::motion::presets::FADE_IN
        .with_delay(motion::ROW_STAGGER_MS * (index as u64).min(STAGGER_DEPTH));
    element
        .with_animation(
            key.into().element("row-in"),
            motion::animation(spec),
            |el, t| el.relative().opacity(t).left(px(-8.0 * (1.0 - t))),
        )
        .into_any_element()
}

#[cfg(test)]
#[path = "pending_tests.rs"]
mod tests;
