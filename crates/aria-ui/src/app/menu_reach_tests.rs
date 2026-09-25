//! What the menu bar actually sends, measured rather than asserted by name.
//!
//! Seventy-nine menu rows were untestable by construction: each one takes a
//! `&mut gpui::Context<AppState>`, and nothing outside a running window builds
//! one, so the bar was the largest operator surface in the product with no
//! coverage at all. `gpui`'s `test-support` feature is a dev-dependency for
//! exactly this: `TestAppContext` holds a headless `App`, `App::new` gives the
//! editor state an entity, and `Entity::update` hands the row the context it
//! wants.
//!
//! Every row is declared below as one of three kinds, every drivable row is run
//! against a dressed editor state, and the history it writes is compared with
//! what it claims. The table is checked against the live menu in both
//! directions, so a row added without a decision turns the suite red instead of
//! shipping as a control that sends nothing.
//!
//! What this does not catch, stated plainly:
//!
//! - A row that asks the operator for a path is driven with a supplied answer,
//!   not with a dialog: see `PICK_ROWS` and the tests under it. What a native
//!   dialog itself does is not observed here.
//! - Rows are driven through the entity, not through a drawn window, so what a
//!   row asks the window to do — a redraw, a focus move — is not observed here.
//!
//! Each row also declares the state it is driven against, because a row run
//! against a fixture that cannot offer it is inert, and an inert row reads
//! exactly like a broken one. See [`Dress`].

