//! Every dialog card fits the window it is drawn in, every dialog title is in
//! Title Case, and every line a dialog draws starts in sentence case.
//!
//! The class closed here: a card as tall as its body. A body taller than the
//! window put the title, the header controls and the action row outside the
//! window, where no pointer reaches them and no scroll brings them back.
//!
//! For every dialog kind the overlay registry holds, drawn in a window smaller
//! than a card in both directions, the card lies inside the window, and the
//! header, the scrolling content and the action row lie inside the card.
//!
//! The kinds are read from the registry at run time, and every one of them
//! draws a card through [`super::dialog_surface`].
//!
//! Not covered: dialogs registered by a plugin outside this crate, pixels, and
//! which row the content region is scrolled to.

use std::collections::HashSet;

use gpui::{Bounds, Pixels, Size, TestAppContext, VisualTestContext, point, px, size};

use crate::app::DialogKind;
use crate::app::status::lowercase_start;
use crate::commands::casing_tests::check_title_case;
use crate::plugins::overlay_host::host_of;
use crate::test_support::{bounds_inside, drawn_shell};

/// A window narrower and shorter than a dialog card.
const SHORT: Size<Pixels> = size(px(480.), px(320.));

#[gpui::test]
fn every_dialog_card_fits_a_short_window(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-fit");
    visual.simulate_resize(SHORT);
    visual.run_until_parked();
    let window = Bounds::new(
        point(px(0.), px(0.)),
        visual.update(|window, _| window.viewport_size()),
    );
    assert_eq!(window.size, SHORT, "the window did not take the short size");

    let kinds = registered(&state, visual);
    assert!(
        kinds.len() >= 7,
        "the shipped dialogs did not register: {kinds:?}"
    );

    let mut offenders = Vec::new();
    for kind in kinds {
        open(&state, visual, kind);
        offenders.extend(misplaced(kind, window, visual));
    }
    assert!(
        offenders.is_empty(),
        "drawn in a {:?} window:\n  {}",
        window.size,
        offenders.join("\n  ")
    );
}

