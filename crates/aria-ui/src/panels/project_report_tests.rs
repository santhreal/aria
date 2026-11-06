//! The Project Report's rows stand at one pitch, and a count reads its digit.
//!
//! The classes closed here:
//!
//! * A row that pads itself for its hover wash and so stands taller than the
//!   rows around it. The clip rows padded their wash by `SPACE_XXS` above and
//!   below, which the project rows above them do not, so the card's rows
//!   changed pitch at the section break.
//! * A count that reads two ways at zero. The project counts read `0` and the
//!   clip questions read `None`, so one card answered the same question in two
//!   words.
//!
//! Every row is read from the frame with a still placed and selected, so a row
//! added to either catalog joins both checks without an edit here.
//!
//! Not covered: the Selected content section, a row whose label or value
//! wraps, and the wash, which is not a text run.

use gpui::{Bounds, Pixels, TestAppContext, TextRunLayout, px};

use crate::app::DialogKind;
use crate::app::project_report::ClipFactRow;
use crate::test_support::{drawn_shell, select_a_still};

const EPS: f32 = 0.51;

/// The report as drawn with a still placed and selected: the labels of each
/// section in catalog order, the clip questions, the card, and the runs drawn
/// on or below the card's top. The content scrolls and a run is recorded
/// unclipped, so a row below the card's edge is still the card's.
struct Drawn {
    sections: [Vec<&'static str>; 2],
    clip_facts: Vec<ClipFactRow>,
    card: Bounds<Pixels>,
    runs: Vec<TextRunLayout>,
}

fn drawn_report(cx: &mut TestAppContext, name: &str) -> Drawn {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, name);
    visual.update(|_, app| app.set_reduce_motion(true));
    let (sections, clip_facts) = visual.update(|_, app| {
        state.update(app, |state, cx| {
            select_a_still(state);
            state.open_dialog(DialogKind::ProjectReport);
            cx.notify();
            let clip_facts = state.clip_fact_rows().to_vec();
            (
                [
                    state
                        .project_fact_rows()
                        .iter()
                        .map(|row| row.label)
                        .collect(),
                    clip_facts.iter().map(|row| row.label).collect(),
                ],
                clip_facts,
            )
        })
    });
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Project Report draws a card");
    let runs = visual.update(|window, _| {
        window
            .rendered_text_runs()
            .iter()
            .filter(|run| {
                let center = run.bounds.center();
                center.x >= card.left() && center.x <= card.right() && center.y >= card.top()
            })
            .cloned()
            .collect()
    });
    Drawn {
        sections,
        clip_facts,
        card,
        runs,
    }
}

/// The topmost run reading `label` below `below`.
fn label_run<'a>(runs: &'a [TextRunLayout], label: &str, below: Pixels) -> &'a TextRunLayout {
    runs.iter()
        .filter(|run| run.text.as_ref() == label && run.bounds.top() > below)
        .min_by(|a, b| f32::from(a.bounds.top()).total_cmp(&f32::from(b.bounds.top())))
        .unwrap_or_else(|| panic!("the report draws the row {label:?}"))
}

#[gpui::test]
fn every_row_of_the_project_report_stands_at_one_pitch(cx: &mut TestAppContext) {
    let drawn = drawn_report(cx, "project-report-pitch");
    let mut pitch = None;
    let mut below = drawn.card.top();
    for labels in &drawn.sections {
        assert!(
            labels.len() >= 2,
            "a section of the report draws one row or none: {labels:?}"
        );
        let mut tops = Vec::with_capacity(labels.len());
        for label in labels {
            below = label_run(&drawn.runs, label, below).bounds.top();
            tops.push((*label, below));
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

#[gpui::test]
fn every_clip_question_reads_its_count_as_a_digit(cx: &mut TestAppContext) {
    let drawn = drawn_report(cx, "project-report-counts");
    assert!(
        drawn.clip_facts.iter().any(|row| row.count == 0),
        "a selected still answers every clip question, so no row reads zero: {:?}",
        drawn.clip_facts
    );
    let mut below = drawn.card.top();
    for row in &drawn.clip_facts {
        let label = label_run(&drawn.runs, row.label, below);
        below = label.bounds.top();
        let line = label.bounds.center().y;
        // The value is right-aligned in the card. Runs of the panels behind
        // the dialog share the card's lines, so the value is the rightmost run
        // on the label's line rather than every run right of the label.
        let value = drawn
            .runs
            .iter()
            .filter(|run| {
                run.bounds.left() > label.bounds.right()
                    && (run.bounds.center().y - line).abs() < px(EPS)
            })
            .max_by(|a, b| f32::from(a.bounds.right()).total_cmp(&f32::from(b.bounds.right())))
            .map(|run| run.text.as_ref())
            .unwrap_or_else(|| panic!("{:?} draws no value", row.label));
        assert_eq!(
            value,
            row.count.to_string(),
            "{:?} answers {} clips and reads {value:?}",
            row.label,
            row.count
        );
    }
}
