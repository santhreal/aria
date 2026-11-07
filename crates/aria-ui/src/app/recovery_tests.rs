//! Autosave and crash recovery.
//!
//! The class these close: work between two explicit saves used to exist only in
//! memory, so a crash took it. Time is passed in rather than read, so the
//! interval is asserted exactly instead of slept through, and the "crash" is a
//! second `AppState` over the same data directory — which is what a relaunch is.
//!
//! The copy is written on a thread of its own, which opens a second class: a
//! copy still in flight when the session moves on. A tick must not start a
//! second copy over it, and rewriting the copy, clearing it, or dropping the
//! session must wait for it, or a late copy replaces newer work, resurrects a
//! cleared offer, or lands in a directory the session no longer holds. Each
//! rule is driven through [`Autosave::start`] with a write the test holds, so
//! the copy is in flight for as long as the test needs.
//!
//! A tick also reads the session, which a preview decode holds for the whole
//! decode. A tick that waited for it would hold the window's thread just as
//! long, so a tick that finds the session held starts nothing, and the next
//! tick starts the copy.
//! Home is animated, so a draw that checked `recovery.aria` and its sidecar on
//! disk read the filesystem every frame. The offer is held on [`AppState`],
//! refreshed when arriving at Home, and kept in step across every recovery
//! write, clear, restore, and discard.
//!
//! A dirty session with no edit since its copy writes no other: the copy on
//! disk already holds it, and rewriting it serialized and synced the whole
//! project every interval for as long as the session sat idle. A copy that
//! failed holds nothing, so the next due tick writes again.
//!
//! Not covered: the half-second ticker that calls `autosave_tick`, which needs a
//! window, and that the tick returns before the write finishes; the
//! `project-io` benchmark's `autosave_tick_ms` target measures that.

use std::io;
use std::path::Path;
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use aria_plugin::PluginContext;

use crate::app::AppState;
use crate::app::recovery::{AUTOSAVE_INTERVAL, Autosave, recovery_path};
use crate::scratch_pool::scratch;
use crate::test_support::while_held;

/// How long a late copy stays in flight before it lands. Long enough that the
/// test acts while it is still being written.
const LATE: Duration = Duration::from_millis(150);

/// The longest a held write waits to be released, so a rule that waits for the
/// copy where it must not fails instead of hanging.
const BOUND: Duration = Duration::from_secs(10);

