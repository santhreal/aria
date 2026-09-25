//! Driving the pool's folder strip: every button, on a root and inside a folder.
//!
//! Two of the three buttons only exist inside a folder, so a census that pressed
//! them against one state would report two of them as unreachable. Each state
//! here is dressed for what it offers, and `every_bin_button_is_offered_by_some_state`
//! makes a button nobody dressed a failure rather than a gap.
//!
//! A button that opens the name field is followed through the field: the strip's
//! job is to put the caret there, and the edit is what the field commits. The
//! keystrokes go through the field row's own `push`, so the census cannot type
//! past the name cap in a way the keyboard cannot.

use std::collections::BTreeSet;

use crate::app::AppState;
use crate::app::premiere_tests::with_clip;
use crate::app::sent_census::Watch;
use crate::surfaces::bin_strip::{BIN_ACTIONS, BinStrip};

/// A dressed pool and what the strip would draw over it.
struct Stage {
    what: &'static str,
    dress: fn() -> AppState,
}

const BIN_STAGES: &[Stage] = &[
    Stage {
        what: "the pool root",
        dress: at_the_root,
    },
    Stage {
        what: "an open folder",
        dress: inside_a_folder,
    },
];

/// The pool with no folder open: only the button that makes one applies.
fn at_the_root() -> AppState {
    let (state, _) = with_clip();
    assert_eq!(
        state.current_bin, None,
        "a fresh project already has a folder open"
    );
    state
}

/// One folder, open, with the name field closed: pressing the strip's buttons
/// from here is what an operator does after making a folder and naming it.
fn inside_a_folder() -> AppState {
    let (mut state, _) = with_clip();
    state.create_bin().expect("the folder lands");
    state.cancel_bin_rename();
    assert!(
        state.current_bin.is_some(),
        "making a folder did not open it"
    );
    state
}

/// What the name field is typed with when a button opens it.
const TYPED: &str = " two";

/// Press one row against a fresh dressed state, following through any field it
/// opens, and report what was dispatched.
fn press(
    dress: fn() -> AppState,
    row: &crate::surfaces::bin_strip::BinAction,
) -> Vec<&'static str> {
    let mut state = dress();
    let strip = BinStrip::of(&state);
    let watch = Watch::on(&state);
    (row.run)(&mut state, &strip)
        .unwrap_or_else(|err| panic!("the strip's {} button was refused: {err}", row.id));
    finish_any_field(&mut state);
    watch.take()
}

/// Finish the name field, if the press opened one.
///
/// Typed through the field row's own `push`, which is the function the shell's
/// key handler calls. `commit_bin_rename` stands in for the row's `submit`, which
/// takes a gpui context a headless census has no way to build;
/// `the_strip_is_built_from_the_registry` guards that the row's submit is that
/// same call.
fn finish_any_field(state: &mut AppState) {
    let Some(field) = crate::keys::fields::focused(state) else {
        return;
    };
    assert_eq!(
        field.id, "aria.field.bin-rename",
        "the strip opened a field the pool does not own"
    );
    for ch in TYPED.chars() {
        (field.push)(state, ch);
    }
    state.commit_bin_rename().expect("the folder rename lands");
}

/// Every command the folder strip asked for.
pub(super) fn dispatched() -> BTreeSet<&'static str> {
    let mut out = BTreeSet::new();
    for stage in BIN_STAGES {
        let dressed = (stage.dress)();
        let strip = BinStrip::of(&dressed);
        for row in BIN_ACTIONS {
            if !(row.when)(&strip) {
                continue;
            }
            out.extend(press(stage.dress, row));
        }
    }
    out
}

/// What a press is allowed to have changed: the tree, which folder is open, and
/// what is being typed.
fn shape(state: &AppState) -> (Option<u64>, Option<String>, Vec<String>) {
    (
        state.current_bin,
        state.bin_rename.as_ref().map(|rename| rename.draft.clone()),
        state
            .store
            .bins
            .iter()
            .map(|item| format!("{}:{}", item.id, item.name))
            .collect(),
    )
}

#[test]
fn every_bin_button_is_offered_by_some_state() {
    // A row nobody offers is a button that draws nowhere. The answer is a dressed
    // state, not an exception list.
    for row in BIN_ACTIONS {
        let offered = BIN_STAGES.iter().any(|stage| {
            let dressed = (stage.dress)();
            (row.when)(&BinStrip::of(&dressed))
        });
        assert!(
            offered,
            "no dressed pool offers the strip's {} button",
            row.id
        );
    }
}

