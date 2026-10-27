//! What a pool tile draws while its frame renders, what a full pool drops, and
//! where every dropped picture goes.
//!
//! Closes three classes. A tile scrubbed to a tick not rendered yet drew a
//! glyph, so a scrub flashed the glyph between frames: it draws the picture of
//! the same source held last before the tick, or else first after it, and
//! never another source's. A full pool dropped every picture to make room for
//! one, which blanked every tile: it drops one picture of the source holding
//! the most. And a dropped picture stayed in the window atlas for the life of
//! the window: every path that takes a picture out of the store hands it to
//! the atlas release, and the window no longer holds it once a render landed.
//!
//! Not covered: the order renders land in, which the preview worker sets.

use std::sync::Arc;

use gpui::{RenderImage, TestAppContext};

use super::*;
use crate::app::pool_preview::MAX_THUMBS;
use crate::test_support::atlas::held_or_released_once;
use crate::test_support::drawn_shell;

fn one_pixel() -> Arc<RenderImage> {
    crate::frame::rgba_bytes_to_render(1, 1, &[255, 0, 0, 255]).expect("one pixel")
}

/// A store that has seen sources `1..=sources`, each at stamp 1.
fn store_of(sources: u64) -> PoolPreviewStore {
    let mut store = PoolPreviewStore::default();
    for media in 1..=sources {
        store.sync_media(media, 1);
    }
    store
}

/// Land a thumbnail of `media` at `tick` against stamp 1, and return it.
fn land(store: &mut PoolPreviewStore, media: u64, tick: i64) -> Arc<RenderImage> {
    let image = one_pixel();
    store.start(media, tick);
    store.finish(1, media, tick, Some(Arc::clone(&image)));
    image
}

fn ticks(store: &PoolPreviewStore, media: u64) -> Vec<i64> {
    store.held(media).map(|((_, tick), _)| *tick).collect()
}

#[test]
fn a_tile_draws_the_picture_held_last_before_its_tick_or_else_first_after() {
    let mut store = store_of(4);
    land(&mut store, 1, 25);
    let at_10 = land(&mut store, 2, 10);
    let at_40 = land(&mut store, 2, 40);
    let third = land(&mut store, 3, 5);
    let cases = [
        (2, 10, &at_10, "the picture rendered there"),
        (2, 25, &at_10, "the last before"),
        (2, 39, &at_10, "the last before, however near the next"),
        (2, 40, &at_40, "the picture rendered there"),
        (2, i64::MAX, &at_40, "the last before, however far"),
        (2, 0, &at_10, "the first after, with none before"),
        (2, i64::MIN, &at_10, "the first after, however far"),
        (3, 0, &third, "the first after, not the source before it"),
        (3, 90, &third, "the last before, not the source after it"),
    ];
    for (media, tick, want, why) in cases {
        let got = store
            .picture(media, tick)
            .unwrap_or_else(|| panic!("source {media} at {tick}: nothing drawn"));
        assert!(Arc::ptr_eq(got, want), "source {media} at {tick}: {why}");
    }
    assert!(
        store.picture(4, 30).is_none(),
        "a source with nothing held drew a neighbor's picture"
    );
}

#[test]
fn a_full_pool_drops_one_picture_of_the_source_holding_the_most() {
    // Two tiles at rest, and a third scrubbed until the pool is full.
    let mut store = store_of(3);
    land(&mut store, 1, 0);
    land(&mut store, 2, 0);
    let scrubbed = MAX_THUMBS - 2;
    let last = i64::try_from(scrubbed - 1).expect("a small count") * 4;
    for n in 0..scrubbed {
        land(&mut store, 3, i64::try_from(n).expect("a small count") * 4);
    }
    assert_eq!(store.thumbs.len(), MAX_THUMBS);

    // The scrub goes on near the start: the frame farthest from it goes.
    land(&mut store, 3, 1);
    assert_eq!(store.thumbs.len(), MAX_THUMBS);
    assert_eq!(ticks(&store, 3)[..3], [0, 1, 4]);
    assert_eq!(
        ticks(&store, 3).last(),
        Some(&(last - 4)),
        "the frame farthest from the pointer stayed"
    );
    // And past the end: the first goes.
    land(&mut store, 3, last + 100);
    assert_eq!(ticks(&store, 3).first(), Some(&1));
    assert_eq!(ticks(&store, 3).last(), Some(&(last + 100)));

    // A scrub starting on a tile at rest drops the scrubbed tile's last frame.
    land(&mut store, 2, 50);
    assert_eq!(ticks(&store, 2), [0, 50]);
    assert_eq!(ticks(&store, 3).last(), Some(&(last - 4)));
    assert_eq!(ticks(&store, 1), [0], "a tile at rest lost its picture");
    assert_eq!(store.thumbs.len(), MAX_THUMBS);

    // A landing over a held picture drops nothing else.
    land(&mut store, 1, 0);
    assert_eq!(store.thumbs.len(), MAX_THUMBS);
    assert_eq!(ticks(&store, 2), [0, 50]);

    // A tie keeps the other source's pictures and drops the landing one's,
    // whichever of the two comes first in the store.
    let half = i64::try_from(MAX_THUMBS / 2).expect("a small count");
    for (landing, other) in [(1, 2), (2, 1)] {
        let mut store = store_of(2);
        for tick in 0..half {
            land(&mut store, 1, tick);
            land(&mut store, 2, tick);
        }
        land(&mut store, landing, half);
        assert_eq!(
            ticks(&store, other).len(),
            MAX_THUMBS / 2,
            "source {other} lost one to source {landing}'s landing"
        );
        assert_eq!(ticks(&store, landing).first(), Some(&1));
    }
}

