//! The Project Report's rows stand at one pitch.
//!
//! The class closed here: a row that pads itself for its hover wash and so
//! stands taller than the rows around it. The clip rows padded their wash by
//! `SPACE_XXS` above and below, which the project rows above them do not, so
//! the card's rows changed pitch at the section break. The label of every
//! project row and every clip row is read from the frame with a still placed
//! and selected, so a row added to either catalog joins the check without an
//! edit here, and the distance from each row to the next in a section must be
//! one distance across both sections.
//!
//! Not covered: the Selected content section, a row whose label or value
//! wraps, and the wash, which is not a text run.

use gpui::{TestAppContext, px};

use crate::app::DialogKind;
use crate::test_support::{drawn_shell, select_a_still};

const EPS: f32 = 0.51;

#[gpui::test]
fn every_row_of_the_project_report_stands_at_one_pitch(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "project-report-pitch");
    visual.update(|_, app| app.set_reduce_motion(true));
    let sections: [Vec<&'static str>; 2] = visual.update(|_, app| {
        state.update(app, |state, cx| {
            select_a_still(state);
            state.open_dialog(DialogKind::ProjectReport);
            cx.notify();
            [
                state
                    .project_fact_rows()
                    .iter()
                    .map(|row| row.label)
                    .collect(),
                state.clip_fact_rows().iter().map(|row| row.label).collect(),
            ]
        })
    });
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Project Report draws a card");
    // The content scrolls, and a run is recorded unclipped, so a row below the
    // card's edge is still the card's.
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let inside = |run: &&gpui::TextRunLayout| {
        let center = run.bounds.center();
        center.x >= card.left() && center.x <= card.right() && center.y >= card.top()
    };
    let mut pitch = None;
    let mut below = card.top();
    for labels in &sections {
        assert!(
            labels.len() >= 2,
            "a section of the report draws one row or none: {labels:?}"
        );
        let mut tops = Vec::with_capacity(labels.len());
        for label in labels {
            let run = runs
                .iter()
                .filter(inside)
                .filter(|run| run.text.as_ref() == *label && run.bounds.top() > below)
                .min_by(|a, b| f32::from(a.bounds.top()).total_cmp(&f32::from(b.bounds.top())))
                .unwrap_or_else(|| panic!("the report draws the row {label:?}"));
            below = run.bounds.top();
            tops.push((*label, run.bounds.top()));
        }
        for pair in tops.windows(2) {
            let step = pair[1].1 - pair[0].1;
            let pitch = *pitch.get_or_insert(step);
            assert!(
                (step - pitch).abs() < px(EPS),
                "{:?} follows {:?} by {step:?}, the report's pitch is {pitch:?}",
                pair[1].0,
                pair[0].0
            );
        }
    }
}
