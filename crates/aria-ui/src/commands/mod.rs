//! The commands the editor ships, and the host they register through.
//!
//! A table, not a set of calls: the tests below enumerate it, so a command added
//! here is covered without a second list. These register through
//! `CommandHost::add`, the same call a plugin makes.
//!
//! The registry is the editor's own vocabulary, not the palette's: the palette
//! draws it, the keymap binds it, the menu bar sequences it, and a plugin adds
//! to it. So it lives here rather than under the one panel that happens to show
//! it, and no caller has to name a panel to reach a command.
//!
//! The rows live in sibling files, split by what a row acts on. `BUILTIN_COMMANDS`
//! is their concatenation, joined at compile time from the groups' own lengths —
//! no count is written down, so a row added to any group is in the registry by
//! construction.

// The row tables and the host sit beside this file rather than in a directory of
// their own, so each declaration names its file explicitly.
pub mod ai;
pub mod clip;
pub mod host;
pub mod media;
pub mod mixer;
pub mod project;
pub mod reach;
pub mod render;
pub mod session;
pub mod text;

use crate::app::analysis::AnalysisKind;
use crate::app::palette::Pending;
use crate::app::{DialogKind, spawn_preview, start_playback};
use crate::const_table;

use self::ai::{AI_JOB_ROWS, AI_ROWS};
use self::clip::{CLIP_JOB_ROWS, CLIP_ROWS};
use self::host::{CommandEntry, WindowedEntry};
use self::media::MEDIA_ROWS;
use self::mixer::MIXER_ROWS;
use self::project::PROJECT_ROWS;
use self::reach::REACH_ROWS;
use self::render::RENDER_ROWS;
use self::session::SESSION_ROWS;
use self::text::TEXT_ROWS;

/// Run a palette row, close the palette, and follow through on the two side
/// effects a command can need.
///
/// One dispatch site: the click handler and the shell's Enter both come here, so
/// a row that starts playback from the keyboard starts it from the mouse too.
/// `index` addresses the current hits; `None` means the highlighted row.
///
/// Running the row is [`AppState`](crate::app::AppState)'s job, not the registry's: the state resolves
/// the row, invokes it, and hands back only the work that needs a window. The
/// clock is asked for unconditionally afterwards because it is a no-op unless
/// the row left the transport playing, which is cheaper than a list of the row
/// ids that play.
pub(crate) fn dispatch(
    shell: &mut crate::shell::Shell,
    index: Option<usize>,
    window: &gpui::Window,
    cx: &mut gpui::Context<crate::shell::Shell>,
) {
    let pending = shell.state.update(cx, |state, _| match index {
        Some(at) => state.run_palette_offer(at),
        None => state.run_palette_highlight(),
    });
    match pending {
        Some(Pending::Menu(menu, id)) => shell.state.update(cx, |state, cx| {
            crate::chrome::menu_items::run_item(menu, id, state, cx);
        }),
        Some(Pending::Windowed(run)) => shell.state.update(cx, run),
        None => {}
    }
    shell.state.update(cx, |state, _| {
        if state.dialog == DialogKind::Palette {
            state.open_dialog(DialogKind::None);
        }
    });
    start_playback(shell.state.clone(), window, cx);
    spawn_preview(shell.state.clone(), cx);
    cx.notify();
}

/// The row groups, in the order the palette offers them.
const GROUPS: &[&[CommandEntry]] = &[
    SESSION_ROWS,
    CLIP_ROWS,
    PROJECT_ROWS,
    REACH_ROWS,
    AI_ROWS,
    MEDIA_ROWS,
    MIXER_ROWS,
    RENDER_ROWS,
    TEXT_ROWS,
];

/// How many rows the groups hold between them.
const TOTAL: usize = const_table::total(GROUPS);

/// Every shipped command that runs against the state alone.
pub const BUILTIN_COMMANDS: &[CommandEntry] = &const_table::joined::<TOTAL, CommandEntry>(GROUPS);

/// The detector, transcription, ducking, and update rows.
///
/// One of the groups [`BUILTIN_JOB_COMMANDS`] joins. These need the entity
/// context because they start work, which is a separate table rather than a
/// second field on every plain row: they are the only commands that cannot run
/// against a headless state, and keeping them apart is what lets the plain ones
/// stay callable from a test and from the mobile FFI.
const ANALYSIS_JOB_ROWS: &[WindowedEntry] = &[
    WindowedEntry {
        id: "pal-detect-shots",
        label: "Detect Shots",
        run: |state, cx| state.run_detector(AnalysisKind::Shots, cx),
    },
    WindowedEntry {
        id: "pal-detect-silence",
        label: "Detect Silence",
        run: |state, cx| state.run_detector(AnalysisKind::Silence, cx),
    },
    WindowedEntry {
        id: "pal-detect-beats",
        label: "Detect Beats",
        run: |state, cx| state.run_detector(AnalysisKind::Beats, cx),
    },
    WindowedEntry {
        id: "pal-loudness",
        label: "Measure Loudness",
        run: |state, cx| {
            if let Err(err) = state.measure_loudness(cx) {
                state.report_error(err);
            }
        },
    },
    WindowedEntry {
        id: "pal-transcribe",
        label: "Transcribe Selected Clip",
        run: |state, cx| {
            if let Err(err) = state.transcribe_selected(cx) {
                state.report_error(err);
            }
        },
    },
    WindowedEntry {
        id: "pal-duck",
        label: "Duck Under Voice",
        run: |state, cx| {
            if let Err(err) = state.duck_selected(cx) {
                state.status_message = Some(err);
            }
        },
    },
];

