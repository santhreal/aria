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
//! What this does not catch:
//!
//! - Windowed rows (`BUILTIN_JOB_COMMANDS`) need the entity context and are not
//!   driven here. They start jobs rather than edits, so they produce no history
//!   label to measure.
//! - Menu rows, which the palette also offers, take a `gpui::Context`;
//!   `menu_reach_tests` drives them.
//! - A row is run once, against one clip, with the playhead inside it. A row
//!   that needs a marked range, a second selected clip, a cut under the
//!   playhead, or a filled clipboard is declared `NoEdit` for that reason, not
//!   because the command is broken.
//! - A row that prompts for a path is driven with a supplied answer,
//!   not with a dialog: see `PICK_ROWS` and the tests under it. What a native
//!   dialog itself does is not observed here.

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
    ("pal-bin-new", Edits),
    ("pal-motion-blur", Edits),
    ("pal-denoise", Edits),
    ("pal-pitch-lock", Edits),
    ("pal-mask-add-linear", Edits),
    ("pal-mask-add-mirror", Edits),
    ("pal-mask-add-circle", Edits),
    ("pal-mask-add-rectangle", Edits),
    ("pal-mask-add-heart", Edits),
    ("pal-mask-add-star", Edits),
    ("pal-mask-add-path", Edits),
    ("pal-chroma-green", Edits),
    ("pal-slot-toggle", Edits),
    ("pal-key-opacity", Edits),
    ("pal-key-position", Edits),
    ("pal-key-scale", Edits),
    ("pal-track-solo", Edits),
    ("pal-track-open-gap", Edits),
    ("pal-track-close-gap", Edits),
    // Each keyframe row pins a property the fixture clip has: it is a media
    // clip, so the audio envelopes answer too.
    ("pal-key-anchor-point", Edits),
    ("pal-key-rotation", Edits),
    ("pal-key-crop", Edits),
    ("pal-key-volume", Edits),
    ("pal-key-pan", Edits),
    // Moves the playhead or the view, switches a sidebar tab, or opens a
    // prompt.
    ("pal-play", NoEdit),
    ("pal-handles", NoEdit),
    ("pal-bin-up", NoEdit),
    ("pal-captions", NoEdit),
    ("pal-analysis", NoEdit),
    ("pal-next-moment", NoEdit),
    ("pal-previous-moment", NoEdit),
    ("pal-shift-selection", NoEdit),
    // Inert against the fixture rather than broken: it holds one silent 1x1
    // still on one track, so there is no mask to invert, no title to make
    // editable, no keyframe to remove, and no other lane. A row here is a row
    // this harness cannot reach, which is why the covered set is a floor and
    // not a lock.
    ("pal-mask-invert", NoEdit),
    ("pal-text-editable", NoEdit),
    ("pal-key-remove", NoEdit),
    ("pal-track-up", NoEdit),
    ("pal-track-down", NoEdit),
    ("pal-track-duck", NoEdit),
    ("pal-media-filter-clear", NoEdit),
    // The fixture's clip is a still, not a title, so both font steps are inert
    // here for the same reason the picker is hidden off a title; nothing in the
    // fixture is proxied, so the clear row reports that and writes nothing; and
    // a still is refused a proxy, so the sweep counts one refusal and starts no
    // job.
    ("pal-font-next", NoEdit),
    ("pal-font-prev", NoEdit),
    ("pal-proxy-clear-all", NoEdit),
    ("pal-proxy-create-all", NoEdit),
    // Project, pool, and session measurements. Every one reports, navigates,
    // or asks for a path; the duplicate-onto-a-lane row is inert here because
    // the fixture project has one track, so no other lane accepts the clip.
    ("pal-project-clip-under-playhead", NoEdit),
    ("pal-project-next-flagged-clip", NoEdit),
    ("pal-project-duplicate-to-lane", NoEdit),
    ("pal-project-reveal-media", NoEdit),
    ("pal-project-describe-template", NoEdit),
    ("pal-project-history-pending", NoEdit),
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
    // Render: the LUT pack and the cost row write files or a status line, and
    // RelinkMedia, which the reload rows send, is declared not-undoable in
    // aria-engine.
    ("pal-look-install", NoEdit),
    ("pal-render-cost", NoEdit),
    ("pal-media-reload", NoEdit),
    ("pal-media-reload-all", NoEdit),
    // Text and models: a font preflight, an ink report, and a digest sweep.
    // All three read or spawn; none writes an edit.
    ("pal-font-check", NoEdit),
    ("pal-title-ink", NoEdit),
    ("pal-models-verify", NoEdit),
    // Mixer: adding a bus is an Add track edit.
    ("pal-mixer-add-bus", Edits),
];

