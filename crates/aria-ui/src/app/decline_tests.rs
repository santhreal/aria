//! Contracts for a declined command.
//!
//! The class closed here is "a command that finds nothing to act on does
//! nothing and says nothing". What is asserted:
//!
//! - Every reason has its own words, walked from the registry rather than
//!   retyped, so a reason added without wording is red here.
//! - The shared reader declines once for the whole family: a command that acts
//!   on the selection and finds none says so through it, and says it in the
//!   status line rather than as an error, because nothing failed.
//! - No clip being selected is worded once, by the registry: no production
//!   source outside it writes the words, so a menu row's tooltip and the line
//!   its command writes read the same.
//! - A chord is withheld with the menu row that draws it, in the row's words,
//!   and a row withholds no edit its chord's body would make.
//!
//! The transition family's own three refusals are asserted next to the family,
//! in `transition_tests.rs`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use aria_engine::EngineError;
use aria_models::TrackId;

use super::super::AppState;
use super::super::premiere_tests::with_clip;
use super::Decline;
use crate::chrome::menu_clip::{AUDIO_GAIN, EXTRACT_AUDIO, FREEZE, GROUP, LINK, TIME_REMAP};
use crate::chrome::menu_items::MenuItem as BarRow;
use crate::chrome::menu_items::{chord_rows, items_of};
use crate::keys::bindings::SHIPPED_BINDINGS;
use crate::keys::{KeyBody, dispatch_plain_key_down};
use crate::test_support::sources::{production_sources, string_literals};
use crate::types::AppMenu;

#[test]
fn every_reason_has_its_own_words() {
    let mut seen = BTreeSet::new();
    for why in Decline::ALL {
        let reason = why.reason();
        assert!(!reason.trim().is_empty(), "{why:?} has no wording");
        assert!(
            seen.insert(reason),
            "{why:?} repeats another reason: {reason}"
        );
    }
}

#[test]
fn a_command_with_no_selection_says_so_and_does_not_fail() {
    let (mut state, _clip) = with_clip();
    state.clear_selection();
    state.status_message = None;

    state
        .set_selected_transition("crossfade")
        .expect("declining is not an error");

    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
    assert_eq!(state.last_error, None);
}

/// The reader is the one place the selection is read for a command, so it is
/// also the one place the refusal is written: a caller cannot forget it.
#[test]
fn the_shared_reader_returns_the_selection_and_says_nothing_when_there_is_one() {
    let (mut state, clip) = with_clip();
    state.status_message = None;

    assert_eq!(state.selected_clip_or_decline(), Some(clip));
    assert_eq!(state.status_message, None, "a selection is not a refusal");

    state.clear_selection();
    assert_eq!(state.selected_clip_or_decline(), None);
    assert_eq!(
        state.status_message.as_deref(),
        Some(Decline::NoSelection.reason())
    );
}

/// **No clip being selected is worded once, by the registry.**
///
/// The class closed here: a command that found no clip selected and wrote its
/// own line for it, so `Select a clip first`, `No clip is selected` and
/// `Nothing is selected` stated one state three ways, and a menu row's tooltip
/// read differently from the line the same command wrote when run from the
/// palette. Every string literal in the crate's production sources is read
/// with test items blanked, and one that words an empty selection fails unless
/// it is in this module.
///
/// Not covered: a wording outside `WORDINGS`, such as a new phrasing of the
/// same state. A panel's hint to select a clip, as the Info panel's `Select a
/// clip.` is, states how to fill the panel rather than why a command did
/// nothing, and is not read as one.
#[test]
fn only_the_registry_words_an_empty_selection() {
    const WORDINGS: [&str; 5] = [
        "select a clip first",
        "no clip is selected",
        "no clip selected",
        "nothing is selected",
        "nothing selected",
    ];
    assert!(
        WORDINGS.contains(&Decline::NoSelection.reason().to_lowercase().as_str()),
        "the registry's wording is not one the census reads"
    );
    let mut read = 0usize;
    let mut offenders = Vec::new();
    for (rel, code) in production_sources() {
        if rel == "app/decline.rs" {
            continue;
        }
        for (line, literal) in string_literals(code) {
            read += 1;
            let words = literal.trim().trim_end_matches('.').to_lowercase();
            if WORDINGS.contains(&words.as_str()) {
                offenders.push(format!("{rel}:{line}: {literal:?}"));
            }
        }
    }
    assert!(read > 1000, "read only {read} string literals");
    assert!(
        offenders.is_empty(),
        "state an empty selection through Decline::NoSelection:\n{}",
        offenders.join("\n")
    );
}

