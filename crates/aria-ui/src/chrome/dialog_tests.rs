//! Every dialog card fits the window it is drawn in.
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
use crate::plugins::overlay_host::host_of;
use crate::test_support::drawn_shell;

/// A window narrower and shorter than a dialog card.
const SHORT: Size<Pixels> = size(px(480.), px(320.));

/// Registered dialogs that draw no card, and what they draw instead.
const NO_CARD: &[(DialogKind, &str)] = &[(
    DialogKind::ProjectReport,
    "the report is drawn in the shell's own column, with no scrim and no card",
)];

/// Whether `inner` lies inside `outer`, edges included. Layout is in floats, so
/// an edge shared with the outer box may land a rounding error past it.
fn inside(inner: Bounds<Pixels>, outer: Bounds<Pixels>) -> bool {
    let slack = px(0.5);
    inner.left() >= outer.left() - slack
        && inner.top() >= outer.top() - slack
        && inner.right() <= outer.right() + slack
        && inner.bottom() <= outer.bottom() + slack
}

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

    let kinds = visual.read(|app| {
        let Some(host) = host_of(&state.read(app).ctx) else {
            panic!("the ui plugin mounts the overlay host");
        };
        host.dialog_kinds()
    });
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
        offenders.extend(misplaced(kind, window, visual));
    }
    assert!(
        offenders.is_empty(),
        "drawn in a {:?} window:\n  {}",
        window.size,
        offenders.join("\n  ")
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
    if !inside(card, window) {
        out.push(format!(
            "{kind:?}: the card {card:?} is not inside the window"
        ));
    }
    for region in ["dialog-header", "dialog-content"] {
        match visual.debug_bounds(region) {
            None => out.push(format!("{kind:?} drew no {region}")),
            Some(bounds) if !inside(bounds, card) => out.push(format!(
                "{kind:?}: {region} {bounds:?} is not inside the card {card:?}"
            )),
            Some(_) => {}
        }
    }
    // A body with no action draws no action row.
    if let Some(footer) = visual.debug_bounds("dialog-footer")
        && !inside(footer, card)
    {
        out.push(format!(
            "{kind:?}: the action row {footer:?} is not inside the card {card:?}"
        ));
    }
    out
}
