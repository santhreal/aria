//! Driving the pool card's own click: the plain one, the modified one, and the
//! run between two cards.
//!
//! # Why
//!
//! The card's behaviour lived in a mouse listener: select this item, then place
//! it unless its file is gone. Nothing could enumerate it, so the offline case
//! read like an ordinary early return and the selection could hold one item at a
//! time while the sidebar's multicam button claimed to read a set.
//!
//! The registry is [`pool_pick`](crate::surfaces::pool_pick). The pool the rows
//! are pressed against is stocked with three cards, because a run needs a middle
//! one: a span of two proves nothing a plain pair of clicks would not.
//!
//! The drawn order is read off the state rather than assumed to be the import
//! order. A span runs along what the list drew, and the list sorts.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::sent_census::Watch;
use crate::surfaces::pool_pick::{self, Reach};
use crate::types::{EditMode, SidebarTab};

/// This registry's file, for the shared parsers in the parent module.
const REGISTRY: &str = "pool_pick.rs";

/// The tab the cards below are drawn in.
const TAB: SidebarTab = SidebarTab::Media;

/// A pool of three cards, and the order the list draws them in.
struct Stocked {
    state: AppState,
    cards: Vec<u64>,
}

fn stocked() -> Stocked {
    let (mut state, _) = crate::app::fixtures::with_video();
    for name in [
        "fixture-640x360-24fps-3s.mp4",
        "fixture-640x360-30fps-3s.mp4",
    ] {
        state
            .import_media(crate::app::fixtures::asset(name))
            .expect("a second and third card to run a span across");
    }
    state.sync_from_engine();
    let cards = drawn(&state);
    assert_eq!(
        cards.len(),
        3,
        "a span needs a card between its two ends, so the pool holds three"
    );
    Stocked { state, cards }
}

/// The pool in the order the list draws it.
fn drawn(state: &AppState) -> Vec<u64> {
    state.pool_rows(TAB).ids()
}

/// Every way of clicking a card. One per arm of `Reach::what`, which
/// `every_way_of_clicking_a_card_is_pressed` holds this list to.
fn reaches() -> Vec<Reach> {
    vec![Reach::Replace, Reach::Toggle, Reach::Span]
}

/// Click one card, refusing to pass a refusal off as a click.
fn click(state: &mut AppState, reach: Reach, card: u64) -> bool {
    pool_pick::apply(state, reach, card, TAB)
        .unwrap_or_else(|err| panic!("`{}` was refused: {err}", reach.what()))
}

/// A pool holding one card whose file is gone, and that card's id.
///
/// The file is really removed: whether a lost path reads as missing is the
/// projection's rule, and a dressing that set the flag by hand would pass
/// against a projection that had stopped reading paths at all. What the dressing
/// has to work around is the revision gate: the pool is rebuilt when the project
/// changes, so a file that disappears behind a settled state stays drawn as
/// online until the next edit. The next import is that edit, and it is what an
/// operator does after losing a card.
fn with_offline_card() -> (AppState, u64) {
    let dir = crate::scratch_pool::scratch("pool-pick");
    let staged = dir.join("gone.mp4");
    std::fs::copy(
        crate::app::fixtures::asset("fixture-640x360-24fps-3s.mp4"),
        &staged,
    )
    .expect("stage a file that can be lost");
    let (mut state, _) = crate::app::fixtures::with_video();
    let card = state
        .import_media(staged)
        .expect("import the staged file")
        .raw();
    std::fs::remove_file(dir.join("gone.mp4")).expect("lose the file");
    crate::projection::invalidate_media_statuses();
    state
        .import_media(crate::app::fixtures::asset("fixture-640x360-30fps-3s.mp4"))
        .expect("a second card, and the edit that rebuilds the pool");
    state.sync_from_engine();
    assert!(
        state
            .store
            .media
            .iter()
            .any(|item| item.id == card && item.missing),
        "the pool still draws the lost card as online, so this proves nothing"
    );
    (state, card)
}

/// Every command a click on a card asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    // The count comes off a state that is dropped before the loop: a census
    // holding two projects open at once holds two compositor devices.
    let count = reaches().len();
    for index in 0..count {
        let mut pool = stocked();
        let reach = reaches().swap_remove(index);
        let card = pool.cards[1];
        pool.state.edit_mode = EditMode::Ripple;
        let watch = Watch::on(&pool.state);
        click(&mut pool.state, reach, card);
        out.extend(watch.take());
    }
    // The one reach that places goes through the edit mode, so the other mode is
    // pressed too: the command a click sends is the sequence's mode, not a
    // property of the card.
    let mut pool = stocked();
    pool.state.edit_mode = EditMode::Overwrite;
    let card = pool.cards[1];
    let watch = Watch::on(&pool.state);
    click(&mut pool.state, Reach::Replace, card);
    out.extend(watch.take());
    out
}