/// Every path that takes a picture out of the store: a landing in a full
/// store, a landing over a held thumbnail, the monitor's next frame, and a
/// source that changed under its thumbnails and its monitor frame.
#[test]
fn every_picture_that_leaves_the_pool_goes_to_the_atlas_release() {
    let mut store = store_of(2);
    let mut given = Vec::new();
    let mut released = Vec::new();
    let mut accounted = |store: &mut PoolPreviewStore, given: &[Arc<RenderImage>], step: &str| {
        store.retire_into(&mut released);
        held_or_released_once(
            store
                .thumbs
                .values()
                .chain(store.source.as_ref().map(|(_, image)| image)),
            &released,
            given,
            step,
        );
        released.len()
    };
    for tick in 0..=i64::try_from(MAX_THUMBS).expect("a small count") {
        given.push(land(&mut store, 1, tick));
    }
    given.push(land(&mut store, 1, 7));
    assert_eq!(
        accounted(&mut store, &given, "a landing"),
        2,
        "the dropped picture and the replaced one"
    );

    for tick in [3, 9] {
        let image = one_pixel();
        store.start_source(2, tick);
        store.finish_source(1, 2, tick, Some(Arc::clone(&image)));
        given.push(image);
    }
    assert_eq!(accounted(&mut store, &given, "the monitor's next frame"), 3);

    given.push(land(&mut store, 2, 0));
    store.sync_media(2, 5);
    store.sync_media(1, 5);
    assert_eq!(
        accounted(&mut store, &given, "a changed source"),
        given.len()
    );
    assert!(store.thumbs.is_empty() && store.source.is_none());
}

/// WHY: a thumbnail the pool dropped stayed in the window atlas until the
/// window closed, so every relink, proxy and long scrub added pictures no
/// tile drew again. A tile's picture dropped because its source changed is
/// gone from the atlas once the render that change started has landed.
#[gpui::test]
async fn a_picture_the_pool_drops_leaves_the_window_atlas(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    let (state, visual) = drawn_shell(cx, "pool-atlas");
    let media = visual.update(|window, app| {
        let media = state.update(app, |state, cx| {
            cx.notify();
            state
                .import_media(fixture())
                .expect("import the fixture")
                .raw()
        });
        window.refresh();
        media
    });
    visual.run_until_parked();
    let drawn = visual
        .update(|window, app| {
            state
                .read(app)
                .pool_preview
                .thumbs
                .values()
                .find(|image| window.has_image_atlas_entry(image))
                .cloned()
        })
        .expect("the pool drew the fixture's picture");

    // The source changed under its pictures, as a relink or a proxy changes it.
    visual.update(|window, app| {
        state.update(app, |state, cx| {
            let stamp = state
                .media_stamp(media)
                .expect("the fixture is in the pool");
            state.pool_preview.sync_media(media, stamp.wrapping_add(1));
            cx.notify();
        });
        window.refresh();
    });
    visual.run_until_parked();
    assert!(
        visual.read(|app| state
            .read(app)
            .pool_preview
            .thumbs
            .values()
            .all(|image| !Arc::ptr_eq(image, &drawn))),
        "the change kept the picture"
    );
    assert!(
        !visual.update(|window, _| window.has_image_atlas_entry(&drawn)),
        "the dropped picture stayed in the window atlas"
    );
}
