//! What the palette actually sends, measured rather than asserted by name.
//!
//! The class closed here is "a control exists and sends nothing". Every palette
//! row is declared below as one of three kinds, every row is run against a
//! dressed editor state, and the history it writes is compared with what it
//! claims. A row that stops sending its command goes red; so does a row that is
//! added without a decision, because the table is checked against the live
//! command list in both directions.
//!
//! `aria_commands::Reach` states the same thing from the other side: whether an
//! operator can ask for a command at all. That is a compile-time declaration in
//! the crate that owns the commands, which cannot see a palette row, so the two
//! meet here.
//!
//! What this does not catch, stated plainly:
//!
//! - Windowed rows (`BUILTIN_JOB_COMMANDS`) need the entity context and are not
//!   driven here. They start jobs rather than edits, so they produce no history
//!   label to measure.
//! - Menu items take a `gpui::Context` and cannot run in a plain unit test.
//! - A row is run once, against one clip, with the playhead inside it. A row
//!   that needs a marked range, a second selected clip, a cut under the
//!   playhead, or a filled clipboard is declared `NoEdit` for that reason, not
//!   because the command is broken.
//! - A row that asks the operator for a path is driven with a supplied answer,
//!   not with a dialog: see `PICK_ROWS` and the tests under it. What a native
//!   dialog itself does is not observed here.

#[path = "reach_tests_g1.rs"]
mod reach_tests_g1;

use aria_commands::{Reach, every_edit_command, every_project_command};
use std::collections::BTreeSet;

use super::AppState;
use super::picker::Answer;
use super::premiere_tests::with_clip;
use crate::commands::BUILTIN_COMMANDS;

/// What a row is expected to do when it is run against the fixture state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Writes at least one history entry.
    Edits,
    /// Writes none: it moves the view, starts a job, saves to disk, asks for a
    /// path, or needs state the fixture does not have.
    NoEdit,
}

use Kind::{Edits, NoEdit};

