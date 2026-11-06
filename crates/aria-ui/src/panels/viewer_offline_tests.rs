//! The Viewer's notice over a frame that shows an offline clip.
//!
//! The class closed here: the preview renders a clip whose file is gone as a
//! plain red slate, and nothing over the picture says it is a missing file
//! rather than footage. These pin that the notice is drawn while the picture
//! holds an offline clip, that it names the clip's file, that a frame whose
//! clips are all there draws none, and that a tick past the clip's end holds
//! no offline clip.
//!
//! Not covered: the slate itself (aria-render's `offline_slate` tests), and
//! the lanes and clips that are turned off.

use std::path::PathBuf;

use gpui::{TestAppContext, VisualTestContext};

fn texture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/texture.png")
}

fn drawn(visual: &mut VisualTestContext) -> bool {
    visual.run_until_parked();
    visual.debug_bounds("viewer-offline").is_some()
}

#[gpui::test]
fn the_viewer_names_the_missing_file_over_its_frame(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let dir = crate::scratch_pool::scratch("viewer-offline-files");
    let gone = dir.join("gone.png");
    std::fs::copy(texture(), &gone).expect("stage a copy of the fixture");
    let (state, visual) = crate::test_support::drawn_shell(cx, "viewer-offline");
    let end = visual.update(|_, app| {
        state.update(app, |s, _| {
            let media = s.import_media(gone.clone()).expect("import the copy");
            s.drop_media_on_timeline(media, None, 0)
                .expect("place the copy");
            s.store
                .sequence
                .tracks
                .iter()
                .flat_map(|lane| lane.clips.iter())
                .next()
                .expect("one clip")
                .timeline_end_ticks
        })
    });
    assert!(
        !drawn(visual),
        "a frame whose clips are all there drew the notice"
    );

    std::fs::remove_file(&gone).expect("lose the file");
    crate::projection::invalidate_media_statuses();
    let other = dir.join("other.png");
    std::fs::copy(texture(), &other).expect("stage a second copy");
    visual.update(|window, app| {
        state.update(app, |s, _| {
            s.import_media(other)
                .expect("an import re-projects the pool");
            s.sync_from_engine();
        });
        window.refresh();
    });
    assert!(
        drawn(visual),
        "a frame holding an offline clip drew no notice"
    );

    let (inside, past) = visual.read(|app| {
        let sequence = &state.read(app).store.sequence;
        (sequence.offline_at(0), sequence.offline_at(end))
    });
    assert_eq!(
        inside.iter().map(|name| name.as_ref()).collect::<Vec<_>>(),
        ["gone.png"]
    );
    assert!(past.is_empty(), "the clip's end tick held it: {past:?}");
}
