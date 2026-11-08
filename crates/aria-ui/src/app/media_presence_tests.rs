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

/// WHY: a clip whose Lottie animation or `.cube` LUT file was gone drew no
/// offline name on its bar or over the Viewer, and only the pool's media was
/// read again while the project was open, so the preview slated the
/// animation or dropped the grade with nothing naming the file. This pins
/// that each file is named at the ticks its clip covers while it is missing,
/// that a file put back is drawn present at the next read with no edit, and
/// that a file lost again is named again.
///
/// Not caught: a file that is there and does not parse. It is drawn present
/// here, and the render fails on it.
#[gpui::test]
fn a_missing_lottie_or_lut_is_named_and_read_again_while_open(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = crate::scratch_pool::scratch("outside-presence");
    let animation = dir.join("anim.json");
    let grade = dir.join("grade.cube");
    let (mut state, _) = with_clip();
    state
        .set_selected_lut(grade.to_string_lossy())
        .expect("grade the clip");
    let end = state
        .store
        .sequence
        .tracks
        .iter()
        .flat_map(|lane| lane.clips.iter())
        .map(|clip| clip.timeline_end_ticks)
        .max()
        .expect("one clip");
    let lottie = aria_models::Generator::lottie(animation.to_string_lossy(), 64, 64);
    state
        .drop_generated_on_timeline(lottie, None, end)
        .expect("place the animation after the clip");
    state.sync_from_engine();
    let revision = state.revision;
    let state = cx.update(|app| app.new(|_| state));
    let named = |cx: &mut TestAppContext| {
        cx.update(|app| {
            let sequence = &state.read(app).store.sequence;
            [0, end].map(|tick| {
                sequence
                    .offline_at(tick)
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
            })
        })
    };
    let missing = [vec!["grade.cube"], vec!["anim.json"]];
    assert_eq!(named(cx), missing, "a missing outside file was not named");

    let start = Instant::now();
    std::fs::write(&animation, "{}").expect("put the animation back");
    std::fs::write(&grade, "").expect("put the LUT back");
    refresh(cx, &state, start);
    let none: [Vec<&str>; 2] = [vec![], vec![]];
    assert_eq!(named(cx), none, "a file put back is still named missing");

    std::fs::remove_file(&animation).expect("lose the animation");
    std::fs::remove_file(&grade).expect("lose the LUT");
    refresh(cx, &state, start + PRESENCE_EVERY);
    assert_eq!(named(cx), missing, "a file lost again was not named");
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