/// Every palette row and what it does. Checked against the live table, so a new
/// row fails the suite until someone classifies it.
const ROWS: &[(&str, Kind)] = &[
    // Sends a command against one selected clip with the playhead inside it.
    ("pal-split", Edits),
    ("pal-delete", Edits),
    ("pal-bin-new", Edits),
    ("pal-marker", Edits),
    ("pal-adjustment-layer", Edits),
    ("pal-flip-h", Edits),
    ("pal-flip-v", Edits),
    ("pal-crop-reset", Edits),
    ("pal-motion-blur", Edits),
    ("pal-grade-reset", Edits),
    ("pal-denoise", Edits),
    ("pal-pitch-lock", Edits),
    ("pal-mask-clear", Edits),
    ("pal-chroma-green", Edits),
    ("pal-chroma-clear", Edits),
    ("pal-matte-luma", Edits),
    ("pal-matte-alpha", Edits),
    ("pal-matte-clear", Edits),
    ("pal-slot-toggle", Edits),
    ("pal-key-opacity", Edits),
    ("pal-key-position", Edits),
    ("pal-key-scale", Edits),
    ("pal-track-solo", Edits),
    ("pal-track-open-gap", Edits),
    ("pal-track-close-gap", Edits),
    // Moves the view, opens a route or a dialog, or writes to disk. Undo and
    // redo belong here too: they move the history pointer rather than adding
    // to it.
    ("pal-play", NoEdit),
    ("pal-undo", NoEdit),
    ("pal-redo", NoEdit),
    ("pal-save", NoEdit),
    ("pal-open", NoEdit),
    ("pal-new", NoEdit),
    ("pal-export", NoEdit),
    ("pal-settings", NoEdit),
    ("pal-jobs", NoEdit),
    ("pal-history", NoEdit),
    ("pal-preview-fullscreen", NoEdit),
    ("pal-preview-title-safe", NoEdit),
    ("pal-preview-action-safe", NoEdit),
    ("pal-preview-grid", NoEdit),
    ("pal-preview-scopes", NoEdit),
    ("pal-preview-fit", NoEdit),
    ("pal-preview-actual", NoEdit),
    ("pal-handles", NoEdit),
    ("pal-bin-up", NoEdit),
    ("pal-captions", NoEdit),
    ("pal-analysis", NoEdit),
    ("pal-next-moment", NoEdit),
    ("pal-previous-moment", NoEdit),
    ("pal-fit", NoEdit),
    ("pal-fit-selection", NoEdit),
    ("pal-mark-in", NoEdit),
    ("pal-mark-out", NoEdit),
    ("pal-marker-next", NoEdit),
    ("pal-marker-prev", NoEdit),
    ("pal-shift-selection", NoEdit),
    // Inert against the fixture rather than broken: it holds one silent 1x1
    // still on one track, with no cut under the playhead, nothing on the
    // clipboard, no marker, no mask, and no keyframe to remove. A row here is
    // a row this harness cannot reach, which is why the covered set is a floor
    // and not a lock.
    ("pal-paste-insert", NoEdit),
    ("pal-paste-attributes", NoEdit),
    ("pal-extract-audio", NoEdit),
    ("pal-freeze", NoEdit),
    ("pal-link", NoEdit),
    ("pal-unlink", NoEdit),
    ("pal-mask-invert", NoEdit),
    ("pal-transition-crossfade", NoEdit),
    ("pal-transition-clear", NoEdit),
    ("pal-text-editable", NoEdit),
    ("pal-key-remove", NoEdit),
    ("pal-track-up", NoEdit),
    ("pal-track-down", NoEdit),
    ("pal-track-duck", NoEdit),
    ("pal-marker-remove", NoEdit),
    // Asks the operator for a path. With no answer supplied the row asks and
    // stops, which is why it writes nothing here; each is driven with an answer
    // by a test at the foot of this file.
    ("pal-relink", NoEdit),
    ("pal-save-template", NoEdit),
    ("pal-use-template", NoEdit),
    // Rows added to close a measured reach hole. Each keyframe row pins a
    // property the fixture clip carries — it is a media clip, so the audio
    // envelopes answer too — and the lane shift opens a hole at tick 0.
    ("pal-key-anchor-point", Edits),
    ("pal-key-rotation", Edits),
    ("pal-key-crop", Edits),
    ("pal-key-volume", Edits),
    ("pal-key-pan", Edits),
    ("pal-lane-shift-later", Edits),
    // The rest are inert against this fixture rather than broken: it has no
    // marker, no effect on the clip, nothing selected in the pool, a default
    // canvas background, an empty pool filter, and its only clip starts at tick
    // 0 — so shifting the lane earlier would push it below zero and the engine
    // refuses instead of recording an edit. The duck rows tune the next duck and
    // send no command at all.
    ("pal-lane-shift-earlier", NoEdit),
    ("pal-ripple-insert", NoEdit),
    ("pal-marker-snap", NoEdit),
    ("pal-effect-reset", NoEdit),
    ("pal-canvas-background", NoEdit),
    ("pal-media-filter-clear", NoEdit),
    ("pal-duck-amount", NoEdit),
    ("pal-duck-attack", NoEdit),
    ("pal-duck-release", NoEdit),
    // The fixture's clip is a still, not a title, so both font steps are inert
    // here for the same reason the picker is hidden off a title; nothing in the
    // fixture is proxied, so the clear row reports that and writes nothing; and
    // a still is refused a proxy, so the sweep counts one refusal and starts no
    // job.
    ("pal-font-next", NoEdit),
    ("pal-font-prev", NoEdit),
    ("pal-proxy-clear-all", NoEdit),
    ("pal-proxy-create-all", NoEdit),
    // Both refuse against the fixture and say why: its project has never been
    // saved, so a render has no home beside it, and its clip is not a render.
    ("pal-bake-clip", NoEdit),
    ("pal-bake-restore", NoEdit),
    // Project, pool, and session measurements. Every one reports, navigates,
    // or asks for a path; the duplicate-onto-a-lane row is inert here because
    // the fixture project has one track, so no other lane accepts the clip.
    ("pal-project-clip-under-playhead", NoEdit),
    ("pal-project-next-flagged-clip", NoEdit),
    ("pal-project-duplicate-to-lane", NoEdit),
    ("pal-project-reveal-media", NoEdit),
    ("pal-project-describe-template", NoEdit),
    ("pal-project-history-pending", NoEdit),
    ("pal-project-frame-cost", NoEdit),
    ("pal-project-content-size", NoEdit),
    // Assistant, provider, and configuration. Two of these open Settings with
    // its filter set rather than firing blind, so none of them edits.
    ("pal-ai-tools", NoEdit),
    ("pal-ai-system-prompt", NoEdit),
    ("pal-ai-slash", NoEdit),
    ("pal-ai-models", NoEdit),
    ("pal-ai-connection", NoEdit),
    ("pal-ai-schema", NoEdit),
    ("pal-cloud-checksum", NoEdit),
    // Media support: six report, spawn a job, or cycle a session field; the two
    // import rows open a picker and stop until it answers.
    ("pal-media-support", NoEdit),
    ("pal-media-verify", NoEdit),
    ("pal-media-loudness", NoEdit),
    ("pal-media-exposure", NoEdit),
    ("pal-media-normalise", NoEdit),
    ("pal-media-target", NoEdit),
    ("pal-media-import-still", NoEdit),
    ("pal-media-lottie", NoEdit),
    // Render: RelinkMedia is declared not-undoable in aria-engine, and the
    // other two write a file or a status line.
    ("pal-look-install", NoEdit),
    ("pal-render-cost", NoEdit),
    ("pal-media-reload", NoEdit),
    ("pal-media-reload-all", NoEdit),
    // Text and models: a font preflight, an ink report, and a digest sweep.
    // All three read or spawn; none writes an edit.
    ("pal-font-check", NoEdit),
    ("pal-title-ink", NoEdit),
    ("pal-models-verify", NoEdit),
    // Nested sequences. Nesting works on whatever is selected, so it edits from
    // the fixture. The other three need a nest to point at and the fixture has
    // none, so under this harness they refuse and write nothing; what they do
    // when there *is* one is driven in `app::nested::tests`.
    ("pal-nest-selection", Edits),
    ("pal-nest-open", NoEdit),
    ("pal-nest-leave", NoEdit),
    ("pal-nest-place", NoEdit),
    // Mixer: opening the pane writes nothing; adding a bus is an Add track
    // edit, which is the whole point of the row.
    ("pal-mixer-open", NoEdit),
    ("pal-mixer-add-bus", Edits),
];

