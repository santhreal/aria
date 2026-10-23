//! Driving the pool card's own click: the plain one, the modified one, the run
//! between two cards, and the double-click that opens a Source tab.
//!
//! # Why
//!
//! A click on a card is a selection gesture and never an edit: browsing the
//! pool must not put clips on the sequence. The selection is a set, because the
//! sidebar's multicam button builds its group from it, and a double-click opens
//! the card in the Viewer rather than placing it twice. The placement a click
//! once made is Insert or Overwrite with the pool selection and no Source tab in
//! front, pressed here as well.
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
    vec![Reach::Replace, Reach::Toggle, Reach::Span, Reach::Open]
}

/// Click one card.
fn click(state: &mut AppState, reach: Reach, card: u64) {
    pool_pick::apply(state, reach, card, TAB);
}

/// The clips on the sequence as (start, end) timeline ticks, in start order.
fn spans(state: &AppState) -> Vec<(i64, i64)> {
    let mut spans: Vec<(i64, i64)> = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .filter(|clip| clip.kind != crate::types::UiClipKind::Audio)
        .map(|clip| (clip.timeline_start_ticks, clip.timeline_end_ticks))
        .collect();
    spans.sort_unstable();
    spans
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

/// Every command a click on a card asked for: none, which the census below
/// holds every reach to.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    // The count comes off a state that is dropped before the loop: a census
    // holding two projects open at once holds two compositor devices.
    let count = reaches().len();
    for index in 0..count {
        let mut pool = stocked();
        let reach = reaches().swap_remove(index);
        let card = pool.cards[1];
        let watch = Watch::on(&pool.state);
        click(&mut pool.state, reach, card);
        out.extend(watch.take());
    }
    out
}