/// The job groups. A second table like [`GROUPS`], for the same reason: a lane
/// adding rows adds a slice here rather than editing a shared literal.
const JOB_GROUPS: &[&[WindowedEntry]] = &[ANALYSIS_JOB_ROWS, AI_JOB_ROWS, CLIP_JOB_ROWS];

/// How many job rows the groups hold between them.
const JOB_TOTAL: usize = const_table::total(JOB_GROUPS);

/// The shipped commands that need the entity context because they start work.
///
/// Joined from `JOB_GROUPS` by the same const fn that builds
/// [`BUILTIN_COMMANDS`], so neither table has a count written down.
pub const BUILTIN_JOB_COMMANDS: &[WindowedEntry] =
    &const_table::joined::<JOB_TOTAL, WindowedEntry>(JOB_GROUPS);

#[cfg(test)]
mod tests {
    use super::{AnalysisKind, BUILTIN_COMMANDS, BUILTIN_JOB_COMMANDS};
    use aria_models::{ClipParam, ClipParamClass, MaskKind};
    use std::collections::BTreeSet;

    /// Both tables, as `(id, label)`. Every invariant below holds across the two:
    /// they land in one registry keyed by id, and the palette draws one list.
    fn rows() -> Vec<(&'static str, &'static str)> {
        BUILTIN_COMMANDS
            .iter()
            .map(|cmd| (cmd.id, cmd.label))
            .chain(BUILTIN_JOB_COMMANDS.iter().map(|cmd| (cmd.id, cmd.label)))
            .collect()
    }

    /// The tables are enumerated at run time, so a command added without a unique
    /// id or label turns this red rather than shadowing an existing row. A
    /// duplicate id also breaks GPUI element identity, and a duplicate label makes
    /// the filtered list ambiguous — Enter would run whichever came first.
    #[test]
    fn every_palette_command_has_a_unique_id_and_label() {
        let rows = rows();
        assert!(!rows.is_empty());
        let ids: BTreeSet<&str> = rows.iter().map(|(id, _)| *id).collect();
        assert_eq!(ids.len(), rows.len(), "duplicate palette command id");
        let labels: BTreeSet<&str> = rows.iter().map(|(_, label)| *label).collect();
        assert_eq!(labels.len(), rows.len(), "duplicate palette command label");
    }

    /// Ids are the element ids the shell hands GPUI, and the prefix is what keeps
    /// them from colliding with a panel's own ids.
    #[test]
    fn every_palette_id_is_namespaced_and_its_label_is_readable() {
        for (id, label) in rows() {
            assert!(id.starts_with("pal-"), "{id} is missing the pal- prefix");
            assert!(!label.trim().is_empty(), "{id} has no label");
            assert_eq!(
                label.trim(),
                label,
                "{id} has padded whitespace in its label"
            );
        }
    }

    /// Every detector is reachable by name. The class this closes: a detector
    /// added to `AnalysisKind` used to be reachable only from the Analysis panel,
    /// so a new one shipped with no palette row and nobody noticed. The list is
    /// enumerated from the enum, so adding a kind turns this red until it has a
    /// row.
    #[test]
    fn every_detector_has_a_palette_row() {
        for kind in AnalysisKind::ALL {
            let id = format!("pal-detect-{}", kind.id());
            assert!(
                BUILTIN_JOB_COMMANDS.iter().any(|cmd| cmd.id == id),
                "{} has no palette row ({id})",
                kind.label()
            );
        }
    }

    /// Every mask shape can be added by name. Enumerated from `MaskKind::ALL`, so
    /// a shape added to the model is red here until the palette offers it, the
    /// same way a detector is.
    #[test]
    fn every_mask_shape_has_a_palette_row() {
        let missing: Vec<String> = MaskKind::ALL
            .into_iter()
            .map(|kind| format!("pal-mask-add-{}", kind.id()))
            .filter(|id| !BUILTIN_COMMANDS.iter().any(|cmd| cmd.id == id.as_str()))
            .collect();
        assert!(
            missing.is_empty(),
            "mask shapes with no palette row: {missing:?}"
        );
    }

    /// A job command in the plain table would run with no context and could not
    /// start its job, so the split is the invariant, not a convention.
    #[test]
    fn the_job_commands_are_not_in_the_plain_table() {
        for cmd in BUILTIN_JOB_COMMANDS {
            assert!(
                !BUILTIN_COMMANDS.iter().any(|plain| plain.id == cmd.id),
                "{} is registered twice, once without a context",
                cmd.id
            );
        }
    }

    /// Every parameter the animatable-property catalog addresses directly must be
    /// keyframable by name.
    ///
    /// Both sides are read at run time: the left from `ClipParam::CENSUS`, which
    /// the enum's own macro emits, and the right from the live command table. The
    /// class comes from `ClipParam::class`, whose match is exhaustive in
    /// `aria-models`, so a new variant cannot reach this test unclassified and a
    /// new `Direct` variant is red here until it has a row.
    ///
    /// What it does not catch: a `Delegated` axis (look, style, text, shape) has
    /// one property per member of its sub-enum and is driven from the inspector's
    /// keyframe block, and an `Excluded` parameter has its own command — a speed
    /// ramp is not written through `set_param_keyframe` at all. Both are skipped
    /// from the declaration rather than from a list kept here.
    #[test]
    fn every_direct_clip_param_has_a_keyframe_palette_row() {
        let mut missing = Vec::new();
        for param in ClipParam::CENSUS {
            let ClipParamClass::Direct(id) = param.class() else {
                continue;
            };
            let row = format!("pal-key-{id}");
            if !BUILTIN_COMMANDS.iter().any(|cmd| cmd.id == row) {
                missing.push(format!("{param:?} has no palette row ({row})"));
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }
}

#[cfg(test)]
#[path = "casing_tests.rs"]
pub(crate) mod casing_tests;
