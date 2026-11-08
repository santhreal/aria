//! The pool draws a media file as it is on disk while the project is open.
//!
//! The class closed here: a file deleted, moved, or put back while the project
//! is open drawn as it was at the last edit, because the only read of its
//! presence was a projection's, made on the window's thread, and a projection
//! runs after an edit. A read now runs on a thread of its own every
//! [`PRESENCE_EVERY`], one at a time, and re-projects only when the pool draws
//! a file wrong.
//!
//! Not caught: a read that never returns. It holds its thread and the reads
//! after it, and nothing here stands in for a share that stops answering.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::{AppContext, Entity, TestAppContext};

use super::{PRESENCE_EVERY, PresenceRead};
use crate::app::AppState;
use crate::app::premiere_tests::with_clip;

fn texture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

#[test]
fn a_read_is_due_every_interval_and_never_while_one_is_out() {
    let start = Instant::now();
    assert!(
        PresenceRead::default().due(start),
        "the first read is due at once"
    );
    let out = PresenceRead {
        started: Some(start),
        out: true,
    };
    assert!(
        !out.due(start + PRESENCE_EVERY * 10),
        "a read still out holds the next one back"
    );
    let back = PresenceRead {
        started: Some(start),
        out: false,
    };
    assert!(!back.due(start + PRESENCE_EVERY - Duration::from_millis(1)));
    assert!(back.due(start + PRESENCE_EVERY));
}

/// Whether the pool draws `media` missing.
fn drawn_missing(cx: &mut TestAppContext, state: &Entity<AppState>, media: u64) -> Option<bool> {
    cx.update(|app| {
        state
            .read(app)
            .store
            .media
            .iter()
            .find(|item| item.id == media)
            .map(|item| item.missing)
    })
}

/// Run the read due at `now` to its end.
fn refresh(cx: &mut TestAppContext, state: &Entity<AppState>, now: Instant) {
    cx.update(|app| state.update(app, |state, cx| state.refresh_media_presence(now, cx)));
    cx.run_until_parked();
}

#[gpui::test]
fn a_file_lost_and_returned_while_open_is_drawn_without_an_edit(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let file = crate::scratch_pool::scratch("media-presence").join("held.png");
    std::fs::copy(texture(), &file).expect("stage the file");
    let (mut state, _) = with_clip();
    let media = state
        .import_media(file.clone())
        .expect("import the staged file")
        .raw();
    state.sync_from_engine();
    let revision = state.revision;
    let state = cx.update(|app| app.new(|_| state));
    assert_eq!(drawn_missing(cx, &state, media), Some(false));

    let start = Instant::now();
    std::fs::remove_file(&file).expect("lose the file");
    refresh(cx, &state, start);
    assert_eq!(
        drawn_missing(cx, &state, media),
        Some(true),
        "a file lost while the project is open is drawn present"
    );
    assert!(
        cx.update(|app| state.read(app).offline_announced.contains(&media)),
        "a file lost while the project is open is not announced offline"
    );

    std::fs::copy(texture(), &file).expect("put the file back");
    refresh(cx, &state, start + PRESENCE_EVERY);
    assert_eq!(
        drawn_missing(cx, &state, media),
        Some(false),
        "a file put back while the project is open is drawn missing"
    );
    assert_eq!(
        cx.update(|app| state.read(app).revision),
        revision,
        "reading the files edited the project"
    );
}

/// The status ticker is what starts a read: a file lost under a drawn window
/// is drawn missing after the clock moves, with nothing pressed.
#[gpui::test]
fn the_status_ticker_reads_the_files_again(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let file = crate::scratch_pool::scratch("media-presence-tick").join("held.png");
    std::fs::copy(texture(), &file).expect("stage the file");
    let (state, visual) = crate::test_support::drawn_shell(cx, "media-presence");
    let media = visual.update(|_, app| {
        state.update(app, |state, _| {
            let media = state
                .import_media(file.clone())
                .expect("import the staged file");
            state.sync_from_engine();
            // A read the window started while it opened is not this one.
            state.presence = PresenceRead::default();
            media.raw()
        })
    });
    visual.run_until_parked();
    std::fs::remove_file(&file).expect("lose the file");
    visual.executor().advance_clock(Duration::from_secs(1));
    visual.run_until_parked();
    let drawn = visual.read(|app| {
        state
            .read(app)
            .store
            .media
            .iter()
            .find(|item| item.id == media)
            .map(|item| item.missing)
    });
    assert_eq!(drawn, Some(true), "the ticker did not read the files again");
}