/// A copy that lands `LATE` after it starts, holding `text`: older work still
/// on its way to disk.
fn late_copy(dir: &Path, text: &'static str) -> impl FnOnce() -> io::Result<()> + Send + 'static {
    let file = recovery_path(dir);
    move || {
        std::thread::sleep(LATE);
        std::fs::write(file, text)
    }
}

fn dirty(dir: std::path::PathBuf) -> AppState {
    let mut state = AppState::new(PluginContext::new(), dir).expect("state");
    state.new_project().expect("a project to edit");
    state
        .add_track(aria_models::TrackKind::Video, "V2")
        .expect("an edit to lose");
    assert!(state.store.is_dirty);
    state
}

#[test]
fn a_fresh_data_directory_offers_nothing() {
    let state = AppState::new(PluginContext::new(), scratch("recovery")).expect("state");
    assert!(state.pending_recovery().is_none());
}

#[test]
fn a_dirty_session_writes_a_copy_and_then_waits_out_the_interval() {
    let mut state = dirty(scratch("recovery"));
    let now = Instant::now();
    assert!(state.autosave_tick(now), "the first dirty tick writes");
    state.finish_autosave();
    assert!(recovery_path(&state.data_dir).is_file());
    state
        .add_track(aria_models::TrackKind::Audio, "after the copy")
        .expect("an edit the copy does not hold");

    assert!(
        !state.autosave_tick(now + AUTOSAVE_INTERVAL - Duration::from_millis(1)),
        "it wrote again before the interval was up"
    );
    assert!(
        state.autosave_tick(now + AUTOSAVE_INTERVAL),
        "the interval elapsed and nothing was written"
    );
}

#[test]
fn a_dirty_session_with_no_edit_since_its_copy_writes_no_other() {
    let mut state = dirty(scratch("recovery"));
    let now = Instant::now();
    assert!(state.autosave_tick(now), "the first dirty tick writes");
    state.finish_autosave();
    let copy = recovery_path(&state.data_dir);
    std::fs::write(&copy, "the copy on disk").expect("mark the copy");
    for round in 1..4 {
        assert!(
            !state.autosave_tick(now + AUTOSAVE_INTERVAL * round),
            "round {round} wrote the same session again"
        );
    }
    state.finish_autosave();
    assert_eq!(
        std::fs::read_to_string(&copy).expect("the copy"),
        "the copy on disk",
        "an idle tick rewrote the copy"
    );
    state
        .add_track(aria_models::TrackKind::Audio, "after the copy")
        .expect("an edit the copy does not hold");
    assert!(
        state.autosave_tick(now + AUTOSAVE_INTERVAL * 4),
        "an edit after the copy wrote no new one"
    );
    state.finish_autosave();
    assert!(aria_models::Project::load_from_file(&copy).is_ok());
}

#[test]
fn a_copy_that_failed_is_written_at_the_next_due_tick() {
    let mut state = dirty(scratch("recovery"));
    let now = Instant::now();
    let revision = state.engine.lock().revision();
    state
        .autosave
        .start(now, revision, || Err(io::Error::other("the disk is full")))
        .expect("the autosave thread");
    state.finish_autosave();
    assert!(
        state.autosave_tick(now + AUTOSAVE_INTERVAL),
        "a copy that failed counted as written"
    );
    state.finish_autosave();
    assert!(recovery_path(&state.data_dir).is_file());
}

#[test]
fn a_clean_session_writes_nothing() {
    let mut state = AppState::new(PluginContext::new(), scratch("recovery")).expect("state");
    state.new_project().expect("project");
    assert!(!state.autosave_tick(Instant::now()));
    assert!(!recovery_path(&state.data_dir).exists());
}

#[test]
fn an_autosave_does_not_make_the_session_look_saved() {
    // The copy goes straight to disk rather than through Save: a session that
    // read clean after an autosave would allow a quit on work that only the
    // recovery file holds.
    let mut state = dirty(scratch("recovery"));
    let revision = state.revision;
    assert!(state.autosave_tick(Instant::now()));
    assert!(state.store.is_dirty, "the autosave cleared the dirty flag");
    assert_eq!(state.revision, revision, "the autosave was an edit");
    assert!(state.project_path.is_none());
}

#[test]
fn the_copy_holds_the_project_as_it_was_when_the_tick_started_it() {
    let mut state = dirty(scratch("recovery"));
    assert!(state.autosave_tick(Instant::now()));
    state
        .add_track(aria_models::TrackKind::Audio, "after the tick")
        .expect("an edit while the copy is written");
    state.finish_autosave();
    let copy = aria_models::Project::load_from_file(&recovery_path(&state.data_dir))
        .expect("the copy reads back as a project");
    let names: Vec<&str> = copy
        .timeline()
        .tracks_ordered()
        .map(|track| track.name.as_str())
        .collect();
    assert!(
        names.contains(&"V2"),
        "the copy lost the edit it was made for: {names:?}"
    );
    assert!(
        !names.contains(&"after the tick"),
        "the copy took an edit made after the tick: {names:?}"
    );
}

#[test]
fn a_copy_in_flight_holds_off_the_next_tick() {
    let mut state = dirty(scratch("recovery"));
    let now = Instant::now();
    let (release, held) = mpsc::channel::<()>();
    state
        .autosave
        .start(now, 0, move || {
            let _ = held.recv_timeout(BOUND);
            Ok(())
        })
        .expect("the autosave thread");
    let later = now + AUTOSAVE_INTERVAL * 3;
    assert!(
        !state.autosave_tick(later),
        "a second copy started while the first was still being written"
    );
    release.send(()).expect("the held write is waiting");
    state.finish_autosave();
    assert!(
        state.autosave_tick(later),
        "the copy finished and the next due tick still started nothing"
    );
    state.finish_autosave();
    assert!(recovery_path(&state.data_dir).is_file());
}

#[test]
fn a_tick_while_the_session_is_held_starts_nothing() {
    let mut state = dirty(scratch("recovery"));
    let now = Instant::now();
    let engine = Arc::clone(&state.engine);
    let waited = while_held(&engine, || {
        assert!(
            !state.autosave_tick(now),
            "the tick started a copy while the session was held"
        );
    });
    assert!(!waited, "the tick waited for the held session");
    assert!(
        state.autosave_tick(now),
        "the session came free and the next tick started nothing"
    );
    state.finish_autosave();
    assert!(recovery_path(&state.data_dir).is_file());
}

#[test]
fn rewriting_the_copy_waits_for_the_one_in_flight() {
    let mut state = dirty(scratch("recovery"));
    // The session's own slot: a copy in flight writes only there.
    state.write_recovery().expect("the first copy");
    state
        .autosave
        .start(Instant::now(), 0, late_copy(&state.data_dir, "older work"))
        .expect("the autosave thread");
    state.write_recovery().expect("the copy, now");
    state.finish_autosave();
    let copy = std::fs::read_to_string(recovery_path(&state.data_dir)).expect("the copy");
    assert_ne!(
        copy, "older work",
        "the copy in flight landed after the newer one and replaced it"
    );
    aria_models::Project::load_from_file(&recovery_path(&state.data_dir))
        .expect("the newer copy reads back as a project");
}

#[test]
fn clearing_the_copy_waits_for_the_one_in_flight() {
    let mut state = dirty(scratch("recovery"));
    state.write_recovery().expect("the first copy");
    state
        .autosave
        .start(Instant::now(), 0, late_copy(&state.data_dir, "older work"))
        .expect("the autosave thread");
    state.clear_recovery();
    state.finish_autosave();
    assert!(
        state.pending_recovery().is_none(),
        "the copy in flight landed after the offer was cleared"
    );
}

#[test]
fn dropping_the_session_waits_for_its_copy() {
    let dir = scratch("recovery");
    let mut state = dirty(dir.clone());
    state
        .autosave
        .start(Instant::now(), 0, late_copy(&dir, "in flight"))
        .expect("the autosave thread");
    drop(state);
    assert!(
        recovery_path(&dir).is_file(),
        "the session dropped with its copy still being written into a directory it no \
         longer holds"
    );
}

#[test]
fn a_copy_waits_out_the_interval_and_any_copy_in_flight() {
    let mut autosave = Autosave::default();
    let now = Instant::now();
    assert!(autosave.due(now), "the first copy waits for nothing");
    let (release, held) = mpsc::channel::<()>();
    autosave
        .start(now, 0, move || {
            let _ = held.recv_timeout(BOUND);
            Ok(())
        })
        .expect("the autosave thread");
    assert!(!autosave.due(now + AUTOSAVE_INTERVAL - Duration::from_millis(1)));
    assert!(
        !autosave.due(now + AUTOSAVE_INTERVAL),
        "due while the last copy is still being written"
    );
    release.send(()).expect("the held write is waiting");
    autosave.finish();
    assert!(autosave.due(now + AUTOSAVE_INTERVAL));
    assert!(!autosave.due(now + AUTOSAVE_INTERVAL - Duration::from_millis(1)));
}

#[test]
fn saving_the_project_clears_the_offer() {
    let mut state = dirty(scratch("recovery"));
    state.write_recovery().expect("a copy to clear");
    state.project_path = Some(state.data_dir.join("kept.aria"));
    state.save_project().expect("save");
    assert!(state.pending_recovery().is_none());
    assert!(!recovery_path(&state.data_dir).exists());
}

#[test]
fn undoing_back_to_the_saved_step_clears_dirty_and_the_recovery_offer() {
    use crate::app::DialogKind;

    let mut state = dirty(scratch("recovery"));
    state.project_path = Some(state.data_dir.join("saved_step.aria"));
    state.save_project().expect("save baseline");
    assert!(!state.store.is_dirty);

    state
        .add_track(aria_models::TrackKind::Audio, "A2")
        .expect("edit after save");
    assert!(state.store.is_dirty);
    state.write_recovery().expect("recovery copy while dirty");
    assert!(state.pending_recovery().is_some());
    assert!(recovery_path(&state.data_dir).is_file());

    assert!(state.undo(), "undo back to saved step");
    assert!(
        !state.store.is_dirty,
        "undoing back to the saved step must mark the session clean"
    );
    assert!(
        state.pending_recovery().is_none() && !recovery_path(&state.data_dir).exists(),
        "undoing back to the saved step must clear the recovery copy"
    );

    state.request_quit();
    assert_eq!(state.dialog, DialogKind::None);
    assert!(
        state.quit_requested(),
        "quitting after undoing back to the saved step must not prompt"
    );
}

#[test]
fn starting_a_new_project_clears_the_offer() {
    let mut state = dirty(scratch("recovery"));
    state.write_recovery().expect("a copy to clear");
    state.new_project().expect("new");
    assert!(state.pending_recovery().is_none());
}

#[test]
fn a_relaunch_offers_the_copy_back_named_after_the_project() {
    let dir = scratch("recovery");
    let path = dir.join("afternoon.aria");
    let mut state = dirty(dir.clone());
    state.project_path = Some(path.clone());
    state.save_project().expect("a saved baseline");
    state
        .add_track(aria_models::TrackKind::Audio, "A2")
        .expect("work after the save");
    let lanes = state.store.sequence.tracks.len();
    state.write_recovery().expect("the crash-time copy");
    drop(state);

    // A relaunch: a new session over the same data directory.
    let mut relaunched = AppState::new(PluginContext::new(), dir).expect("state");
    let offer = relaunched
        .pending_recovery()
        .expect("no offer after a crash");
    assert_eq!(offer.project.as_deref(), Some(path.as_path()));
    assert_eq!(offer.name, "afternoon");

    relaunched.restore_recovery().expect("restore");
    assert_eq!(
        relaunched.store.sequence.tracks.len(),
        lanes,
        "the recovered session is missing the work"
    );
    assert_eq!(
        relaunched.project_path,
        Some(path),
        "restoring aimed at the recovery copy instead of the project"
    );
    assert!(
        relaunched.store.is_dirty,
        "the recovered session reads saved, so a quit would lose it"
    );
    assert_eq!(relaunched.route, crate::types::UiRoute::Editor);
    assert!(
        relaunched
            .status_message
            .as_deref()
            .is_some_and(|text| text.contains("afternoon")),
        "the restore said nothing: {:?}",
        relaunched.status_message
    );
    assert!(
        relaunched.pending_recovery().is_none(),
        "the offer survived being taken"
    );
}

#[test]
fn an_untitled_crash_is_offered_without_a_project_path() {
    let dir = scratch("recovery");
    let mut state = dirty(dir.clone());
    assert!(state.project_path.is_none());
    state.write_recovery().expect("copy");
    drop(state);

    let mut relaunched = AppState::new(PluginContext::new(), dir).expect("state");
    let offer = relaunched.pending_recovery().expect("offer");
    assert_eq!(offer.project, None);
    assert_eq!(offer.name, "Untitled project");
    relaunched.restore_recovery().expect("restore");
    assert_eq!(
        relaunched.project_path, None,
        "an untitled recovery must not claim the recovery file as its project"
    );
    assert!(relaunched.store.is_dirty);
}

#[test]
fn discarding_the_offer_removes_it() {
    let dir = scratch("recovery");
    let mut state = dirty(dir.clone());
    state.write_recovery().expect("copy");
    state.discard_recovery();
    assert!(state.pending_recovery().is_none());
    assert!(!recovery_path(&dir).exists());
    assert!(state.status_message.is_some());
}

/// WHY: the sidecar naming the project a copy shadows was written after the
/// copy, and a failed write was dropped: such a copy read back with whatever
/// sidecar an earlier copy left, and a restore aimed it at that project's
/// file. A sidecar that cannot be replaced now fails the copy. A directory in
/// the sidecar's place can be neither removed nor written over. Not caught: a
/// sidecar removed and then not written, which reads back as untitled work.
#[test]
fn a_sidecar_that_cannot_be_replaced_fails_the_copy() {
    let dir = scratch("recovery");
    let mut state = dirty(dir.clone());
    std::fs::create_dir_all(dir.join("recovery.path").join("held")).expect("a held sidecar");
    assert!(
        state.write_recovery().is_err(),
        "a copy whose sidecar was not written reported success"
    );
}

#[test]
fn restoring_nothing_is_not_an_error() {
    let mut state = AppState::new(PluginContext::new(), scratch("recovery")).expect("state");
    state
        .restore_recovery()
        .expect("a missing copy is no error");
}

/// WHY: Closes "a draw reads the disk" for the Home recovery banner. Home's
/// `home_body` calls [`AppState::pending_recovery`] on every frame; reading
/// `recovery.aria` and `recovery.path` inside `pending_recovery` hit the
/// filesystem at frame rate. The offer is held on `AppState`, refreshed on
/// `go_home`, and kept in step across `write_recovery`, `clear_recovery`,
/// `restore_recovery`, and `discard_recovery`. Does not catch a new caller that
/// reads `recovery_path` directly instead of `pending_recovery`.
#[gpui::test]
fn home_draws_the_recovery_offer_the_state_holds(cx: &mut gpui::TestAppContext) {
    let (state, visual) = crate::test_support::drawn_shell(cx, "recovery_held");
    let data_dir = visual.update(|_, app| {
        state.update(app, |state, cx| {
            let dir = state.data_dir.clone();
            state.project_path = Some(dir.join("cut.aria"));
            state.save_project().expect("baseline");
            state
                .add_track(aria_models::TrackKind::Video, "V2")
                .expect("unsaved edit");
            state.write_recovery().expect("recovery copy");
            state.go_home();
            cx.notify();
            dir
        })
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();

    let banner_shown = |visual: &mut gpui::VisualTestContext| {
        visual.update(|window, _| {
            window.rendered_text_runs().iter().any(|run| {
                run.text.contains(
                    "\u{201c}cut\u{201d} has unsaved work from a session that ended early.",
                )
            })
        })
    };
    assert!(
        banner_shown(visual),
        "Home must show the recovery banner when arriving with a recovery copy"
    );

    std::fs::remove_file(recovery_path(&data_dir)).expect("delete recovery copy behind state");
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        banner_shown(visual),
        "Home read the recovery file during a draw instead of drawing the held offer"
    );

    visual.update(|_, app| {
        state.update(app, |state, cx| {
            state.go_home();
            cx.notify();
        });
    });
    visual.run_until_parked();
    visual.update(|window, _| window.refresh());
    visual.run_until_parked();
    assert!(
        !banner_shown(visual),
        "go_home did not refresh pending_recovery from disk"
    );

    visual.update(|_, app| {
        state.update(app, |state, _| {
            state.write_recovery().expect("write recovery");
            assert_eq!(
                state.pending_recovery().map(|o| o.name),
                Some("cut".to_string()),
                "write_recovery did not refresh pending_recovery"
            );

            state.clear_recovery();
            assert!(
                state.pending_recovery().is_none(),
                "clear_recovery did not clear pending_recovery"
            );

            state.write_recovery().expect("write recovery for restore");
            assert!(state.pending_recovery().is_some());
            state.restore_recovery().expect("restore recovery");
            assert!(
                state.pending_recovery().is_none(),
                "restore_recovery did not clear pending_recovery"
            );

            state.write_recovery().expect("write recovery for discard");
            assert!(state.pending_recovery().is_some());
            state.discard_recovery();
            assert!(
                state.pending_recovery().is_none(),
                "discard_recovery did not clear pending_recovery"
            );

            assert!(state.autosave_tick(Instant::now()));
            state.finish_autosave();
            assert!(
                state.pending_recovery().is_some(),
                "finish_autosave did not refresh pending_recovery"
            );
        });
    });
}

/// WHY: two processes over one data directory shared one recovery copy. The
/// second session's autosave replaced the first session's copy, and a save in
/// the second deleted it, so a crash of the first lost its unsaved work. Each
/// session holds a slot of its own: a second session neither replaces, offers
/// nor clears the first session's copy, and once the first session is gone its
/// copy is offered to the next launch. A second `AppState` stands in for the
/// second process: the slot lock belongs to one open file, so two sessions in
/// one process conflict as two processes do. Not caught: more sessions or
/// left-behind copies than the slots hold.
#[test]
fn a_second_session_neither_replaces_nor_clears_the_first_sessions_copy() {
    const FIRST: &str = "only the first session has this lane";
    let dir = scratch("recovery");
    let holds_first = |file: &Path| {
        aria_models::Project::load_from_file(file)
            .expect("a recovery copy")
            .timeline()
            .tracks_ordered()
            .any(|track| track.name == FIRST)
    };
    let mut first = dirty(dir.clone());
    first
        .add_track(aria_models::TrackKind::Audio, FIRST)
        .expect("the first session's work");
    first.write_recovery().expect("the first session's copy");

    let mut second = dirty(dir.clone());
    second.write_recovery().expect("the second session's copy");
    assert!(
        holds_first(&recovery_path(&dir)),
        "the second session's copy replaced the first session's"
    );
    second.project_path = Some(dir.join("second.aria"));
    second.save_project().expect("the second session saves");
    assert!(
        recovery_path(&dir).is_file() && holds_first(&recovery_path(&dir)),
        "the second session's save deleted the first session's copy"
    );
    assert_eq!(
        second.pending_recovery(),
        None,
        "the second session offered the live first session's copy"
    );

    // The first session ends without saving; a new one edits before it takes
    // up the offer.
    drop(first);
    let mut relaunched = dirty(dir.clone());
    relaunched
        .write_recovery()
        .expect("the relaunched session's copy");
    assert!(
        holds_first(&recovery_path(&dir)),
        "the relaunched session's copy replaced the ended session's"
    );
    let offer = relaunched
        .pending_recovery()
        .expect("the ended first session's copy was not offered");
    assert!(
        holds_first(&offer.file),
        "the offer is not the first session's copy"
    );
}