fn kind_of(id: &str) -> Option<Kind> {
    ROWS.iter().find(|(row, _)| *row == id).map(|(_, k)| *k)
}

/// Run one palette row against a fresh state and report the history labels it
/// added, newest last.
fn labels_from(row: &crate::commands::host::CommandEntry) -> Vec<String> {
    driven(row).0
}

/// Run one palette row against a fresh state and report both the history labels
/// it added, newest last, and every command it dispatched.
///
/// The two answer different questions: the label says the row changed the
/// project, the dispatch says the row asked for something. A save asks and
/// changes nothing, so the surface census needs the second.
fn driven(row: &crate::commands::host::CommandEntry) -> (Vec<String>, Vec<&'static str>) {
    let (mut state, _) = with_clip();
    // Put the playhead inside the clip: several rows are inert outside it, and an
    // inert row would look identical to a broken one.
    if let Some(ui) = state.selected_ui_clip() {
        let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
        state.set_playhead(mid);
    }
    let watch = super::sent_census::Watch::on(&state);
    let before = history_labels(&state);
    (row.run)(&mut state);
    let after = history_labels(&state);
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

/// Every command label the drivable palette rows produced. Each row costs a
/// fresh editor state, so the census is built once and shared.
///
/// Shared with the operator-surface census, which needs the same set from the
/// other direction: this one is what the palette reaches.
pub(super) fn measured() -> &'static BTreeSet<String> {
    &CENSUS.0
}

