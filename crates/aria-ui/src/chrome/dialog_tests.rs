//! Every dialog card fits the window it is drawn in, and every dialog title is
//! in Title Case.
//!
//! The class closed here: a card as tall as its body. A body taller than the
//! window put the title, the header controls and the action row outside the
//! window, where no pointer reaches them and no scroll brings them back.
//!
//! For every dialog kind the overlay registry holds, drawn in a window smaller
//! than a card in both directions, the card lies inside the window, and the
//! header, the scrolling content and the action row lie inside the card.
//!
//! The kinds are read from the registry at run time. A kind that draws no card
//! through [`super::dialog_surface`] fails here unless [`NO_CARD`] has a row for
//! it, and a [`NO_CARD`] row fails once its kind draws a card.
//!
//! Not covered: dialogs registered by a plugin outside this crate, pixels, and
//! which row the content region is scrolled to.

use gpui::{Bounds, Pixels, Size, TestAppContext, VisualTestContext, point, px, size};

use crate::app::DialogKind;
use crate::commands::casing_tests::check_title_case;
use crate::plugins::overlay_host::host_of;
use crate::test_support::{bounds_inside, drawn_shell};

/// A window narrower and shorter than a dialog card.
const SHORT: Size<Pixels> = size(px(480.), px(320.));

/// Registered dialogs that draw no card, and what they draw instead.
const NO_CARD: &[(DialogKind, &str)] = &[(
    DialogKind::ProjectReport,
    "the report is drawn in the shell's own column, with no scrim and no card",
)];

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
    for (kind, _) in NO_CARD {
        assert!(
            kinds.contains(kind),
            "{kind:?} has a NO_CARD row and no registration; delete the row"
        );
    }

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
        if NO_CARD.iter().any(|(held, _)| *held == kind) {
            continue;
        }
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
    let card = visual.debug_bounds("dialog-card");
    if let Some((_, instead)) = NO_CARD.iter().find(|(held, _)| *held == kind) {
        return match card {
            Some(_) => vec![format!(
                "{kind:?} draws a dialog card; delete its NO_CARD row ({instead})"
            )],
            None => Vec::new(),
        };
    }
    let Some(card) = card else {
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
