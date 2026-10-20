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
    Baked, Blended, Clip, Copied, Cut, Edged, EffectCopied, Effected, Fitted, Grouped, Inside,
    Keyed, Labelled, Lanes, Linked, Marked, Masked, Nest, Pair, Picked, Ranged, Rendered, Sound,
    Source, Sourced, Spanned,
};
use Kind::{Edits, NoEdit};
/// Every menu row, what it does, and the state it is driven against. Checked
/// against the live menus, so a new row fails the suite until someone decides
/// both columns for it.
const ROWS: &[(&str, Kind, Dress)] = &[
    // Sends a command against one selected clip with the playhead inside it.
    ("edit-cut", Edits, Clip),
    ("edit-dup", Edits, Clip),
    ("edit-ripple", Edits, Clip),
    ("edit-clear", Edits, Clip),
    ("clip-enable", Edits, Clip),
    ("clip-time-remap", Edits, Clip),
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
    ("seq-chroma-clear", Edits, Clip),
    ("seq-matte-luma", Edits, Clip),
    ("seq-matte-alpha", Edits, Clip),
    ("seq-matte-clear", Edits, Clip),
    // Acts on state the fixture has to be dressed with first: a copy on the
    // clipboard, a marker under the playhead, a second clip to link to, a
    // junction, a source range, a nest, a mask, or a render already on disk.
    ("seq-mask-clear", Edits, Masked),
    ("edit-paste", Edits, Copied),
    ("edit-paste-insert", Edits, Copied),
    ("edit-paste-attributes", Edits, Copied),
    // Opens the dialog; its Remove button commits the edit.
    ("edit-remove-attributes", NoEdit, Effected),
    ("edit-paste-fit", Edits, Fitted),
    ("seq-source-insert", Edits, Source),
    ("seq-source-overwrite", Edits, Source),
    ("seq-ripple-insert", Edits, Source),
    ("seq-marker-remove", Edits, Marked),
    ("seq-link", Edits, Pair),
    ("seq-unlink", Edits, Linked),
    ("clip-group", Edits, Pair),
    ("clip-ungroup", Edits, Grouped),
    ("seq-extract-audio", Edits, Sound),
    ("seq-freeze", Edits, Sound),
    ("seq-transition-crossfade", Edits, Cut),
    ("seq-transition-clear", Edits, Blended),
    ("seq-nest", Edits, Clip),
    ("file-new-sequence", Edits, Clip),
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
    ("file-quit", NoEdit, Clip),
    ("edit-undo", NoEdit, Clip),
    ("edit-redo", NoEdit, Clip),
    ("edit-copy", NoEdit, Clip),
    ("edit-select-all", NoEdit, Clip),
    ("edit-deselect", NoEdit, Clip),
    ("clip-speed-duration", NoEdit, Clip),
    ("clip-audio-gain", NoEdit, Sound),
    // Stereo removes a channel tool; the dressing carries none.
    ("clip-channels-stereo", NoEdit, Sound),
    ("clip-channels-swap", Edits, Sound),
    ("clip-channels-left", Edits, Sound),
    ("clip-channels-right", Edits, Sound),
    ("clip-channels-mono", Edits, Sound),
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
    ("view-fullscreen", NoEdit, Clip),
    ("view-title-safe", NoEdit, Clip),
    ("view-action-safe", NoEdit, Clip),
    ("view-grid", NoEdit, Clip),
    ("view-rulers", NoEdit, Clip),
    ("view-guide-snap", NoEdit, Clip),
    ("view-transparency-grid", NoEdit, Clip),
    ("view-guides-show", NoEdit, Clip),
    ("seq-linked-selection", NoEdit, Clip),
    ("view-guides-clear", NoEdit, Clip),
    ("view-split", NoEdit, Clip),
    // Holds a reference picture beside the Program frame, or sets how it is
    // drawn: the operator's view, not the project.
    ("view-compare-frame", NoEdit, Clip),
    ("view-compare-still", NoEdit, Clip),
    ("view-compare-source", NoEdit, Source),
    ("view-compare-vertical", NoEdit, Clip),
    ("view-compare-horizontal", NoEdit, Clip),
    ("view-compare-side", NoEdit, Clip),
    ("view-compare-stop", NoEdit, Clip),
    ("view-scopes", NoEdit, Clip),
    ("view-waveform-luma", NoEdit, Clip),
    ("view-waveform-rgb", NoEdit, Clip),
    ("view-waveform-ycbcr", NoEdit, Clip),
    ("view-zoom-fit", NoEdit, Clip),
    ("view-zoom-actual", NoEdit, Clip),
    ("view-handles-transform", NoEdit, Clip),
    ("view-handles-crop", NoEdit, Clip),
    ("view-handles-mask", NoEdit, Clip),
    ("view-playback-resolution", NoEdit, Clip),
    ("view-paused-resolution", NoEdit, Clip),
    ("view-proxies", NoEdit, Clip),
    ("view-loop", NoEdit, Clip),
    ("view-thumbs-head", NoEdit, Clip),
    ("view-thumbs-head-tail", NoEdit, Clip),
    ("view-thumbs-continuous", NoEdit, Clip),
    ("view-waveform-log", NoEdit, Clip),
    ("view-waveform-linear", NoEdit, Clip),
    ("view-waveform-hidden", NoEdit, Clip),
    ("view-follow-page", NoEdit, Clip),
    ("view-follow-smooth", NoEdit, Clip),
    ("win-home", NoEdit, Clip),
    ("win-palette", NoEdit, Clip),
    ("win-jobs", NoEdit, Clip),
    ("help-learn", NoEdit, Clip),
    ("help-data", NoEdit, Clip),
    ("help-about", NoEdit, Clip),
    ("help-updates", NoEdit, Clip),
    // Asks the operator for a path. With no answer supplied the row asks and
    // stops, which is why it writes nothing here; each is driven with an answer
    // by a test at the foot of this file.
    ("file-open", NoEdit, Clip),
    ("file-save-as", NoEdit, Clip),
    ("file-save-copy", NoEdit, Clip),
    ("file-save-template", NoEdit, Clip),
    ("file-use-template", NoEdit, Clip),
    ("file-import", NoEdit, Clip),
    ("file-import-recent-clear", NoEdit, Clip),
    ("file-import-timeline", NoEdit, Clip),
    ("file-export-timeline", NoEdit, Clip),
    ("file-export-frame", NoEdit, Clip),
    ("file-install-plugin", NoEdit, Clip),
    ("file-relink-all", NoEdit, Clip),
    // Opens the project report, which reads facts and writes nothing.
    ("win-project-report", NoEdit, Clip),
    // Arranges the panes and saves the arrangement. Neither touches the project,
    // so neither writes history. Declared row by row rather than generated from
    // `layouts::WORKSPACES`: this table exists to make a new row someone's
    // decision, and a generated declaration would classify it silently.
    ("win-workspace-editing", NoEdit, Clip),
    ("win-workspace-animation", NoEdit, Clip),
    ("win-workspace-colour", NoEdit, Clip),
    ("win-workspace-audio", NoEdit, Clip),
    ("win-workspace-effects", NoEdit, Clip),
    ("win-workspace-save", NoEdit, Clip),
    ("win-workspace-reset", NoEdit, Clip),
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
    ("seq-render-in-to-out", NoEdit, Clip),
    ("seq-render-selection", NoEdit, Clip),
    ("clip-analyze:aria.beats", NoEdit, Sound),
    ("seq-analyze:aria.silence", NoEdit, Clip),
    ("seq-analyze:aria.loudness", NoEdit, Clip),
    // Marks and the view: In and Out are session state, not project state.
    ("mark-clip", NoEdit, Clip),
    ("mark-go-in", NoEdit, Clip),
    ("mark-go-out", NoEdit, Clip),
    ("mark-clear", NoEdit, Clip),
    ("marker-clear-all", Edits, Marked),
    ("fx-controls", NoEdit, Clip),
    ("anim-reveal-keyed", NoEdit, Clip),
    ("anim-reveal-modified", NoEdit, Clip),
    ("anim-keyframe-velocity", NoEdit, Keyed),
    ("anim-graph-editor", NoEdit, Clip),
    ("seq-delete-render-files", NoEdit, Clip),
    // Cuts at the playhead, inside the clip.
    ("seq-add-edit", Edits, Clip),
    ("seq-add-edit-all", Edits, Clip),
    // A label is a project edit; clearing one needs a label to clear.
    ("edit-label-violet", Edits, Clip),
    ("edit-label-iris", Edits, Clip),
    ("edit-label-caribbean", Edits, Clip),
    ("edit-label-lavender", Edits, Clip),
    ("edit-label-cerulean", Edits, Clip),
    ("edit-label-forest", Edits, Clip),
    ("edit-label-rose", Edits, Clip),
    ("edit-label-mango", Edits, Clip),
    ("edit-label-purple", Edits, Clip),
    ("edit-label-blue", Edits, Clip),
    ("edit-label-teal", Edits, Clip),
    ("edit-label-magenta", Edits, Clip),
    ("edit-label-tan", Edits, Clip),
    ("edit-label-green", Edits, Clip),
    ("edit-label-brown", Edits, Clip),
    ("edit-label-yellow", Edits, Clip),
    ("edit-label-none", Edits, Labelled),
    // Each sets the interpolation of the keyframe under the playhead.
    ("anim-toggle-hold", Edits, Keyed),
    ("anim-interp-linear", Edits, Keyed),
    ("anim-interp-bezier", Edits, Keyed),
    ("anim-interp-hold", Edits, Keyed),
    ("anim-easy-ease", Edits, Keyed),
    ("anim-easy-ease-in", Edits, Keyed),
    ("anim-easy-ease-out", Edits, Keyed),
    ("anim-add-keyframe", Edits, Picked),
    ("anim-add-expression", Edits, Picked),
    ("anim-time-reverse", Edits, Spanned),
    ("fx-last", Edits, Effected),
    ("fx-remove-all", Edits, Effected),
    ("fx-paste", Edits, EffectCopied),
    ("seq-delete-tracks", Edits, Lanes),
    ("seq-extend-edit", Edits, Edged),
    ("seq-lift", Edits, Ranged),
    ("seq-extract", Edits, Ranged),
    ("seq-default-transitions", Edits, Cut),
    ("clip-subclip", Edits, Source),
    ("clip-replace-source", Edits, Sourced),
    // Selects an edit point, which is session state.
    ("seq-trim-edit", NoEdit, Clip),
    // A toggle of the editor's own state.
    ("seq-snap", NoEdit, Clip),
    // Reverts to the saved file; the fixture was never saved, so the row
    // reports that and stops. Driven against a saved project by its own test.
    ("file-revert", NoEdit, Clip),
    // Open a field, a dialog, a pane, or a page.
    ("clip-rename", NoEdit, Clip),
    ("edit-find", NoEdit, Clip),
    ("edit-keyboard-shortcuts", NoEdit, Clip),
    ("edit-preferences", NoEdit, Clip),
    ("file-project-settings", NoEdit, Clip),
    ("seq-settings", NoEdit, Clip),
    ("help-plugins", NoEdit, Clip),
    ("win-media", NoEdit, Clip),
    ("win-effects", NoEdit, Clip),
    ("win-program", NoEdit, Clip),
    ("win-timeline", NoEdit, Clip),
    ("win-inspector", NoEdit, Clip),
    ("win-assistant", NoEdit, Clip),
    ("win-mixer", NoEdit, Clip),
];
/// What a row does and what it is driven against.
fn row_of(id: &str) -> Option<(Kind, Dress)> {
    by_prefix(id).or_else(|| {
        ROWS.iter()
            .find(|(row, ..)| *row == id)
            .map(|(_, kind, dress)| (*kind, *dress))
    })
}
/// Rows generated one per catalog entry or registered dock panel, declared by
/// their id prefix: every effect the catalogs list and every panel the host
/// holds is offered, so the table cannot name them one by one without going
/// stale when an entry is added.
fn by_prefix(id: &str) -> Option<(Kind, Dress)> {
    use crate::chrome::menu_effect::{APPLY_AUDIO_PREFIX, APPLY_PREFIX};
    if id.starts_with(APPLY_AUDIO_PREFIX) {
        Some((Edits, Sound))
    } else if id.starts_with(APPLY_PREFIX) {
        Some((Edits, Clip))
    } else if id.starts_with(crate::chrome::menu_items::PANEL_PREFIX) {
        Some((NoEdit, Clip))
    } else {
        None
    }
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
/// One menu row's outcome when run against its declared dressing.
///
/// # Why
///
/// Every `Dress::state()` constructs an `AppState` and a headless `Renderer`
/// (`~240 ms` wall and `66` Lavapipe threads per call). Recording each row's
/// history labels, disabled-guard reading, form state, and file-picker request
/// during the single `CENSUS` pass lets `menu_reach_tests_row` and
/// `menu_reach_tests_verbs` check every row's contract without repeating four
/// full-menu passes (`~800` redundant `AppState::new` calls).
struct RowOutcome {
    id: String,
    kind: Kind,
    dress: Dress,
    wrote: Vec<String>,
    blocked: Option<&'static str>,
    opened_form: bool,
}

struct Probe {
    wrote: Vec<String>,
    asked: Vec<&'static str>,
    blocked: Option<&'static str>,
    opened_form: bool,
    asked_pick: bool,
}

struct MenuCensus {
    labels: BTreeSet<String>,
    sent: BTreeSet<&'static str>,
    rows: Vec<RowOutcome>,
    asking_picks: Vec<String>,
}

/// Run one menu row against a state the caller built: a row that asks for a
/// path is driven against one carrying the answer, and a row that opens a form
/// is driven with Enter pressed on the form (`enter`).
fn dispatch(
    menu: AppMenu,
    id: &str,
    state: AppState,
    enter: bool,
) -> (Vec<String>, Vec<&'static str>) {
    let probe = probe(menu, id, state, enter);
    (probe.wrote, probe.asked)
}

fn probe(menu: AppMenu, id: &str, state: AppState, enter: bool) -> Probe {
    let cx = gpui::TestAppContext::single();
    let before = history_labels(&state);
    let watch = super::sent_census::Watch::on(&state);
    let entity = cx.update(|app| app.new(|_| state));
    let (found, blocked) = cx.update(|app| {
        entity.update(app, |state: &mut AppState, cx| {
            // The item list is read from the immutable state, then the row runs
            // against the mutable one: two borrows, in order.
            let item = items_of(menu, &*state).into_iter().find(|it| it.id == id);
            let Some(item) = item else {
                return (false, None);
            };
            let blocked = item.blocked_by(&*state);
            let run = item.run;
            run(id, state, cx);
            if enter {
                assert!(
                    state.run_dialog_primary(),
                    "{id} opened no form Enter commits"
                );
            }
            (true, blocked)
        })
    });
    assert!(found, "{id} vanished between enumeration and running");
    let (after, opened_form, asked_pick) = cx.update(|app| {
        let state = entity.read(app);
        let opened_form = matches!(
            crate::app::dialog_primary::primary(state.dialog),
            crate::app::dialog_primary::Primary::Run(_)
        );
        (history_labels(state), opened_form, state.picks_asked() > 0)
    });
    Probe {
        wrote: after.into_iter().skip(before.len()).collect(),
        asked: watch.take(),
        blocked,
        opened_form,
        asked_pick,
    }
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
    &CENSUS.labels
}
/// Every command the bar's rows dispatched, whether or not it edited.
pub(super) fn dispatched() -> &'static BTreeSet<&'static str> {
    &CENSUS.sent
}
/// The bar's census: history labels, commands sent, and per-row outcomes from
/// one pass.
static CENSUS: std::sync::LazyLock<MenuCensus> = std::sync::LazyLock::new(|| {
    let mut labels = BTreeSet::new();
    let mut sent = BTreeSet::new();
    let mut row_outcomes = Vec::new();
    let mut asking_picks = Vec::new();
    let rows = live_rows();
    for (menu, id) in &rows {
        // A row with no declaration is named by
        // `the_table_names_every_menu_row_and_nothing_else`; measuring its
        // labels against the plain clip here would hide it behind a second
        // failure, though a path pick is still recorded so an undeclared
        // picking row goes red in `every_row_that_asks_for_a_path_is_driven_with_an_answer`.
        let Some((kind, dress)) = row_of(id) else {
            if probe(*menu, id, with_clip().0, false).asked_pick {
                asking_picks.push(id.clone());
            }
            continue;
        };
        let outcome = probe(*menu, id, dress.state(), false);
        if outcome.asked_pick {
            asking_picks.push(id.clone());
        }
        labels.extend(outcome.wrote.iter().cloned());
        sent.extend(outcome.asked);
        row_outcomes.push(RowOutcome {
            id: id.clone(),
            kind,
            dress,
            wrote: outcome.wrote,
            blocked: outcome.blocked,
            opened_form: outcome.opened_form,
        });
    }
    // A row that opens a file dialog is driven a second time, answered. The
    // menu it lives on is read off the live bar rather than written here
    // twice.
    for (id, answered) in PICK_ROWS {
        let Some((menu, _)) = rows.iter().find(|(_, live)| live == id) else {
            continue;
        };
        let (wrote, asked) = dispatch(*menu, id, answered.state().0, false);
        labels.extend(wrote);
        sent.extend(asked);
    }
    // A row that opens a form is driven a second time with the form
    // committed, which is the press that sends its command.
    for (id, dress) in FORM_ROWS {
        let Some((menu, _)) = rows.iter().find(|(_, live)| live == id) else {
            continue;
        };
        let (wrote, asked) = dispatch(*menu, id, dress.state(), true);
        labels.extend(wrote);
        sent.extend(asked);
    }
    MenuCensus {
        labels,
        sent,
        rows: row_outcomes,
        asking_picks,
    }
});
/// Every menu row that asks the operator for a path, with the state and the
/// answer it is driven with. Checked against the live menus at run time by
/// `every_row_that_asks_for_a_path_is_driven_with_an_answer`.
const PICK_ROWS: &[(&str, Answered)] = &[
    ("file-open", Answered::Open),
    ("file-save-as", Answered::SaveAs),
    ("file-save-copy", Answered::SaveCopy),
    ("file-save-template", Answered::SaveTemplate),
    ("file-use-template", Answered::UseTemplate),
    ("file-import", Answered::Import),
    ("file-relink-all", Answered::RelinkAll),
    ("file-import-timeline", Answered::ImportTimeline),
    ("file-export-timeline", Answered::ExportTimeline),
    ("file-export-frame", Answered::ExportFrame),
    ("file-install-plugin", Answered::InstallPlugin),
    ("view-compare-still", Answered::CompareStill),
];
/// Every menu row that opens a form Enter commits, with the state it is driven
/// against. Checked against the live menus at run time by
/// `every_row_that_opens_a_form_is_committed`.
const FORM_ROWS: &[(&str, Dress)] = &[
    ("edit-remove-attributes", Effected),
    ("clip-speed-duration", Clip),
    ("clip-audio-gain", Sound),
    ("seq-marker-rename", Marked),
    ("anim-keyframe-velocity", Keyed),
];
/// Rows that open a dialog with a primary action the census does not press, and
/// why.
pub(super) const FORM_EXEMPT: &[(&str, &str)] = &[
    (
        "file-export",
        "the sheet's Export writes a file and starts a job, which the export tests drive",
    ),
    ("file-new", LEAVES),
    ("file-open", LEAVES),
    ("file-home", LEAVES),
    ("file-close", LEAVES),
    ("file-quit", LEAVES),
    ("win-home", LEAVES),
    ("help-learn", LEAVES),
];
/// Why a row that leaves the project is exempt from the form census.
const LEAVES: &str = "leaving a project with unsaved edits opens the Unsaved Changes prompt, \
                      whose Save, Discard, and Cancel unsaved_tests drives";
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
#[path = "menu_reach_tests_verbs.rs"]
mod menu_reach_tests_verbs;
