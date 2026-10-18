//! The Settings pickers stand on the section grid.
//!
//! The class closed here: a list of choices drawn at its own inset and pitch.
//! The Appearance choices were 30 px rows whose labels stood 8 px right of the
//! section title and of every other settings row's label.
//!
//! The Appearance choices are read with the Playback rows below them, so a
//! picker row and a labelled control row are held to one edge. Not covered:
//! the Canvas pickers, which need a project's canvas, and the glyphs.

use gpui::{TestAppContext, TextRunLayout, px};

use crate::app::DialogKind;
use crate::test_support::drawn_shell;
use crate::theme::Theme;

const EPS: f32 = 0.51;

/// The topmost run inside `card` that reads `text`.
fn run<'a>(
    runs: &'a [TextRunLayout],
    card: gpui::Bounds<gpui::Pixels>,
    text: &str,
) -> &'a TextRunLayout {
    runs.iter()
        .filter(|run| card.contains(&run.bounds.center()) && run.text.as_ref() == text)
        .min_by(|a, b| a.bounds.top().partial_cmp(&b.bounds.top()).unwrap())
        .unwrap_or_else(|| panic!("the Settings dialog draws `{text}`"))
}

#[gpui::test]
fn settings_pickers_stand_on_the_section_grid(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-rows");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(DialogKind::Settings);
            cx.notify();
        });
    });
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("the Settings dialog draws a card");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let title = run(&runs, card, "Appearance");
    let choices = ["System", "Light", "Dark"].map(|text| run(&runs, card, text));
    let control = run(&runs, card, "Audio scrubbing");
    for label in choices.iter().chain([&control]) {
        assert!(
            (label.bounds.left() - title.bounds.left()).abs() < px(EPS),
            "`{}` starts at {:?}, the section title at {:?}",
            label.text,
            label.bounds.left(),
            title.bounds.left()
        );
    }
    let pitch = px(Theme::ROW_HEIGHT + Theme::SPACE_XS);
    for pair in choices.windows(2) {
        let step = pair[1].bounds.top() - pair[0].bounds.top();
        assert!(
            (step - pitch).abs() < px(EPS),
            "`{}` follows `{}` by {step:?}, not one row and the section gap",
            pair[1].text,
            pair[0].text
        );
    }
}