/// A clip command whose menu bar row is withheld declines in that row's words
/// when it runs anyway, as from a key, and leaves no undo step and no dialog.
///
/// The class closed: a guard and its command that disagree, so the bar drew a
/// row disabled for one reason while the command, run past it, linked a linked
/// pair again or sent the engine an edit it rejects. Covered: Link on a linked
/// pair, Group on a group, and Time Remapping, Freeze Frame, Extract Audio and
/// Audio Gain on a title. Not covered: a guard and command pair not listed.
#[test]
fn a_withheld_clip_command_declines_in_its_rows_words() {
    type Dress = fn() -> AppState;
    type Run = fn(&mut AppState) -> Result<(), EngineError>;
    let linked: Dress = crate::app::fixtures::linked_pair;
    let grouped: Dress = || {
        let mut state = crate::app::fixtures::linked_pair();
        state.unlink_selected().expect("the pair comes apart");
        state.group_selected().expect("the pair forms a group");
        state
    };
    let title: Dress = || crate::app::clip_text::text_caret::tests::typing().0;
    let cases: [(&BarRow, Dress, Run); 6] = [
        (&LINK, linked, |state| state.link_selected()),
        (&GROUP, grouped, |state| state.group_selected()),
        (&TIME_REMAP, title, |state| state.toggle_time_remap()),
        (&FREEZE, title, |state| state.freeze_selected_frame()),
        (&EXTRACT_AUDIO, title, |state| {
            state.extract_selected_audio()
        }),
        (&AUDIO_GAIN, title, |state| {
            state.open_audio_gain();
            Ok(())
        }),
    ];
    for (row, dress, run) in cases {
        let mut state = dress();
        let id = row.id;
        let reason = row
            .blocked_by(&state)
            .unwrap_or_else(|| panic!("{id} is offered on its dressed state"));
        let before = (state.history_depth(), state.dialog);
        state.status_message = None;
        run(&mut state).unwrap_or_else(|err| panic!("{id} failed instead of declining: {err}"));
        assert_eq!(
            state.status_message.as_deref(),
            Some(reason),
            "{id} declined in other words than its row"
        );
        assert_eq!(
            (state.history_depth(), state.dialog),
            before,
            "{id} changed the project or opened a dialog"
        );
        assert_eq!(state.last_error, None, "{id} reported an error");
    }
}

