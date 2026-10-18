//! The export sheet's rows are one grid.
//!
//! The class closed here: two row shapes on one sheet. The read-only rows
//! (Encoder, Canvas, Destination) and the clickable rows (Format through
//! Range) were built apart, so the clickable labels sat four pixels right of
//! the others and the two kinds stood at different pitches.
//!
//! Every row the sheet draws is found by its label, so a row added to the
//! sheet joins the check without an edit here. Not covered: the in-flight
//! render rows, which are drawn by the loaders and only while a job runs.

use gpui::{TestAppContext, px};

use crate::app::DialogKind;
use crate::interned::intern;
use crate::test_support::drawn_shell;
use crate::theme::Theme;

const LABELS: [&str; 11] = [
    "Encoder",
    "Canvas",
    "Format",
    "Scale",
    "Frame rate",
    "Codec",
    "Bitrate",
    "Keyframes",
    "Audio",
    "Range",
    "Destination",
];

const EPS: f32 = 0.51;

#[gpui::test]
fn every_export_row_stands_on_one_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "export-rows");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Export);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the export sheet draws a card");
    let mut lines: Vec<(&str, gpui::Bounds<gpui::Pixels>)> = LABELS
        .iter()
        .filter_map(|&label| {
            visual
                .debug_bounds(intern(&format!("export-line-{label}")))
                .map(|bounds| (label, bounds))
        })
        .collect();
    assert!(
        lines.len() >= 7,
        "the sheet draws its rows, found {:?}",
        lines.iter().map(|(label, _)| *label).collect::<Vec<_>>()
    );
    lines.sort_by(|a, b| a.1.top().partial_cmp(&b.1.top()).unwrap());
    let (first_label, first) = lines[0];
    for (label, line) in &lines {
        assert!(
            (f32::from(line.size.height) - Theme::ROW_HEIGHT).abs() < EPS,
            "{label} is {:?} tall, not one row",
            line.size.height
        );
        assert!(
            (line.left() - first.left()).abs() < px(EPS)
                && (line.right() - first.right()).abs() < px(EPS),
            "{label} spans {:?}..{:?}, {first_label} {:?}..{:?}",
            line.left(),
            line.right(),
            first.left(),
            first.right()
        );
    }
    let pitch = lines[1].1.top() - lines[0].1.top();
    for pair in lines.windows(2) {
        let step = pair[1].1.top() - pair[0].1.top();
        assert!(
            (step - pitch).abs() < px(EPS),
            "{} follows {} by {step:?}, the sheet's pitch is {pitch:?}",
            pair[1].0,
            pair[0].0
        );
    }
    // The labels start where the sheet's title starts.
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let inside = |run: &&gpui::TextRunLayout| card.contains(&run.bounds.center());
    let title = runs
        .iter()
        .filter(inside)
        .filter(|run| run.text.as_ref() == "Export")
        .min_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap())
        .expect("the sheet draws its title");
    for (label, _) in &lines {
        let run = runs
            .iter()
            .filter(inside)
            .find(|run| run.text.as_ref() == *label)
            .unwrap_or_else(|| panic!("the sheet draws the label {label}"));
        assert!(
            (run.bounds.left() - title.bounds.left()).abs() < px(EPS),
            "{label} starts at {:?}, the title at {:?}",
            run.bounds.left(),
            title.bounds.left()
        );
    }
}
