//! Filmstrip cells and pool tile pictures held while the session's first
//! Viewer frame decodes, and started by the landing of the Viewer decode that
//! has no other after it.
//!
//! The class closed here: background decodes held for the first frame with no
//! landing that starts them, started beside a decode that follows it, or
//! started again by every landing after it. The first leaves the strips and
//! the tiles blank, the second contends with the frame the boot cover waits
//! on, and the third redraws the pool on every frame. The shell's redraws per
//! event are asserted in `shell::panel_view`'s tests.
//!
//! Not caught: thread scheduling once both workers run after the first frame.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{AppContext as _, TestAppContext};

use crate::app::premiere_tests::with_sequence;
use crate::app::transport::land_preview;
use crate::filmstrips::{ask, test_key as key};

#[test]
fn held_decodes_start_once_when_the_first_frame_lands_with_none_after_it() {
    let cx = TestAppContext::single();
    let state = cx.update(|app| app.new(|_| with_sequence(1, 10)));
    let media = cx.update(|app| state.read(app).store.media[0].id);
    let redraws = Rc::new(Cell::new(0u32));
    let _subscribed = cx.update(|app| {
        let redraws = redraws.clone();
        app.subscribe(&state, move |_, _: &crate::app::PoolTileChanged, _| {
            redraws.set(redraws.get() + 1);
        })
    });
    let cancelled = || Err(aria_render::RenderError::Cancelled.into());

    // The first frame decodes, and a second is asked for while it does, as a
    // resize during the boot asks.
    let first = cx.update(|app| {
        let first = state.update(app, |state, _| {
            assert!(
                state.claim_preview(),
                "an idle decoder starts the first frame"
            );
            let first = state.preview_generation;
            assert!(
                !state.claim_preview(),
                "a second request waits for the first"
            );
            assert!(state.initial_viewer_frame_pending());
            first
        });
        ask(&state, app, vec![key(0), key(8)], vec![key(0)]);
        super::request_thumb(&state, app, media, 0);
        first
    });
    let held = |stage: &str| {
        cx.update(|app| {
            let state = state.read(app);
            assert_eq!(state.filmstrips.in_flight(), 0, "{stage}: a cell started");
            assert_eq!(
                state.filmstrips.asked().0,
                &[key(0), key(8)],
                "{stage}: the queued cells were dropped"
            );
            assert!(
                state.pool_preview.wants(media, 0),
                "{stage}: the tile's picture started"
            );
        });
        assert_eq!(redraws.get(), 0, "{stage}: the pool redrew");
    };
    held("decoding");

    // The first decode lands cancelled with the second owed: everything stays
    // held, because the second decode is the one the boot cover waits on.
    let again =
        cx.update(|app| state.update(app, |state, cx| land_preview(state, cx, first, cancelled())));
    assert!(again, "the landing owes the second request its decode");
    cx.update(|app| {
        state.update(app, |state, _| {
            assert!(state.claim_preview(), "the second decode starts");
        });
    });
    held("a decode follows");

    // The second lands with none after it: the queued cells start, and the
    // pool redraws once so its tiles ask again.
    let second = cx.update(|app| state.read(app).preview_generation);
    let again = cx.update(|app| {
        state.update(app, |state, cx| {
            land_preview(state, cx, second, cancelled())
        })
    });
    assert!(!again, "nothing was asked for while the second decode ran");
    cx.update(|app| {
        assert_eq!(
            state.read(app).filmstrips.in_flight(),
            1,
            "no queued cell started"
        );
        super::request_thumb(&state, app, media, 0);
        assert!(
            !state.read(app).pool_preview.wants(media, 0),
            "the tile asked again and its picture did not start"
        );
    });
    assert_eq!(redraws.get(), 1, "the held tiles redrew the pool once");

    // A later decode lands with nothing held: the pool does not redraw.
    cx.update(|app| {
        state.update(app, |state, cx| {
            assert!(state.claim_preview());
            let generation = state.preview_generation;
            assert!(!land_preview(state, cx, generation, cancelled()));
        });
    });
    assert_eq!(
        redraws.get(),
        1,
        "a landing with nothing held redrew the pool"
    );
    cx.run_until_parked();
}