/// A chord pressed while the menu row that draws it is withheld declines in
/// that row's words and changes nothing, and the chord's own body, run past the
/// withholding, changes nothing either.
///
/// The class closed: a chord and the row that draws it disagreeing about one
/// state. A chord ran past its row's guard, so the bar drew Lift disabled with
/// one sentence while its key wrote another, and a key bound to a disabled row
/// did nothing and said nothing. A guard was stricter than the code it guards,
/// so the bar drew Clear, Ripple Delete and Deselect All disabled over a
/// selected gap their keys acted on. A chord is withheld with its row,
/// so a guard that withholds a working edit would take it from the keyboard
/// too, and the body is run past the guard to show it would not.
///
/// The rows are walked from the live menus and the chords from the shipped key
/// table, on three dressings: a clip with nothing selected, a selected gap,
/// and an empty sequence. A keyed row added later is checked without being
/// listed, and the chord index built once for the keypress is the keyed rows
/// each dressing draws, so a keyed row drawn on some states only is red.
///
/// Not covered: a row withheld only on a state no dressing reaches, a chord
/// body's effect outside the project, the selection, the playhead and the open
/// dialog, and the chord of a plugin command, which no shipped row draws.
#[test]
fn a_chord_past_its_withheld_row_declines_in_the_rows_words() {
    type Dress = fn() -> AppState;
    let unselected: Dress = || {
        let (mut state, _) = with_clip();
        state.clear_selection();
        state
    };
    let gap: Dress = || {
        let (mut state, _) = with_clip();
        let lane = &state.store.sequence.tracks[0];
        let (track, end) = (TrackId::from_raw(lane.id), lane.clips[0].timeline_end_ticks);
        let texture =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png");
        let media = state
            .import_media(texture)
            .expect("the fixture imports again");
        state
            .drop_media_on_timeline(media, Some(track), end + 48)
            .expect("a second clip lands after a gap");
        state
            .select_gap_at(track, end + 1)
            .expect("the span between the clips is a gap");
        state
    };
    let empty: Dress = || {
        let (mut state, _) = with_clip();
        state.new_sequence().expect("an empty sequence opens");
        state
    };
    let seen = |state: &AppState| {
        (
            state.history_depth(),
            state.dialog,
            state.store.clone(),
            state.selection.clone(),
            state.selected_gap,
            state.playhead,
            state.active_panel,
            state.sources.focus_index(),
        )
    };
    let mut wrong = Vec::new();
    for (dressing, dress) in [("unselected", unselected), ("gap", gap), ("empty", empty)] {
        // One state serves every row of the dressing while each row leaves it
        // as it found it, and is dressed again after a row that did not: a
        // dressed state costs a renderer, and two never live at once because
        // each claims the same scratch.
        let mut state = Some(dress());
        let mut drawn = BTreeMap::new();
        for menu in AppMenu::ALL {
            for row in items_of(menu, state.as_ref().expect("dressed")) {
                if let Some(key) = row.key {
                    drawn.entry(key).or_insert(row.id);
                }
            }
        }
        let indexed: BTreeMap<&str, &str> = chord_rows(state.as_ref().expect("dressed"))
            .iter()
            .map(|(&key, &(row, _))| (key, row))
            .collect();
        assert_eq!(
            indexed, drawn,
            "the chords read on a keypress are not the rows drawn on the {dressing} dressing"
        );
        let withheld: Vec<(&str, &str, &str)> = AppMenu::ALL
            .into_iter()
            .flat_map(|menu| items_of(menu, state.as_ref().expect("dressed")))
            .filter_map(|row| {
                let reason = row.blocked_by(state.as_ref().expect("dressed"))?;
                Some((row.id, row.key?, reason))
            })
            .collect();
        assert!(
            !withheld.is_empty(),
            "no keyed row is withheld on the {dressing} dressing"
        );
        for (id, key, reason) in withheld {
            let binding = SHIPPED_BINDINGS
                .iter()
                .find(|binding| binding.id == key)
                .unwrap_or_else(|| panic!("{id} draws {key}, which no shipped binding is"));
            let KeyBody::Plain(run) = binding.run else {
                wrong.push(format!(
                    "{key} past {id} ({dressing}): its chord is not withheld"
                ));
                continue;
            };
            let found = wrong.len();
            let at = state.get_or_insert_with(dress);
            let before = seen(at);
            at.status_message = None;
            at.last_error = None;
            dispatch_plain_key_down(at, binding, false, run);
            if at.status_message.as_deref() != Some(reason) {
                wrong.push(format!(
                    "{key} past {id} ({dressing}): row says {reason:?}, chord wrote {:?}",
                    at.status_message
                ));
            }
            if seen(at) != before || at.last_error.is_some() {
                wrong.push(format!("{key} past {id} ({dressing}): the chord edited"));
            }
            run(at);
            if seen(at) != before {
                wrong.push(format!(
                    "{key} past {id} ({dressing}): its row withholds an edit its body makes"
                ));
            }
            if wrong.len() > found {
                state = None;
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// WHY: closes the class of a chord bound to a menu row's action that no row
/// declares. The menu then draws no shortcut beside the row, and a withheld
/// row does not withhold the chord, so the chord reaches the body the row was
/// drawn disabled over. A binding named like a row is that row's chord, walked
/// from the shipped table and every live menu: some row of that name declares
/// one of the bindings of that name, so a new binding named after a row is red
/// here until a row declares it. A second binding of one name is a second
/// chord for the action (Ctrl+Y beside Ctrl+Shift+Z for Redo), and one row of
/// a name drawn in two menus draws the chord once.
///
/// Rows a registered analyzer contributes are skipped: the detector chords
/// start the built-in detector job, which those rows do not run.
///
/// What this does not catch: a chord whose name differs from the row it runs,
/// and a second chord of one name, which no row withholds.
#[test]
fn a_binding_named_like_a_row_is_declared_on_a_row_of_that_name() {
    const ANALYZER_ROWS: [&str; 2] = ["clip-analyze:", "seq-analyze:"];
    let (state, _) = with_clip();
    let mut declared: BTreeMap<&str, Vec<Option<&str>>> = BTreeMap::new();
    for menu in AppMenu::ALL {
        for row in items_of(menu, &state) {
            if ANALYZER_ROWS
                .iter()
                .any(|prefix| row.id.starts_with(prefix))
            {
                continue;
            }
            declared.entry(row.label).or_default().push(row.key);
        }
    }
    let wrong: Vec<String> = declared
        .iter()
        .filter_map(|(label, keys)| {
            let named: Vec<&str> = SHIPPED_BINDINGS
                .iter()
                .filter(|binding| binding.name == *label)
                .map(|binding| binding.id)
                .collect();
            let drawn = keys.iter().flatten().any(|key| named.contains(key));
            (!named.is_empty() && !drawn)
                .then(|| format!("\"{label}\" declares {keys:?}, none of {named:?}"))
        })
        .collect();
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