/// Every command the palette rows dispatched, whether or not it edited.
pub(super) fn dispatched() -> &'static BTreeSet<&'static str> {
    &CENSUS.1
}

/// The palette census: history labels, then commands sent. Both come from one
/// pass, because a pass costs a fresh editor state per row.
static CENSUS: std::sync::LazyLock<(BTreeSet<String>, BTreeSet<&'static str>)> =
    std::sync::LazyLock::new(|| {
        let mut labels = BTreeSet::new();
        let mut sent = BTreeSet::new();
        for row in BUILTIN_COMMANDS {
            let (wrote, asked) = driven(row);
            labels.extend(wrote);
            sent.extend(asked);
        }
        (labels, sent)
    });

#[test]
fn the_table_names_every_palette_row_and_nothing_else() {
    // A row added without a decision is the whole failure mode: it ships as a
    // control that sends nothing and no test notices.
    let live: BTreeSet<&str> = BUILTIN_COMMANDS.iter().map(|row| row.id).collect();
    let declared: BTreeSet<&str> = ROWS.iter().map(|(id, _)| *id).collect();
    let missing: Vec<_> = live.difference(&declared).collect();
    let stale: Vec<_> = declared.difference(&live).collect();
    assert!(
        missing.is_empty() && stale.is_empty(),
        "undeclared rows {missing:?}, declared rows that no longer exist {stale:?}"
    );
}

#[test]
fn every_row_does_what_it_says() {
    // Collected rather than asserted one at a time: one run should name every
    // row that drifted, not the first.
    let mut wrong = Vec::new();
    for row in BUILTIN_COMMANDS {
        let Some(kind) = kind_of(row.id) else {
            continue;
        };
        let produced = labels_from(row);
        match (kind, produced.is_empty()) {
            (Edits, true) => wrong.push(format!("{} declared Edits, sent nothing", row.id)),
            (NoEdit, false) => wrong.push(format!("{} declared NoEdit, sent {produced:?}", row.id)),
            _ => {}
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn the_commands_the_palette_sends_only_grow() {
    // A shrink means a row stopped sending what it used to send. Growth is fine:
    // the list is a floor, not a lock.
    const FLOOR: &[&str] = &[
        "Add generated clip",
        "Add marker",
        "Add track",
        "Crop",
        "Grade",
        "Mark replaceable",
        "New bin",
        "Ripple delete",
        "Set chroma key",
        "Set denoise",
        "Set keyframe",
        "Set mask",
        "Set motion blur",
        "Set pitch lock",
        "Set track matte",
        "Shift clips",
        "Solo track",
        "Split clip",
    ];
    let labels = measured();
    let gone: Vec<_> = FLOOR
        .iter()
        .filter(|label| !labels.contains(**label))
        .collect();
    assert!(gone.is_empty(), "the palette stopped sending {gone:?}");
}

#[test]
fn every_command_the_palette_sends_is_declared_reachable() {
    // The inverse direction of `Reach`: a command an operator demonstrably sends
    // must not be declared internal.
    let labels = measured();
    for cmd in every_edit_command() {
        if labels.contains(cmd.label()) && !cmd.reach().is_operator() {
            panic!(
                "{} is declared unreachable but the palette sends it",
                cmd.label()
            );
        }
    }
    for cmd in every_project_command() {
        if labels.contains(cmd.label()) && !cmd.reach().is_operator() {
            panic!(
                "{} is declared unreachable but the palette sends it",
                cmd.label()
            );
        }
    }
}

#[test]
fn an_internal_command_carries_its_reason_into_the_editor() {
    // The editor is the crate that would have to build the surface, so the
    // reason has to survive the crate boundary rather than living in a comment.
    let internal: Vec<(&str, &str)> = every_edit_command()
        .into_iter()
        .filter_map(|cmd| match cmd.reach() {
            Reach::Operator(_) => None,
            Reach::Planned(reason) | Reach::Internal(reason) => Some((cmd.label(), reason)),
        })
        .collect();
    assert!(
        !internal.is_empty(),
        "the reach declaration lost its internal commands"
    );
    for (label, reason) in internal {
        assert!(
            reason.contains(' '),
            "{label} declares an internal reach with no prose reason"
        );
    }
}

/// Every palette row that asks the operator for a path, each driven below with a
/// supplied answer. The list is checked against the live table at run time by
/// `every_row_that_asks_for_a_path_is_driven_with_an_answer`.
const PICK_ROWS: &[&str] = &[
    "pal-relink",
    "pal-save-template",
    "pal-use-template",
    // Driven in `project_reach_tests.rs`, which supplies an answer to each.
    "pal-project-reveal-media",
    "pal-project-describe-template",
    "pal-cloud-checksum",
    "pal-media-import-still",
    "pal-media-lottie",
    // Driven in `text_reach_tests.rs`, which supplies a font and a non-font.
    "pal-font-check",
];

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// The checksum row hashes the file the operator picked and reports it.
///
/// Drives the row the way the palette does rather than calling the hasher: the
/// class this closes is a picking row that opens a dialog and then does nothing
/// with the answer, which no test of the hasher alone would catch. The digest is
/// asserted by shape and the file by name, because asserting the digest against
/// a second call of the same function would only restate it.
#[test]
fn the_checksum_row_hashes_the_file_it_was_given() {
    let (mut state, _) = with_clip();
    state.supply_pick(Answer::file(fixture()));

    run_row("pal-cloud-checksum", &mut state);

    assert_eq!(state.picks_asked(), 1, "the row did not ask for a file");
    let line = state.status_message.expect("the row reported nothing");
    assert!(
        line.starts_with("texture.png"),
        "the report must name the file that was picked: {line}"
    );
    let digest = match line.split("sha256 ").nth(1) {
        Some(digest) => digest,
        None => panic!("the report carries no digest: {line}"),
    };
    assert_eq!(digest.len(), 64, "sha256 is 64 hex characters: {digest}");
    assert!(
        digest
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "the digest must be lowercase hex: {digest}"
    );
    assert!(
        state.last_error.is_none(),
        "the row failed: {:?}",
        state.last_error
    );
}

/// A scratch dir of this name's own.
///
/// The name goes in the prefix rather than under a shared dir: claiming a scratch
/// dir empties it, and the rows below hold several live at once.
fn scratch(name: &str) -> std::path::PathBuf {
    crate::scratch_pool::scratch(&format!("reach-pick-{name}"))
}

fn run_row(id: &str, state: &mut AppState) {
    let row = BUILTIN_COMMANDS
        .iter()
        .find(|row| row.id == id)
        .expect("the row must still be registered");
    (row.run)(state);
}

#[test]
fn every_row_that_asks_for_a_path_is_driven_with_an_answer() {
    // Derived from the live table rather than declared twice: running a row and
    // asking the state whether it asked for a path names the picking rows at run
    // time. A new row that opens a dialog is red here until it is declared and
    // driven. Reaching this line at all is the other half of the proof: a row
    // that blocked on a dialog would never return.
    let mut asking = Vec::new();
    for row in BUILTIN_COMMANDS {
        let (mut state, _) = with_clip();
        (row.run)(&mut state);
        if state.picks_asked() > 0 {
            asking.push(row.id);
        }
    }
    let declared: BTreeSet<&str> = PICK_ROWS.iter().copied().collect();
    let undeclared: Vec<&&str> = asking
        .iter()
        .filter(|id| !declared.contains(**id))
        .collect();
    assert!(
        undeclared.is_empty(),
        "these rows ask for a path and no test answers one: {undeclared:?}"
    );
}

#[test]
fn saving_a_template_writes_the_bundle_the_operator_named() {
    let (mut state, _) = with_clip();
    let path = scratch("save").join("row.ariat");
    let _ = std::fs::remove_file(&path);
    state.supply_pick(Answer::file(path.clone()));

    run_row("pal-save-template", &mut state);

    assert_eq!(state.picks_asked(), 1, "the row did not ask for a path");
    assert!(
        path.exists(),
        "the bundle was not written where the operator said"
    );
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