use super::AppState;
use super::picker::Answer;
use super::premiere_tests::with_clip;
use crate::chrome::menu_items::items_of;
use crate::types::AppMenu;
use aria_commands::every_edit_command;
use gpui::AppContext;
use menu_reach_dress::{Answered, Dress};
use std::collections::BTreeSet;
/// What a row is expected to do when it is run against the state it declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Writes at least one history entry.
    Edits,
    /// Writes none: it moves the view, opens a route or a dialog, writes to
    /// disk, or asks the operator for a path.
    NoEdit,
}
use Dress::{
    Baked, Blended, Clip, Copied, Cut, Fitted, Inside, Linked, Marked, Nest, Pair, Rendered, Sound,
    Source,
};
use Kind::{Edits, NoEdit};
/// Every menu row, what it does, and the state it is driven against. Checked
/// against the live menus, so a new row fails the suite until someone decides
/// both columns for it.
const ROWS: &[(&str, Kind, Dress)] = &[
    // Sends a command against one selected clip with the playhead inside it.
    ("edit-cut", Edits, Clip),
    ("edit-dup", Edits, Clip),
    ("edit-split", Edits, Clip),
    ("edit-ripple", Edits, Clip),
    ("seq-v", Edits, Clip),
    ("seq-a", Edits, Clip),
    ("seq-adjustment", Edits, Clip),
    ("seq-marker", Edits, Clip),
    ("seq-ripple-in", Edits, Clip),
    ("seq-ripple-out", Edits, Clip),
    ("seq-flip-h", Edits, Clip),
    ("seq-flip-v", Edits, Clip),
    ("seq-crop-reset", Edits, Clip),
    ("seq-grade-reset", Edits, Clip),
    ("seq-mask-clear", Edits, Clip),
    ("seq-chroma-clear", Edits, Clip),
    ("seq-matte-luma", Edits, Clip),
    ("seq-matte-alpha", Edits, Clip),
    ("seq-matte-clear", Edits, Clip),
    // Acts on state the fixture has to be dressed with first: a copy on the
    // clipboard, a marker under the playhead, a second clip to link to, a
    // junction, a source range, a nest, or a render already on disk.
    ("edit-paste", Edits, Copied),
    ("edit-paste-insert", Edits, Copied),
    ("edit-paste-attributes", Edits, Copied),
    ("edit-paste-fit", Edits, Fitted),
    ("seq-source-insert", Edits, Source),
    ("seq-source-overwrite", Edits, Source),
    ("seq-ripple-insert", Edits, Source),
    ("seq-marker-remove", Edits, Marked),
    ("seq-link", Edits, Pair),
    ("seq-unlink", Edits, Linked),
    ("seq-extract-audio", Edits, Sound),
    ("seq-freeze", Edits, Sound),
    ("seq-transition-crossfade", Edits, Cut),
    ("seq-transition-clear", Edits, Blended),
    ("seq-nest", Edits, Clip),
    ("seq-nest-open", Edits, Nest),
    ("seq-nest-leave", Edits, Inside),
    ("seq-nest-place", Edits, Nest),
    ("seq-bake-clip", Edits, Rendered),
    ("seq-bake-restore", Edits, Baked),
    // Moves the view, opens a route or a dialog, toggles a panel, or writes to
    // disk. Undo and redo belong here too: they move the history pointer rather
    // than adding to it.
    ("file-new", NoEdit, Clip),
    ("file-home", NoEdit, Clip),
    ("file-save", NoEdit, Clip),
    ("file-rename", NoEdit, Clip),
    ("file-export", NoEdit, Clip),
    ("file-close", NoEdit, Clip),
    ("edit-undo", NoEdit, Clip),
    ("edit-redo", NoEdit, Clip),
    ("edit-copy", NoEdit, Clip),
    ("edit-select-all", NoEdit, Clip),
    ("edit-deselect", NoEdit, Clip),
    ("seq-fit", NoEdit, Clip),
    ("seq-fit-selection", NoEdit, Clip),
    ("seq-in", NoEdit, Clip),
    ("seq-out", NoEdit, Clip),
    ("seq-mark-in", NoEdit, Clip),
    ("seq-mark-out", NoEdit, Clip),
    ("seq-clear-in", NoEdit, Clip),
    ("seq-clear-out", NoEdit, Clip),
    // Both move the playhead to a marker, which is a view move whether or not
    // there is one to move to, so each is driven against the marker anyway.
    ("seq-marker-next", NoEdit, Marked),
    ("seq-marker-prev", NoEdit, Marked),
    // Loads the selected clip's source into the monitor: a view move.
    ("seq-match-frame", NoEdit, Clip),
    // Opens a text field over the marker under the playhead. The field's own
    // commit is covered where the field is.
    ("seq-marker-rename", NoEdit, Marked),
    ("view-sidebar", NoEdit, Clip),
    ("view-inspector", NoEdit, Clip),
    ("view-ai", NoEdit, Clip),
    ("view-mixer", NoEdit, Clip),
    ("view-fullscreen", NoEdit, Clip),
    ("view-title-safe", NoEdit, Clip),
    ("view-action-safe", NoEdit, Clip),
    ("view-grid", NoEdit, Clip),
    ("view-scopes", NoEdit, Clip),
    ("view-zoom-fit", NoEdit, Clip),
    ("view-zoom-actual", NoEdit, Clip),
    ("view-handles", NoEdit, Clip),
    ("view-settings", NoEdit, Clip),
    ("win-home", NoEdit, Clip),
    ("win-palette", NoEdit, Clip),
    ("win-jobs", NoEdit, Clip),
    ("win-history", NoEdit, Clip),
    ("help-learn", NoEdit, Clip),
    ("help-data", NoEdit, Clip),
    ("help-updates", NoEdit, Clip),
    // Asks the operator for a path. With no answer supplied the row asks and
    // stops, which is why it writes nothing here; each is driven with an answer
    // by a test at the foot of this file.
    ("file-open", NoEdit, Clip),
    ("file-save-as", NoEdit, Clip),
    ("file-save-template", NoEdit, Clip),
    ("file-use-template", NoEdit, Clip),
    ("file-import", NoEdit, Clip),
    ("file-relink-all", NoEdit, Clip),
    // Opens the project report, which reads facts and writes nothing.
    ("win-project-report", NoEdit, Clip),
    // Arranges the panes and saves the arrangement. Neither touches the project,
    // so neither writes history. Declared row by row rather than generated from
    // `layouts::WORKSPACES`: this table exists to make a new row someone's
    // decision, and a generated declaration would classify it silently.
    ("win-workspace-editing", NoEdit, Clip),
    ("win-workspace-colour", NoEdit, Clip),
    ("win-workspace-audio", NoEdit, Clip),
    ("win-workspace-effects", NoEdit, Clip),
    ("win-workspace-save", NoEdit, Clip),
    // Sequence rows that share a helper, and an exact label, with a palette row
    // already declared in `reach_tests.rs`. Same code path against the same
    // fixture, so the classification is the twin's rather than a fresh guess.
    ("seq-canvas-background", NoEdit, Clip),
    ("seq-duck-amount", NoEdit, Clip),
    ("seq-duck-attack", NoEdit, Clip),
    ("seq-duck-release", NoEdit, Clip),
    ("seq-effect-reset", NoEdit, Clip),
    ("seq-lane-shift-earlier", NoEdit, Clip),
    ("seq-lane-shift-later", Edits, Clip),
    ("seq-marker-snap", NoEdit, Clip),
];
/// What a row does and what it is driven against.
fn row_of(id: &str) -> Option<(Kind, Dress)> {
    ROWS.iter()
        .find(|(row, ..)| *row == id)
        .map(|(_, kind, dress)| (*kind, *dress))
}
/// Every row the bar would draw, across all six menus, as (menu, id).
fn live_rows() -> Vec<(AppMenu, String)> {
    let (state, _) = with_clip();
    let mut out = Vec::new();
    for menu in AppMenu::ALL {
        for item in items_of(menu, &state) {
            out.push((menu, item.id.to_string()));
        }
    }
    out
}
/// Run one menu row against a fresh state and report the history labels it
/// added, newest last.
fn labels_from(menu: AppMenu, id: &str, dress: Dress) -> Vec<String> {
    driven(menu, id, dress).0
}
/// Run one menu row against the state it declares and report both the history
/// labels it added and every command it dispatched: a row that saves, exports or
/// opens asks for a command and writes no history, so the surface census reads
/// the second.
fn driven(menu: AppMenu, id: &str, dress: Dress) -> (Vec<String>, Vec<&'static str>) {
    dispatch(menu, id, dress.state())
}
/// The same, against a state the caller built: a row that asks for a path is
/// driven against one carrying the answer.
fn dispatch(menu: AppMenu, id: &str, state: AppState) -> (Vec<String>, Vec<&'static str>) {
    let cx = gpui::TestAppContext::single();
    let before = history_labels(&state);
    let watch = super::sent_census::Watch::on(&state);
    let entity = cx.update(|app| app.new(|_| state));
    let found = cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            // The item list is read from the immutable state, then the row runs
            // against the mutable one: two borrows, in order.
            let run = items_of(menu, &*state)
                .into_iter()
                .find(|it| it.id == id)
                .map(|it| it.run);
            let Some(run) = run else {
                return false;
            };
            run(id, state, cx);
            true
        })
    });
    assert!(found, "{id} vanished between enumeration and running");
    let after = cx.update(|app| history_labels(entity.read(app)));
    (after.into_iter().skip(before.len()).collect(), watch.take())
}
fn history_labels(state: &AppState) -> Vec<String> {
    state
        .engine
        .lock()
        .history_entries()
        .into_iter()
        .map(|entry| entry.label.into_owned())
        .collect()
}
/// Every command label the drivable menu rows produced, measured once.
///
/// Shared with the operator-surface census: the bar is one of the surfaces a
/// command may be reached from.
pub(super) fn measured() -> &'static BTreeSet<String> {
    &CENSUS.0
}
/// Every command the bar's rows dispatched, whether or not it edited.
pub(super) fn dispatched() -> &'static BTreeSet<&'static str> {
    &CENSUS.1
}
/// The bar's census: history labels, then commands sent, from one pass.
static CENSUS: std::sync::LazyLock<(BTreeSet<String>, BTreeSet<&'static str>)> =
    std::sync::LazyLock::new(|| {
        let mut labels = BTreeSet::new();
        let mut sent = BTreeSet::new();
        let rows = live_rows();
        for (menu, id) in &rows {
            // A row with no declaration is named by
            // `the_table_names_every_menu_row_and_nothing_else`; measuring it
            // against the plain clip here would hide it behind a second failure.
            let Some((_, dress)) = row_of(id) else {
                continue;
            };
            let (wrote, asked) = driven(*menu, id, dress);
            labels.extend(wrote);
            sent.extend(asked);
        }
        // A row that opens a file dialog is driven a second time, answered. The
        // menu it lives on is read off the live bar rather than written here
        // twice.
        for (id, answered) in PICK_ROWS {
            let Some((menu, _)) = rows.iter().find(|(_, live)| live == id) else {
                continue;
            };
            let (wrote, asked) = dispatch(*menu, id, answered.state().0);
            labels.extend(wrote);
            sent.extend(asked);
        }
        (labels, sent)
    });
/// Every menu row that asks the operator for a path, with the state and the
/// answer it is driven with. Checked against the live menus at run time by
/// `every_row_that_asks_for_a_path_is_driven_with_an_answer`.
const PICK_ROWS: &[(&str, Answered)] = &[
    ("file-open", Answered::Open),
    ("file-save-as", Answered::SaveAs),
    ("file-save-template", Answered::SaveTemplate),
    ("file-use-template", Answered::UseTemplate),
    ("file-import", Answered::Import),
    ("file-relink-all", Answered::RelinkAll),
];
fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}
/// A scratch dir of this name's own.
///
/// The name goes in the prefix rather than under a shared dir: claiming a scratch
/// dir empties it, and several of these are held live at once.
fn scratch(name: &str) -> std::path::PathBuf {
    crate::scratch_pool::scratch(&format!("menu-pick-{name}"))
}
/// A scratch dir no other call holds, whatever it is named.
///
/// A dressing is built twice in one process: once by the census, and once by the
/// test that asserts what the row did with it. Claiming a scratch dir empties it,
/// so two builds of one dressing must not name the same dir.
pub(super) fn own_scratch(name: &str) -> std::path::PathBuf {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    scratch(&format!("{name}-{n}"))
}
fn clip_count(state: &AppState) -> usize {
    state
        .store
        .sequence
        .tracks
        .iter()
        .map(|track| track.clips.len())
        .sum()
}
/// The fixture state with its edits saved, so the unsaved-changes prompt does
/// not stand between the row and the pick.
fn saved() -> AppState {
    let (mut state, _) = with_clip();
    state.project_path = Some(scratch("saved").join("open.aria"));
    state.save_project().expect("save the fixture project");
    state
}
fn entity_of(state: AppState, cx: &gpui::TestAppContext) -> gpui::Entity<AppState> {
    cx.update(|app| app.new(|_| state))
}
/// Run one row against a state that is already an entity, so a test can act
/// before it and read after it.
fn run_row(entity: &gpui::Entity<AppState>, cx: &gpui::TestAppContext, menu: AppMenu, id: &str) {
    let found = cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            let run = items_of(menu, &*state)
                .into_iter()
                .find(|item| item.id == id)
                .map(|item| item.run);
            let Some(run) = run else {
                return false;
            };
            run(id, state, cx);
            true
        })
    });
    assert!(found, "{id} is not on offer against this fixture");
}

// The state each row is driven against. Its own file: a dressing is product
// knowledge about what a row acts on, and it is longer than the table.
#[path = "menu_reach_dress.rs"]
mod menu_reach_dress;

// The cases live in sibling files, declared here so the fixtures above stay in
// one place. Only `#[test]` fns moved.
#[path = "menu_reach_tests_operator.rs"]
mod menu_reach_tests_operator;
#[path = "menu_reach_tests_row.rs"]
mod menu_reach_tests_row;
