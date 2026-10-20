//! Driving the settings dialog's project pickers: every aspect, every delivery
//! size, every frame rate, every channel layout and every time display the
//! dialog draws.
//!
//! # Why
//!
//! Three operator commands claimed the dialog and nothing could press it, so a
//! row wired to the wrong setter would not have turned anything red. The rows are
//! a registry now ([`settings_pick`](crate::surfaces::settings_pick)) and this
//! presses every one of them, reading the arms back out of the registry's own
//! source so a pick added to the enum has no press and no green suite until
//! someone writes one.
//!
//! Each pick is pressed against a project that does *not* already hold it. A
//! picker is idempotent by design — choosing the aspect the project already has
//! writes no history and sends no command, which is the right behaviour and the
//! wrong measurement — so the dressing moves the project to another member of the
//! same catalog first. A row is only worth clicking when it is not the
//! current one.

use std::collections::BTreeSet;

use aria_models::{AudioLayout, CanvasAspect, Rational, TimeDisplay};

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::sent_census::Watch;
use crate::surfaces::settings_pick::{self, Pick, RATES};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "settings_pick.rs";

/// Every pick the dialog offers, in the order it draws them.
///
/// The size rows come off the project rather than out of a list here: the
/// baselines the dialog offers are the ones the project's own aspect yields, so a
/// hardcoded list would press rows the panel never drew.
fn picks(state: &AppState) -> Vec<Pick> {
    let mut out: Vec<Pick> = CanvasAspect::ALL.into_iter().map(Pick::Aspect).collect();
    out.push(Pick::Resolution(None));
    out.extend(
        state
            .canvas_resolution_choices()
            .into_iter()
            .map(|(_, size)| Pick::Resolution(Some(size))),
    );
    out.extend(RATES.into_iter().map(Pick::Rate));
    out.extend(AudioLayout::ALL.into_iter().map(Pick::Layout));
    out.extend(TimeDisplay::ALL.into_iter().map(Pick::Display));
    out
}

/// Move the project off `pick`, so pressing it is a change rather than a no-op.
///
/// One arm per family, so a pick added to the registry has to say how it is
/// dressed before this compiles.
fn away_from(state: &mut AppState, pick: Pick) {
    match pick {
        Pick::Aspect(aspect) => {
            let other = CanvasAspect::ALL
                .into_iter()
                .find(|candidate| *candidate != aspect)
                .expect("the model ships more than one aspect");
            install(state, Pick::Aspect(other));
        }
        Pick::Resolution(None) => {
            let (_, size) = state
                .canvas_resolution_choices()
                .into_iter()
                .next()
                .expect("the aspect yields at least one delivery size");
            install(state, Pick::Resolution(Some(size)));
        }
        Pick::Resolution(Some(_)) => install(state, Pick::Resolution(None)),
        Pick::Rate(rate) => {
            let other = RATES
                .into_iter()
                .find(|candidate| !aria_core::rate_eq(*candidate, rate))
                .expect("the dialog offers more than one rate");
            install(state, Pick::Rate(other));
        }
        Pick::Layout(layout) => {
            let other = AudioLayout::ALL
                .into_iter()
                .find(|candidate| *candidate != layout)
                .expect("the model ships more than one layout");
            install(state, Pick::Layout(other));
        }
        Pick::Display(display) => {
            let other = TimeDisplay::ALL
                .into_iter()
                .find(|candidate| *candidate != display)
                .expect("the model ships more than one time display");
            install(state, Pick::Display(other));
        }
    }
}

/// Press one row the way the dialog does, and refuse to continue if it is
/// refused: a dressing that cannot be installed is a broken dressing.
fn install(state: &mut AppState, pick: Pick) {
    let what = pick.what();
    settings_pick::apply(state, pick)
        .unwrap_or_else(|err| panic!("dressing the project with {what} was refused: {err}"));
}

/// Every command a picker asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for pick in picks(&with_clip().0) {
        let (mut state, _) = with_clip();
        away_from(&mut state, pick);
        let watch = Watch::on(&state);
        install(&mut state, pick);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_pick_the_dialog_offers_is_pressed() {
    // Fail by default on a new arm: `what` must cover it to compile, so it is
    // named here whether or not anyone pressed it.
    let pressed: BTreeSet<String> = picks(&with_clip().0)
        .into_iter()
        .map(|pick| pick.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "these settings rows are drawn and nothing presses them: {missing:?}. The \
         answer is a catalog this census reads, never an exception"
    );
}

#[test]
fn every_pick_installs_something() {
    // A row that dispatches nothing is a row that looks selected and changed no
    // project: the dialog closes and the sequence is what it was.
    for pick in picks(&with_clip().0) {
        let (mut state, _) = with_clip();
        away_from(&mut state, pick);
        let watch = Watch::on(&state);
        let what = pick.what();
        install(&mut state, pick);
        assert!(
            !watch.take().is_empty(),
            "picking {what} sent nothing, so either the row is inert or the \
             project already held what it offers"
        );
    }
}

#[test]
fn a_second_press_of_the_same_row_asks_for_nothing() {
    // The other half of the same contract, and the reason the census dresses
    // away from each pick: choosing what the project already has is not an edit,
    // so it must not reach the engine or the undo stack.
    for pick in picks(&with_clip().0) {
        let (mut state, _) = with_clip();
        away_from(&mut state, pick);
        install(&mut state, pick);
        let watch = Watch::on(&state);
        let what = pick.what();
        install(&mut state, pick);
        assert!(
            watch.take().is_empty(),
            "picking {what} twice asked the engine a second time, so the dialog \
             writes an undo entry for a click that changed nothing"
        );
    }
}

#[test]
fn the_census_reaches_every_command_these_pickers_claim() {
    let sent = dispatched();
    for label in [
        "Set canvas",
        "Set sequence frame rate",
        "Set audio layout",
        "Set time display",
    ] {
        assert!(
            sent.contains(label),
            "the settings census reached every row it draws and none of them sent \
             `{label}`"
        );
    }
}

#[test]
fn the_dialog_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "settings_dialog");
}

#[test]
fn the_rates_the_dialog_offers_are_distinct() {
    // A duplicated rate would draw two rows that look different and install the
    // same sequence, and the census above would press one of them twice.
    let mut seen: Vec<Rational> = Vec::new();
    for rate in RATES {
        assert!(
            !seen.iter().any(|kept| aria_core::rate_eq(*kept, rate)),
            "{}/{} is offered twice",
            rate.num,
            rate.den
        );
        seen.push(rate);
    }
}
