//! Every choice in Project Settings reads inside its row.
//!
//! The class closed here: a choice label wider than its row, drawn past the
//! dialog card or cut mid-glyph with no ellipsis. The Canvas section holds the
//! longest lists in the dialog: the aspects, the canvas sizes, the frame rates,
//! the audio layouts, and the time displays. The labels are read from the same
//! registries and formats the section draws, so a new aspect, baseline, rate,
//! layout, or display is checked without an edit here.
//!
//! The card is `DIALOG_WIDTH` wide in a window at `MIN_WINDOW_WIDTH`, the
//! narrowest the window opens at. A window manager that ignores the minimum
//! hands the card less, so the sweep continues down to a 256 px window, where
//! the card is 192 px and the longest labels end in an ellipsis.
//!
//! Not caught: glyph shapes, which the test text system does not raster, and
//! the value rows (Size, Frame rate, Audio layout, Time display), which state
//! the current value rather than offer a choice.

use aria_models::{AudioLayout, CanvasAspect, TimeDisplay};
use gpui::{Bounds, Pixels, TestAppContext, px, size};

use crate::app::AppState;
use crate::settings::MIN_WINDOW_WIDTH;
use crate::surfaces::settings_pick::RATES;
use crate::test_support::{bounds_inside, drawn_shell};

/// Narrowest window the sweep draws, and the step down to it.
const NARROWEST: f32 = 256.0;
const STEP: f32 = 64.0;

/// Tall enough that every Canvas row is drawn without scrolling.
const TALL: f32 = 2400.0;

/// Characters a shortened label keeps before its ellipsis.
const LEAD: usize = 3;

/// Every choice label the Canvas section draws, in the section's order.
fn choices(state: &AppState) -> Vec<String> {
    let mut out: Vec<String> = CanvasAspect::ALL
        .into_iter()
        .map(|aspect| aspect.label().to_string())
        .collect();
    out.push(super::DERIVED_SIZE.to_string());
    out.extend(
        state
            .canvas_resolution_choices()
            .into_iter()
            .map(|(baseline, pixels)| super::size_label(baseline, pixels)),
    );
    out.extend(RATES.into_iter().map(super::format_fps));
    out.extend(
        AudioLayout::ALL
            .into_iter()
            .map(|layout| layout.label().to_string()),
    );
    out.extend(
        TimeDisplay::ALL
            .into_iter()
            .map(|display| display.label().to_string()),
    );
    out
}

/// Whether `inner` lies between `outer`'s left and right edges.
fn across(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    let slack = px(0.5);
    inner.left() >= outer.left() - slack && inner.right() <= outer.right() + slack
}

#[gpui::test]
fn every_canvas_choice_reads_whole_or_ends_in_an_ellipsis_inside_its_row(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "settings-choice-fit");
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_settings_at("Canvas");
            cx.notify();
        });
    });
    // The boot cover's wordmark lies over the card.
    crate::test_support::wait_out_splash(visual);
    let labels = visual.read(|app| choices(state.read(app)));
    let mut shortened = 0;
    let mut width = MIN_WINDOW_WIDTH;
    while width >= NARROWEST {
        visual.simulate_resize(size(px(width), px(TALL)));
        visual.run_until_parked();
        let card = visual
            .debug_bounds("dialog-card")
            .expect("Project Settings draws a card");
        let runs = visual.update(|window, _| window.rendered_text_runs().to_vec());
        for label in &labels {
            let row = visual
                .debug_bounds(crate::interned::intern(&format!("picker-label-{label}")))
                .unwrap_or_else(|| panic!("{width} px: Project Settings draws no `{label}`"));
            assert!(
                bounds_inside(row, card),
                "{width} px: the `{label}` label {row:?} runs out of the card {card:?}"
            );
            // The editor behind the scrim draws text under the card too, so
            // the label is the run under the row that reads as it.
            let lead: String = label.chars().take(LEAD).collect();
            let under: Vec<&gpui::TextRunLayout> = runs
                .iter()
                .filter(|run| row.contains(&run.bounds.center()))
                .collect();
            let text = under
                .iter()
                .find(|run| {
                    run.text.as_ref() == label.as_str()
                        || (run.text.ends_with('…') && run.text.starts_with(lead.as_str()))
                })
                .unwrap_or_else(|| {
                    let read: Vec<&str> = under.iter().map(|run| run.text.as_ref()).collect();
                    panic!(
                        "{width} px: the `{label}` row reads {read:?}, not the label or `{lead}…`"
                    )
                });
            let read = text.text.to_string();
            assert!(
                across(text.bounds, row),
                "{width} px: `{read}` at {:?} runs out of its row {row:?}",
                text.bounds
            );
            if read != *label {
                shortened += 1;
            }
        }
        width -= STEP;
    }
    assert!(
        shortened > 0,
        "no label was shortened down to a {NARROWEST} px window, so the ellipsis \
         was never read"
    );
}
