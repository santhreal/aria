//! Contracts for a dialog's primary action.
//!
//! The class closed here is "a dialog can be opened from the keyboard and only
//! answered with the pointer". What is asserted:
//!
//! - Every dialog the shell can draw has a decision recorded, and a decision of
//!   "nothing" carries the reason it is nothing. The kinds are read out of the
//!   overlay registry at run time, so registering a dialog and forgetting its
//!   row is red here rather than silent.
//! - Enter on the export sheet reaches the export path: the empty-timeline
//!   refusal arrives, which is the render's own answer and needs no encoder.
//! - Enter on the unsaved prompt saves and lets the parked intent through, which
//!   is the answer that keeps the work.
//! - Enter on a dialog with no primary changes nothing observable, and says so
//!   by returning false rather than by guessing at a control.
//!
//! Not covered: the shell's own `on_key`, which has no headless harness. That
//! branch is one call to `run_dialog_primary` and is proven by driving the
//! shipped binary.

use aria_plugin::PluginContext;

use super::super::premiere_tests::with_clip;
use super::{Primary, primary};
use crate::app::{AppState, DialogKind};

/// The dialogs the shell can put on screen, read from the registry that draws
/// them rather than from a list kept here.
fn registered_kinds() -> Vec<DialogKind> {
    let ctx = PluginContext::new();
    ctx.mount(&crate::plugins::UiPlugin)
        .expect("the ui plugin needs nothing");
    let host = crate::plugins::overlay_host::host_of(&ctx).expect("mount registers the host");
    host.dialog_kinds()
}

#[test]
fn every_registered_dialog_says_what_enter_does() {
    let kinds = registered_kinds();
    assert!(
        kinds.len() >= 6,
        "the shipped dialogs did not register: {kinds:?}"
    );
    for kind in kinds {
        match primary(kind) {
            Primary::Run(_) => {}
            Primary::Nothing(reason) => assert!(
                !reason.trim().is_empty(),
                "{kind:?} declines Enter without saying why"
            ),
        }
    }
}

#[test]
fn a_dialog_with_no_primary_action_swallows_enter_and_changes_nothing() {
    let (mut state, _clip) = with_clip();
    for kind in [
        DialogKind::Settings,
        DialogKind::Jobs,
        DialogKind::ProjectReport,
        DialogKind::About,
        DialogKind::Plugin("nobody"),
    ] {
        state.open_dialog(kind);
        state.last_error = None;
        state.status_message = None;
        let rows = state.job_view.len();

        assert!(!state.run_dialog_primary(), "{kind:?} ran something");

        assert_eq!(state.dialog, kind, "{kind:?} closed itself");
        assert_eq!(state.last_error, None, "{kind:?} reported an error");
        assert_eq!(state.status_message, None, "{kind:?} said something");
        assert_eq!(state.job_view.len(), rows, "{kind:?} queued work");
    }
}

/// Enter on the export sheet reaches the render path, and the proof is the
/// render's own refusal, in the shell's copy for it: an empty sequence is
/// turned down by name before a job exists, so this asserts the wiring without
/// spending an encoder.
#[test]
fn enter_on_the_export_sheet_runs_the_export() {
    let dir = crate::scratch_pool::scratch("dialog-primary");
    let mut state = AppState::new(PluginContext::new(), dir).expect("state");
    state.new_project().expect("new project");
    state.open_dialog(DialogKind::Export);
    let rows = state.job_view.len();

    assert!(state.run_dialog_primary(), "the sheet has a primary action");

    let refusal = crate::app::describe_engine_error(&aria_engine::EngineError::Render(
        aria_render::RenderError::Empty(aria_render::EmptyExport::Sequence),
    ));
    assert_eq!(
        state.last_error.as_deref(),
        Some(refusal.as_str()),
        "Enter must reach the same refusal the button reaches"
    );
    // A refused export leaves no job behind, and the sheet stays up: the
    // operator has something to fix and the sheet is where they fix it.
    assert_eq!(state.job_view.len(), rows);
    assert_eq!(state.dialog, DialogKind::Export);
}

/// Enter on the prompt is "save": the intent that was parked runs afterwards,
/// which is the whole reason the prompt interrupted it.
#[test]
fn enter_on_the_unsaved_prompt_saves_and_continues() {
    let (mut state, _clip) = with_clip();
    let path = state.data_dir.join("enter-saves.aria");
    state.project_path = Some(path.clone());
    assert!(
        !state.request(crate::app::unsaved::PendingIntent::NewProject),
        "a dirty session asks first"
    );
    assert_eq!(state.dialog, DialogKind::UnsavedChanges);

    assert!(
        state.run_dialog_primary(),
        "the prompt has a primary action"
    );

    assert!(path.exists(), "Enter did not save");
    assert_eq!(state.dialog, DialogKind::None, "the prompt stayed up");
    assert_eq!(state.pending_intent(), None, "the intent stayed parked");
}
