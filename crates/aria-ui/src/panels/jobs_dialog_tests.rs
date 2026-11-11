//! Every control a jobs surface draws, on the Jobs dialog and on the Jobs
//! panel, is as wide as its label and its padding.
//!
//! WHY: a board row laid its Stop button in the row's column, under the
//! progress bar, so the button stretched across the row and drew as an empty
//! text field with a word in its corner. The job rows of the Analysis, Captions
//! and Generate panels draw the same button at the end of a line, as wide as
//! the word. Closed here for every
//! ghost button and chip either jobs surface draws while a stoppable job runs,
//! measured against the advance of its label, and the test fails when either
//! surface draws no Stop.
//!
//! Not covered: a finished job's Reveal button, drawn only for a job with a
//! path output, beside its output line.

use gpui::{Bounds, FontWeight, Pixels, TestAppContext, VisualTestContext};

use aria_jobs::JobCompletion;

use crate::app::DialogKind;
use crate::components::noted::{Drawn, NotingControls, drawn_controls, record_controls};
use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::theme::Theme;

/// Record the controls of a fresh frame, then assert each one drawn inside
/// `surface` is no wider than its label and padding, or a switch its track.
/// A text element is as wide as its advance rounded up to a whole pixel, and
/// layout rounds each edge to a device pixel. Returns the labels checked.
fn hugging_labels(
    visual: &mut VisualTestContext,
    surface: Bounds<Pixels>,
    scope: &str,
) -> Vec<String> {
    let (controls, _) = drawn_controls();
    let (pixel, hugs): (f32, Vec<f32>) = visual.update(|window, app| {
        let font = Theme::of(app).ui_font(FontWeight::NORMAL);
        let hugs = controls
            .iter()
            .map(|control| match &control.drawn {
                Drawn::Labelled { label, inset, size } => {
                    f32::from(app.text_system().advance_for_text(label, &font, *size)).ceil()
                        + 2.0 * inset
                }
                Drawn::Switch => crate::components::SWITCH_WIDTH,
            })
            .collect();
        (1.0 / window.scale_factor(), hugs)
    });
    let mut checked = Vec::new();
    for (control, hug) in controls.iter().zip(hugs) {
        let Some(bounds) = visual.debug_bounds(intern(&control.selector)) else {
            continue;
        };
        if !surface.contains(&bounds.center()) {
            continue;
        }
        let label = match &control.drawn {
            Drawn::Labelled { label, .. } => label.to_string(),
            Drawn::Switch => control.selector.clone(),
        };
        let width = f32::from(bounds.size.width);
        assert!(
            width <= hug + pixel,
            "{scope}: the control {label:?} ({}) is {width} wide, past the {hug} its label and padding take",
            control.selector
        );
        checked.push(label);
    }
    checked
}

/// Draw a fresh frame of the shell while recording its controls.
fn record_frame(visual: &mut VisualTestContext) {
    record_controls();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
}

#[gpui::test]
fn every_jobs_control_hugs_its_label_on_the_dialog_and_the_panel(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let _noting = NotingControls::start();
    let (state, visual) = drawn_shell(cx, "jobs-hug");
    let (release, wait) = crossbeam_channel::bounded::<()>(1);
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.spawn_job("Exporting held.mp4", move |_| {
                let _ = wait.recv();
                Ok(JobCompletion::new("done"))
            });
            state.open_dialog(DialogKind::Jobs);
            cx.notify();
        })
    });
    visual.run_until_parked();

    record_frame(visual);
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Jobs dialog is not drawn");
    let board = hugging_labels(visual, card, "Jobs dialog");
    assert!(
        board.iter().any(|label| label == "Stop"),
        "the Jobs dialog draws no Stop for a running job: {board:?}"
    );

    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::None);
            state.focus_dock_panel(crate::dock::PANEL_JOBS);
            cx.notify();
        })
    });
    visual.run_until_parked();
    record_frame(visual);
    let panel = visual
        .debug_bounds("jobs-panel")
        .expect("the Jobs panel is not drawn");
    let rows = hugging_labels(visual, panel, "Jobs panel");
    assert!(
        rows.iter().any(|label| label == "Stop"),
        "the Jobs panel draws no Stop for a running job: {rows:?}"
    );
    release.send(()).expect("let the job finish");
}
