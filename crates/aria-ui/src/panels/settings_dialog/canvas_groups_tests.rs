//! Each Canvas choice group stands under the row that reads its value.
//!
//! The class closed here: two single-choice groups drawn back to back, which
//! read as one list with two rows lit. The aspect presets stood under the Size
//! row with the canvas sizes after them, so Match footage and Derived were lit
//! in one list of a dozen rows. The choices are read from the registries and
//! formats the section draws, so a new aspect, baseline, rate, layout or
//! display is placed without an edit here.
//!
//! Not caught: the hairline between groups, which the test renderer does not
//! raster, and a sixth group, whose value row needs listing in [`groups`].

use aria_models::{AudioLayout, CanvasAspect, TimeDisplay};
use gpui::{TestAppContext, px, size};

use crate::app::AppState;
use crate::app::settings_form::SettingsSection;
use crate::settings::MIN_WINDOW_WIDTH;
use crate::surfaces::settings_pick::RATES;
use crate::test_support::drawn_shell;

/// Tall enough that every Canvas row is drawn without scrolling.
const TALL: f32 = 2400.0;

/// Each group's value row label and its choices, in the section's order.
fn groups(state: &AppState) -> Vec<(&'static str, Vec<String>)> {
    let sizes = std::iter::once(super::DERIVED_SIZE.to_string()).chain(
        state
            .canvas_resolution_choices()
            .into_iter()
            .map(|(baseline, pixels)| super::size_label(baseline, pixels)),
    );
    vec![
        (
            "Aspect",
            CanvasAspect::ALL
                .into_iter()
                .map(|aspect| aspect.label().to_string())
                .collect(),
        ),
        ("Size", sizes.collect()),
        (
            "Frame rate",
            RATES.into_iter().map(super::format_fps).collect(),
        ),
        (
            "Audio layout",
            AudioLayout::ALL
                .into_iter()
                .map(|layout| layout.label().to_string())
                .collect(),
        ),
        (
            "Time display",
            TimeDisplay::ALL
                .into_iter()
                .map(|display| display.label().to_string())
                .collect(),
        ),
    ]
}

#[gpui::test]
fn every_canvas_choice_stands_under_its_own_groups_value_row(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-canvas-groups");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_settings_at(SettingsSection::Canvas);
            cx.notify();
        });
    });
    // The boot cover's wordmark lies over the card.
    crate::test_support::wait_out_splash(visual);
    visual.simulate_resize(size(px(MIN_WINDOW_WIDTH), px(TALL)));
    visual.run_until_parked();
    let card = visual
        .debug_bounds("dialog-card")
        .expect("Project Settings draws a card");
    let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
    let groups = visual.read(|app| groups(state.read(app)));

    // The editor behind the scrim draws text too, so a value row is the one
    // run inside the card that reads its label.
    let headers: Vec<gpui::Pixels> = groups
        .iter()
        .map(|(header, _)| {
            let tops: Vec<gpui::Pixels> = runs
                .iter()
                .filter(|run| card.contains(&run.bounds.center()) && run.text.as_ref() == *header)
                .map(|run| run.bounds.top())
                .collect();
            assert_eq!(
                tops.len(),
                1,
                "the Canvas card reads `{header}` {} times, not once as a value row",
                tops.len()
            );
            tops[0]
        })
        .collect();
    for pair in headers.windows(2) {
        assert!(
            pair[0] < pair[1],
            "the value rows stand out of order: {headers:?}"
        );
    }

    for (index, (header, choices)) in groups.iter().enumerate() {
        let below = headers[index];
        let above = headers.get(index + 1).copied();
        for choice in choices {
            let row = visual
                .debug_bounds(crate::interned::intern(&format!("picker-label-{choice}")))
                .unwrap_or_else(|| panic!("the Canvas card draws no `{choice}`"));
            let top = row.top();
            assert!(
                top > below && above.is_none_or(|next| top < next),
                "`{choice}` stands at {top:?}, outside the `{header}` group between \
                 {below:?} and {above:?}"
            );
        }
    }
}