#[test]
fn every_way_of_clicking_a_card_is_pressed() {
    // Fail by default on a new arm: `what` is a total match, so the compiler
    // forces prose for a fourth way of clicking and this forces a press.
    let pressed: BTreeSet<String> = reaches()
        .into_iter()
        .map(|reach| reach.what().to_string())
        .collect();
    let missing: Vec<String> = super::arms_of(REGISTRY)
        .into_iter()
        .filter(|arm| !pressed.contains(arm))
        .collect();
    assert!(
        missing.is_empty(),
        "a card can be clicked these ways and nothing presses them: {missing:?}"
    );
}

#[test]
fn a_plain_click_picks_one_card_and_places_it() {
    let mut pool = stocked();
    let watch = Watch::on(&pool.state);
    click(&mut pool.state, Reach::Toggle, pool.cards[0]);
    let placed = click(&mut pool.state, Reach::Replace, pool.cards[2]);
    assert!(placed, "a plain click no longer places the clip");
    assert_eq!(
        pool.state.pool_selection.ids(),
        [pool.cards[2]],
        "a plain click kept the card that was already picked"
    );
    assert!(
        !watch.take().is_empty(),
        "a plain click asked the project for nothing"
    );
}

#[test]
fn a_modified_click_picks_without_touching_the_project() {
    // The defect this forbids: picking four angles drops four clips on the
    // sequence, and the operator undoes four times to get back.
    for reach in [Reach::Toggle, Reach::Span] {
        let mut pool = stocked();
        let watch = Watch::on(&pool.state);
        let placed = click(&mut pool.state, reach, pool.cards[0]);
        assert!(!placed, "{} placed a clip", reach.what());
        assert_eq!(
            watch.take(),
            Vec::<&'static str>::new(),
            "{} edited the project",
            reach.what()
        );
        assert!(
            pool.state.pool_selection.contains(pool.cards[0]),
            "{} left the card unpicked",
            reach.what()
        );
    }
}

#[test]
fn a_span_picks_the_run_the_list_drew_between_the_two_cards() {
    // The card in the middle is the whole point: a span that only picked its two
    // ends would pass a two-card test and lose every camera between them.
    let mut pool = stocked();
    click(&mut pool.state, Reach::Replace, pool.cards[0]);
    click(&mut pool.state, Reach::Span, pool.cards[2]);
    assert_eq!(
        pool.state.pool_selection.ids(),
        pool.cards.as_slice(),
        "the span skipped the card drawn between its ends"
    );
}

#[test]
fn a_selection_of_several_is_what_a_multicam_group_is_built_from() {
    // The surface the single selection broke. Two angles in pick order, which is
    // the order the group carries them in.
    let mut pool = stocked();
    click(&mut pool.state, Reach::Replace, pool.cards[2]);
    click(&mut pool.state, Reach::Toggle, pool.cards[0]);
    let angles = crate::surfaces::sidebar_pick::angles_from_pool(&pool.state);
    let picked: Vec<u64> = angles.iter().map(|angle| angle.media.raw()).collect();
    assert_eq!(
        picked,
        vec![pool.cards[2], pool.cards[0]],
        "the group's angles are not the cards that were picked, in that order"
    );
}

#[test]
fn a_card_whose_file_is_gone_is_picked_and_not_placed() {
    // Offline is not a refusal: the properties and the relink button are on the
    // card, so it has to be selectable. What it must not do is put a clip on the
    // timeline with nothing behind it to decode.
    let (mut state, card) = with_offline_card();
    let watch = Watch::on(&state);
    let placed = pool_pick::apply(&mut state, Reach::Replace, card, TAB)
        .expect("an offline card is clickable");
    assert!(!placed, "an offline card put a clip on the timeline");
    assert_eq!(
        watch.take(),
        Vec::<&'static str>::new(),
        "an offline card edited the project"
    );
    assert!(
        state.pool_selection.contains(card),
        "an offline card cannot be selected, so its relink button is unreachable"
    );
}

#[test]
fn the_census_reaches_the_commands_a_click_claims() {
    let sent = dispatched();
    for label in ["Ripple insert", "Overwrite"] {
        assert!(
            sent.contains(label),
            "a click on a card is the fallback for the drag and it sent {sent:?}"
        );
    }
}

#[test]
fn a_click_places_through_the_edit_mode() {
    // The card does not decide how a clip lands. The sequence's mode does, the
    // same one a drag from the pool goes through.
    for (mode, label) in [
        (EditMode::Ripple, "Ripple insert"),
        (EditMode::Overwrite, "Overwrite"),
    ] {
        let mut pool = stocked();
        pool.state.edit_mode = mode;
        let watch = Watch::on(&pool.state);
        let placed = click(&mut pool.state, Reach::Replace, pool.cards[1]);
        assert!(placed, "a click in {mode:?} placed nothing");
        assert_eq!(
            watch.take(),
            vec![label],
            "a click in {mode:?} sent the wrong command"
        );
    }
}

#[test]
fn the_pool_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "sidebar_media");
}