#[test]
fn every_way_of_clicking_a_card_is_pressed() {
    // Fail by default on a new arm: `what` is a total match, so the compiler
    // forces prose for a fifth way of clicking and this forces a press.
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
fn no_click_on_a_card_edits_the_project() {
    // The defect this forbids: browsing the pool drops clips on the sequence,
    // and picking four angles leaves four undo steps behind a multicam group.
    assert_eq!(
        dispatched(),
        BTreeSet::new(),
        "a click on a card sent commands to the project"
    );
    for reach in reaches() {
        let mut pool = stocked();
        let before = spans(&pool.state);
        click(&mut pool.state, reach, pool.cards[0]);
        assert!(
            pool.state.pool_selection.contains(pool.cards[0]),
            "{} left the card unpicked",
            reach.what()
        );
        assert_eq!(spans(&pool.state), before, "{} placed a clip", reach.what());
    }
}

#[test]
fn the_click_count_and_modifiers_pick_the_row() {
    assert_eq!(Reach::of(false, false, 1), Reach::Replace);
    assert_eq!(Reach::of(false, false, 2), Reach::Open);
    assert_eq!(Reach::of(false, false, 3), Reach::Open);
    // A fast run of modified clicks builds a selection; it never opens tabs.
    assert_eq!(Reach::of(true, false, 2), Reach::Toggle);
    assert_eq!(Reach::of(false, true, 2), Reach::Span);
    assert_eq!(Reach::of(true, true, 1), Reach::Span);
}

#[test]
fn a_plain_click_picks_one_card() {
    let mut pool = stocked();
    click(&mut pool.state, Reach::Toggle, pool.cards[0]);
    click(&mut pool.state, Reach::Replace, pool.cards[2]);
    assert_eq!(
        pool.state.pool_selection.ids(),
        [pool.cards[2]],
        "a plain click kept the card that was already picked"
    );
    assert!(
        pool.state.sources.tabs().is_empty(),
        "a single click opened a Source tab"
    );
}

#[test]
fn a_double_click_opens_one_source_tab_per_card() {
    let mut pool = stocked();
    let focused = |state: &AppState| state.sources.focused().map(|view| view.media);
    click(&mut pool.state, Reach::Open, pool.cards[0]);
    assert_eq!(focused(&pool.state), Some(pool.cards[0]));
    if let Some(view) = pool.state.sources.focused_mut() {
        view.mark_in = Some(1);
    }
    click(&mut pool.state, Reach::Open, pool.cards[1]);
    assert_eq!(focused(&pool.state), Some(pool.cards[1]));
    // A second double-click on the first card focuses its tab, marks intact.
    click(&mut pool.state, Reach::Open, pool.cards[0]);
    let open: Vec<u64> = pool
        .state
        .sources
        .tabs()
        .iter()
        .map(|view| view.media)
        .collect();
    assert_eq!(
        open,
        vec![pool.cards[0], pool.cards[1]],
        "a card opened twice"
    );
    assert_eq!(focused(&pool.state), Some(pool.cards[0]));
    assert_eq!(
        pool.state.sources.focused().and_then(|view| view.mark_in),
        Some(1),
        "refocusing a tab reset its marks"
    );
    assert_eq!(pool.state.pool_selection.ids(), [pool.cards[0]]);
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
    // Two angles in pick order, which is the order the group carries them in.
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
fn a_card_whose_file_is_gone_is_picked_and_not_opened() {
    // Offline is not a refusal of the pick: the properties and the relink
    // button are on the card, so it has to be selectable. What it must not do
    // is open a Source tab with nothing behind it to decode.
    let (mut state, card) = with_offline_card();
    let watch = Watch::on(&state);
    pool_pick::apply(&mut state, Reach::Open, card, TAB);
    assert_eq!(watch.take(), Vec::<&'static str>::new());
    assert!(
        state.pool_selection.contains(card),
        "an offline card cannot be selected, so its relink button is unreachable"
    );
    assert!(
        state.sources.tabs().is_empty(),
        "an offline card opened a tab"
    );
    assert_eq!(
        state.status_message.as_deref(),
        Some("That source cannot be previewed")
    );
}

#[test]
fn insert_and_overwrite_place_the_pool_selection_with_no_source_tab() {
    // The click's old placement, now a key: the selection in pick order, end to
    // end from the playhead, through the mode the key names and not the
    // sequence's edit mode.
    // Each item is one Source edit through the patching, as a drop is.
    let label = "Source edit";
    for mode in [EditMode::Ripple, EditMode::Overwrite] {
        let mut pool = stocked();
        pool.state.edit_mode = EditMode::Overwrite;
        // Past the fixture's own clip, so the placement is all that lands there.
        let before = spans(&pool.state);
        let end = before.iter().map(|span| span.1).max().unwrap_or(0);
        pool.state.playhead = aria_models::RationalTime::new(end, pool.state.playhead.rate);
        click(&mut pool.state, Reach::Replace, pool.cards[2]);
        click(&mut pool.state, Reach::Toggle, pool.cards[0]);
        let applied =
            |state: &AppState| state.history_entries().iter().filter(|e| e.applied).count();
        let steps = applied(&pool.state);
        let watch = Watch::on(&pool.state);
        pool.state.cut_source_in(mode).expect("place the selection");
        // The two Source edits merge into one step, and the History panel's
        // list shows that one step without waiting for another edit.
        assert_eq!(
            applied(&pool.state),
            steps + 1,
            "{mode:?}: the History list"
        );
        assert_eq!(
            watch.take(),
            vec![label, label],
            "{mode:?} sent the wrong commands"
        );
        let placed: Vec<(i64, i64)> = spans(&pool.state)
            .into_iter()
            .filter(|span| span.0 >= end)
            .collect();
        assert_eq!(placed.len(), 2, "{mode:?} placed {placed:?}");
        assert_eq!(placed[0].0, end, "the first item starts at the playhead");
        assert_eq!(
            placed[1].0, placed[0].1,
            "the second item follows the first"
        );
        assert!(pool.state.undo(), "the placement is undoable");
        assert_eq!(
            spans(&pool.state),
            before,
            "one undo removes the whole placement"
        );
    }
}

#[test]
fn insert_with_nothing_to_place_says_so() {
    let (mut state, _) = crate::app::fixtures::with_video();
    state.pool_selection = Default::default();
    let watch = Watch::on(&state);
    state
        .cut_source_in(EditMode::Ripple)
        .expect("refused, not failed");
    assert_eq!(watch.take(), Vec::<&'static str>::new());
    assert_eq!(
        state.status_message.as_deref(),
        Some("Open a source in the Viewer, or select pool items, to place")
    );
}

#[test]
fn the_pool_panel_calls_no_setter_itself() {
    super::no_setter_escapes(REGISTRY, "sidebar_media");
}