fn kind_of(id: &str) -> Option<Kind> {
    ROWS.iter().find(|(row, _)| *row == id).map(|(_, k)| *k)
}

/// Run one palette row against a fresh state and report the history labels it
/// added, newest last, every command it dispatched, and whether it asked the
/// file picker for a path.
///
/// # Why
///
/// Each `with_clip()` constructs an `AppState` and a headless `Renderer`
/// (`~240 ms` wall and `66` Lavapipe threads per call). Recording per-row
/// history labels and path-picker requests during the single `CENSUS` pass
/// avoids two more passes over every row in
/// `every_row_does_what_it_says` and
/// `every_row_that_asks_for_a_path_is_driven_with_an_answer`.
fn driven(row: &crate::commands::host::CommandEntry) -> (Vec<String>, Vec<&'static str>, bool) {
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
    (
        after.into_iter().skip(before.len()).collect(),
        watch.take(),
        state.picks_asked() > 0,
    )
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

struct PaletteCensus {
    labels: BTreeSet<String>,
    sent: BTreeSet<&'static str>,
    rows: Vec<(&'static str, Vec<String>, bool)>,
}

/// Every command label the drivable palette rows produced. Each row costs a
/// fresh editor state, so the census is built once and shared.
///
/// Shared with the UI surface census, which needs the same set from the
/// other direction: this one is what the palette reaches.
pub(super) fn measured() -> &'static BTreeSet<String> {
    &CENSUS.labels
}

/// Every command the palette rows dispatched, whether or not it edited.
pub(super) fn dispatched() -> &'static BTreeSet<&'static str> {
    &CENSUS.sent
}

/// The palette census: history labels, commands sent, and per-row outcomes. All
/// come from one pass, because a pass costs a fresh editor state per row.
static CENSUS: std::sync::LazyLock<PaletteCensus> = std::sync::LazyLock::new(|| {
    let mut labels = BTreeSet::new();
    let mut sent = BTreeSet::new();
    let mut rows = Vec::with_capacity(BUILTIN_COMMANDS.len());
    for row in BUILTIN_COMMANDS {
        let (wrote, asked, asked_pick) = driven(row);
        labels.extend(wrote.iter().cloned());
        sent.extend(asked);
        rows.push((row.id, wrote, asked_pick));
    }
    PaletteCensus { labels, sent, rows }
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
    for (id, produced, _) in &CENSUS.rows {
        let Some(kind) = kind_of(id) else {
            continue;
        };
        match (kind, produced.is_empty()) {
            (Edits, true) => wrong.push(format!("{id} declared Edits, sent nothing")),
            (NoEdit, false) => wrong.push(format!("{id} declared NoEdit, sent {produced:?}")),
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
        "Add mask",
        "Add track",
        "Mark replaceable",
        "New bin",
        "Set chroma key",
        "Set denoise",
        "Set keyframe",
        "Set motion blur",
        "Set pitch lock",
        "Shift clips",
        "Solo track",
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
    // The inverse direction of `Reach`: a command the palette demonstrably
    // sends must not be declared internal.
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
    let edit = every_edit_command()
        .into_iter()
        .map(|cmd| (cmd.label(), cmd.reach()));
    let project = every_project_command()
        .into_iter()
        .map(|cmd| (cmd.label(), cmd.reach()));
    let internal: Vec<(&str, &str)> = edit
        .chain(project)
        .filter_map(|(label, reach)| match reach {
            Reach::Operator(_) => None,
            Reach::Planned(reason) | Reach::Internal(reason) => Some((label, reason)),
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

/// Every palette row that prompts for a path. The list is checked against the
/// live table at run time by
/// `every_row_that_asks_for_a_path_is_driven_with_an_answer`.
const PICK_ROWS: &[&str] = &[
    // Driven below.
    "pal-cloud-checksum",
    // Driven in `project_reach_tests.rs`, which supplies an answer to each.
    "pal-project-reveal-media",
    "pal-project-describe-template",
    "pal-media-import-still",
    "pal-media-lottie",
    // Driven in `text_reach_tests.rs`, which supplies a font and a non-font.
    "pal-font-check",
];

fn fixture() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

/// The checksum row hashes the picked file and reports it.
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
    let Some(digest) = line.split("sha256 ").nth(1) else {
        panic!("the report has no digest: {line}");
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
    let asking: Vec<&str> = CENSUS
        .rows
        .iter()
        .filter(|(_, _, asked_pick)| *asked_pick)
        .map(|(id, _, _)| *id)
        .collect();
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