#[test]
fn no_bin_button_is_inert() {
    // Offered means it does something. A button drawn over a state it cannot act
    // on is the defect this registry exists to make visible.
    for stage in BIN_STAGES {
        let dressed = (stage.dress)();
        let strip = BinStrip::of(&dressed);
        for row in BIN_ACTIONS {
            if !(row.when)(&strip) {
                continue;
            }
            let mut state = (stage.dress)();
            let before = shape(&state);
            let watch = Watch::on(&state);
            let facts = BinStrip::of(&state);
            (row.run)(&mut state, &facts).expect("the press lands");
            let sent = watch.take();
            finish_any_field(&mut state);
            assert!(
                !sent.is_empty() || shape(&state) != before,
                "the strip's {} button did nothing on {}",
                row.id,
                stage.what
            );
        }
    }
}

#[test]
fn the_root_offers_only_the_button_that_makes_a_folder() {
    // Renaming or deleting the pool root is not an edit the project has, and a
    // button that reports a refusal is worse than one that is not drawn.
    let state = at_the_root();
    let strip = BinStrip::of(&state);
    let offered: Vec<&str> = BIN_ACTIONS
        .iter()
        .filter(|row| (row.when)(&strip))
        .map(|row| row.id)
        .collect();
    assert_eq!(offered, ["bin-new"], "the root strip offers the wrong rows");
}

#[test]
fn every_bin_button_has_its_own_element_id() {
    // The strip draws them side by side, so a shared id is a click that lands on
    // the wrong button.
    let ids: BTreeSet<&str> = BIN_ACTIONS.iter().map(|row| row.id).collect();
    assert_eq!(ids.len(), BIN_ACTIONS.len(), "two buttons share an id");
}

#[test]
fn a_new_folder_opens_with_the_caret_in_its_name() {
    // A folder called "New bin" is not why anyone made one, so the button that
    // makes one has to leave the operator typing.
    let mut state = at_the_root();
    let strip = BinStrip::of(&state);
    let row = BIN_ACTIONS
        .iter()
        .find(|row| row.id == "bin-new")
        .expect("the strip offers a way to make a folder");
    (row.run)(&mut state, &strip).expect("the folder lands");
    let field = crate::keys::fields::focused(&state).expect("no field took the caret");
    assert_eq!(field.id, "aria.field.bin-rename");
    assert_eq!(
        (field.text)(&state),
        crate::app::bins::DEFAULT_BIN_NAME,
        "the field did not open on the new folder's name"
    );
    assert_eq!(
        state.current_bin,
        state.store.bins.last().map(|item| item.id),
        "the new folder is not the open one"
    );
}

#[test]
fn deleting_the_open_folder_steps_up_to_its_parent() {
    // Deleting the folder you are inside has to leave you somewhere: a pool
    // pointed at a folder that is gone shows an empty list with no way back.
    let mut state = inside_a_folder();
    let parent = state.current_bin.expect("a folder is open");
    state.create_bin().expect("the child folder lands");
    state.cancel_bin_rename();
    let child = state.current_bin.expect("the child is open");
    assert_ne!(child, parent, "the child folder did not open");

    let row = BIN_ACTIONS
        .iter()
        .find(|row| row.id == "bin-delete")
        .expect("the strip offers a way to delete a folder");
    let facts = BinStrip::of(&state);
    (row.run)(&mut state, &facts).expect("the delete lands");
    assert_eq!(
        state.current_bin,
        Some(parent),
        "deleting the open folder did not step up to its parent"
    );
    assert!(
        !state.store.bins.iter().any(|item| item.id == child),
        "the deleted folder is still in the tree"
    );
    assert_eq!(state.bin_rename, None, "a name field survived the delete");
}

#[test]
fn the_strip_is_built_from_the_registry() {
    // The census presses `BinAction::run`. That measures the strip only while the
    // strip draws those rows and calls the same functions, and while the field
    // this census commits by hand is the one the field row submits.
    //
    // Not covered: where the buttons sit, or that they are drawn at all when the
    // panel is not on screen. The picture is the panel's.
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for (file, needles) in [
        (
            "panels/sidebar_bins.rs",
            &["for action in BIN_ACTIONS", ".children(buttons)"][..],
        ),
        ("keys/fields.rs", &["state.commit_bin_rename()"][..]),
    ] {
        let drawn = std::fs::read_to_string(src.join(file))
            .unwrap_or_else(|err| panic!("{file} is readable: {err}"));
        for needle in needles {
            assert!(
                drawn.contains(needle),
                "{file} has no `{needle}`, so the census measures a path it does not run"
            );
        }
    }
}
