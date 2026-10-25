//! What a windowed palette row does, measured rather than assumed.
//!
//! A windowed row takes a `&mut gpui::Context<AppState>` and starts a job
//! instead of editing the timeline, so the palette harness — which compares
//! history labels — cannot see it. Nothing else could either: building that
//! context needs a running app. `gpui`'s `test-support` gives tests a headless
//! one, so these rows are driven the same way the menu rows are.
//!
//! What is observable is not an edit. A row either queues a job or refuses in
//! words, and a row that does neither is a dead control: a click produces no
//! visible result. That is the assertion here.
//!
//! The fixture is one silent 1x1 still on one video track, so every analysis row
//! is expected to refuse rather than queue. The refusal has to be visible: an
//! empty `last_error` and `status_message` with no job queued is the failure.

use gpui::AppContext;

use super::AppState;
use super::premiere_tests::with_clip;
use crate::commands::BUILTIN_JOB_COMMANDS;

/// What one windowed row did: a queued job, a refusal in words, or nothing.
#[derive(Debug, PartialEq)]
enum Outcome {
    Queued(usize),
    Refused(String),
    Silent,
}

fn drive(id: &str) -> Outcome {
    drive_with(|state, cx| {
        let row = BUILTIN_JOB_COMMANDS
            .iter()
            .find(|row| row.id == id)
            .expect("the row must still exist");
        (row.run)(state, cx);
    })
}

/// Run one windowed body against a headless app and report what it did.
///
/// Takes the body rather than an id because a palette row and a key binding are
/// the same thing here: a function that needs the entity context. One harness,
/// so a binding cannot be held to a weaker rule than a row.
fn drive_with(run: impl FnOnce(&mut AppState, &mut gpui::Context<AppState>)) -> Outcome {
    let cx = gpui::TestAppContext::single();
    let (mut state, _) = with_clip();
    if let Some(ui) = state.selected_ui_clip() {
        let mid = ui.timeline_start_ticks + (ui.timeline_end_ticks - ui.timeline_start_ticks) / 2;
        state.set_playhead(mid);
    }
    let before = state.job_view.len();
    let entity = cx.update(|app| app.new(|_| state));
    cx.update(|app| entity.update(app, |state: &mut AppState, cx| run(state, cx)));
    cx.update(|app| {
        let state = entity.read(app);
        let queued = state.job_view.len();
        if queued > before {
            return Outcome::Queued(queued - before);
        }
        if let Some(err) = state.last_error.clone() {
            return Outcome::Refused(err);
        }
        if let Some(msg) = state.status_message.clone() {
            return Outcome::Refused(msg);
        }
        Outcome::Silent
    })
}

#[test]
fn every_windowed_row_queues_a_job_or_says_why_not() {
    let mut silent = Vec::new();
    for row in BUILTIN_JOB_COMMANDS {
        match drive(row.id) {
            Outcome::Silent => silent.push(row.id),
            Outcome::Queued(_) => {}
            Outcome::Refused(reason) => assert!(
                reason.contains(' '),
                "{} refused with no prose reason: {reason:?}",
                row.id
            ),
        }
    }
    assert!(
        silent.is_empty(),
        "these rows did nothing and said nothing: {silent:?}"
    );
}

#[test]
fn the_windowed_table_is_not_empty_and_has_unique_ids() {
    // The harness above is vacuous if the table empties out, and two rows with
    // one id are one row to the registry.
    assert!(!BUILTIN_JOB_COMMANDS.is_empty());
    let mut ids: Vec<&str> = BUILTIN_JOB_COMMANDS.iter().map(|row| row.id).collect();
    let count = ids.len();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), count, "duplicate windowed row id");
}

/// Every shipped binding whose body needs the entity context, driven the way the
/// shell drives it.
///
/// The class closed here is a chord that cannot start work. A binding used to be
/// one signature — state in, nothing out — so a job-starting feature was
/// reachable from its own panel and from the palette and from no keystroke at
/// all, and the only way to notice was to look for the key that was missing.
/// A windowed binding is held to the same rule as a windowed row: queue
/// something, or refuse in words.
#[test]
fn every_windowed_binding_starts_work_or_says_why_not() {
    let windowed: Vec<&crate::keys::Binding> = crate::keys::bindings::SHIPPED_BINDINGS
        .iter()
        .filter(|binding| matches!(binding.run, crate::keys::KeyBody::Windowed(_)))
        .collect();
    assert!(
        !windowed.is_empty(),
        "no shipped chord starts work, so the windowed body has no consumer"
    );
    for binding in windowed {
        let crate::keys::KeyBody::Windowed(run) = binding.run else {
            unreachable!("filtered above")
        };
        match drive_with(run) {
            Outcome::Silent => panic!("{} did nothing and said nothing", binding.id),
            Outcome::Queued(_) => {}
            Outcome::Refused(reason) => assert!(
                reason.contains(' '),
                "{} refused with no prose reason: {reason:?}",
                binding.id
            ),
        }
    }
}

/// Every detector has an editor chord that runs it.
///
/// Derived from `AnalysisKind::ALL` rather than a list of three ids, so a fourth
/// detector is red here until it has a chord. The id grammar is the assertion:
/// `key-detect-<id>` beside the palette's `pal-detect-<id>`, one name per
/// detector across both registries.
#[test]
fn every_detector_has_a_chord() {
    for kind in crate::app::analysis::AnalysisKind::ALL {
        let id = format!("key-detect-{}", kind.id());
        let binding = crate::keys::bindings::SHIPPED_BINDINGS
            .iter()
            .find(|binding| binding.id == id)
            .unwrap_or_else(|| panic!("{:?} has no chord: no binding named {id}", kind));
        assert_eq!(
            binding.route,
            crate::keys::KeyRoute::Editor,
            "{id} is not on the editor route, where a selection exists to analyze"
        );
        assert!(
            matches!(binding.run, crate::keys::KeyBody::Windowed(_)),
            "{id} cannot start a job: a detector needs the entity context"
        );
    }
}