/// Every dialog title is in Title Case, as the menu rows and palette entries
/// that open the dialogs are.
///
/// The class closed here: a title in sentence case beside Title Case ones, as
/// `Command palette`, `Unsaved changes` and `Install plugin` were. The title is
/// read from the frame each registered kind draws, so a kind registered later
/// is checked on the run that adds it. The Install Plugin dialog's other title,
/// for an update, is checked by `panels::install_dialog::tests`.
#[gpui::test]
fn every_dialog_title_is_in_title_case(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-titles");
    visual.update(|_, app| app.set_reduce_motion(true));
    let mut titles = Vec::new();
    let mut offenders = Vec::new();
    for kind in registered(&state, visual) {
        open(&state, visual, kind);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let Some(bounds) = visual.debug_bounds("dialog-title") else {
            offenders.push(format!("{kind:?} drew no dialog-title"));
            continue;
        };
        let title: String = visual.update(|window, _| {
            let mut runs: Vec<_> = window
                .rendered_text_runs()
                .iter()
                .filter(|run| bounds.contains(&run.bounds.center()))
                .map(|run| (f32::from(run.bounds.left()), run.text.to_string()))
                .collect();
            runs.sort_by(|a, b| a.0.total_cmp(&b.0));
            runs.into_iter().map(|(_, text)| text).collect()
        });
        if let Err(reason) = check_title_case(&title) {
            offenders.push(format!("{kind:?}: {reason}"));
        }
        titles.push(title);
    }
    assert!(titles.len() >= 7, "the walk read only {titles:?}");
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// Every line a dialog draws starts in sentence case, as the buttons and the
/// prose beside it do.
///
/// The class closed here: a value or a description that starts in lower case
/// among capitalised ones, as the Export sheet's `included`, the motion rows'
/// `every transition at its designed speed`, the Keys section's `unbound` and
/// the Project Report's `none` were. Every run drawn inside the card of each
/// registered kind is read from the frame, so a row added later is checked on
/// the run that adds it. A run the editor behind the card draws at the same
/// place is the editor's, and is skipped. One binding is cleared before the
/// walk, so the Keys section draws a row with no chord.
///
/// Not covered: a line a dialog draws only after a choice changes or a job
/// reports, beyond the cleared binding, and a lower case word inside a line.
#[gpui::test]
fn every_line_a_dialog_draws_starts_in_sentence_case(cx: &mut TestAppContext) {
    let (state, visual) = drawn_shell(cx, "dialog-casing");
    visual.update(|_, app| app.set_reduce_motion(true));
    visual.update(|_, app| {
        state.update(app, |state, _| {
            let Some(first) = state.binding_rows().first().map(|row| row.binding.id) else {
                panic!("the registry holds no binding to clear");
            };
            state.clear_binding(first);
        })
    });
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    let behind: HashSet<(String, u32, u32)> =
        visual.update(|window, _| window.rendered_text_runs().iter().map(placed).collect());
    let mut read = Vec::new();
    let mut offenders = Vec::new();
    for kind in registered(&state, visual) {
        open(&state, visual, kind);
        visual.update(|window, _| window.refresh());
        visual.run_until_parked();
        let Some(card) = visual.debug_bounds("dialog-card") else {
            offenders.push(format!("{kind:?} drew no dialog-card"));
            continue;
        };
        // The content scrolls, and a run is recorded unclipped, so a line below
        // the card's edge is still the card's.
        let lines: Vec<String> = visual.update(|window, _| {
            window
                .rendered_text_runs()
                .iter()
                .filter(|run| {
                    let center = run.bounds.center();
                    center.x >= card.left() && center.x <= card.right() && center.y >= card.top()
                })
                .filter(|run| !behind.contains(&placed(run)))
                .map(|run| run.text.to_string())
                .collect()
        });
        if lines.is_empty() {
            offenders.push(format!("{kind:?} drew no line"));
        }
        for line in &lines {
            if let Some(word) = lowercase_start(line) {
                offenders.push(format!("{kind:?}: {line:?} starts with {word:?}"));
            }
        }
        read.push((kind, lines.len()));
    }
    assert!(read.len() >= 7, "the walk read only {read:?}");
    assert!(offenders.is_empty(), "{}", offenders.join("\n"));
}

/// A run's text and where it was drawn.
fn placed(run: &gpui::TextRunLayout) -> (String, u32, u32) {
    (
        run.text.to_string(),
        f32::from(run.bounds.left()).to_bits(),
        f32::from(run.bounds.top()).to_bits(),
    )
}

/// The dialog kinds the overlay registry holds.
fn registered(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
) -> Vec<DialogKind> {
    visual.read(|app| {
        let Some(host) = host_of(&state.read(app).ctx) else {
            panic!("the ui plugin mounts the overlay host");
        };
        host.dialog_kinds()
    })
}

/// Open `kind` and draw it.
fn open(
    state: &gpui::Entity<crate::app::AppState>,
    visual: &mut VisualTestContext,
    kind: DialogKind,
) {
    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.open_dialog(kind);
            cx.notify();
        });
    });
    visual.run_until_parked();
    assert_eq!(
        visual.read(|app| state.read(app).dialog),
        kind,
        "{kind:?} did not open"
    );
}

/// What is out of place on the frame just drawn with `kind` open, one line per
/// region.
fn misplaced(
    kind: DialogKind,
    window: Bounds<Pixels>,
    visual: &mut VisualTestContext,
) -> Vec<String> {
    let Some(card) = visual.debug_bounds("dialog-card") else {
        return vec![format!(
            "{kind:?} drew no dialog card; draw it through dialog_surface"
        )];
    };
    let mut out = Vec::new();
    if !bounds_inside(card, window) {
        out.push(format!(
            "{kind:?}: the card {card:?} is not inside the window"
        ));
    }
    for region in ["dialog-header", "dialog-content"] {
        match visual.debug_bounds(region) {
            None => out.push(format!("{kind:?} drew no {region}")),
            Some(bounds) if !bounds_inside(bounds, card) => out.push(format!(
                "{kind:?}: {region} {bounds:?} is not inside the card {card:?}"
            )),
            Some(_) => {}
        }
    }
    // A body with no action draws no action row.
    if let Some(footer) = visual.debug_bounds("dialog-footer")
        && !bounds_inside(footer, card)
    {
        out.push(format!(
            "{kind:?}: the action row {footer:?} is not inside the card {card:?}"
        ));
    }
    out
}
