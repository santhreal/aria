//! Render: 1 helpers from [`super`].
//!
//! Split out of `pool_preview.rs` to stay under the file ceiling. A pure
//! move. Each item is `pub(super)` so the parent and its other children can
//! reach it by name; nothing else changed.

use super::*;

pub(super) fn render_into(
    state: &Entity<AppState>,
    cx: &mut App,
    media: u64,
    tick: i64,
    max_w: u32,
    max_h: u32,
    monitor: bool,
) {
    let Some(stamp) = state.read(cx).media_stamp(media) else {
        // The entry left the pool between the frame that drew the tile and this
        // request. Nothing to render, and nothing to remember about it.
        return;
    };
    let wanted = state.update(cx, |state, _| {
        state.pool_preview.sync_media(media, stamp);
        let store = &mut state.pool_preview;
        if monitor {
            if !store.wants_source(media, tick) {
                return false;
            }
            store.start_source(media, tick);
        } else {
            if !store.wants(media, tick) {
                return false;
            }
            store.start(media, tick);
        }
        true
    });
    if !wanted {
        return;
    }
    let engine = state.read(cx).engine().clone();
    // The poster frame is the one picture worth keeping between launches: the
    // resting tile, at tile size. A scrub frame and the monitor's pane-sized
    // frame are transient, so neither is looked up or written.
    let poster = (!monitor && tick == 0)
        .then(|| {
            let state = state.read(cx);
            let root = state.storage_layout().resolve(CacheId::LibraryThumbnails)?;
            let path = state
                .store
                .media
                .iter()
                .find(|item| item.id == media && !item.missing)
                .map(|item| PathBuf::from(&*item.path))?;
            Some((root, path))
        })
        .flatten();
    let state = state.clone();
    cx.spawn(async move |cx| {
        // Its own thread, so a codec that panics cannot take the window with it,
        // and the wait is as long as the decode: waiting on the foreground
        // executor would stop the window for it, which is what a hover over a
        // pool tile used to cost.
        let rendered =
            crate::handoff::on_a_thread(cx.background_executor(), "aria-pool-preview", move || {
                // The content key is a `stat` of the source, so it is taken here
                // rather than on the window's thread.
                let cached = poster.and_then(|(root, path)| {
                    let key = crate::app::analysis::content_key_for(&path).ok()?;
                    Some(disk::poster_path(&root, key, max_w, max_h))
                });
                if let Some(image) = cached
                    .as_deref()
                    .and_then(disk::read)
                    .and_then(|frame| crate::frame::rgba_to_render(&frame))
                {
                    return Some(image);
                }
                let frame = engine
                    .lock()
                    .media_frame_fit(MediaId::from_raw(media), tick, max_w, max_h)
                    .ok()?;
                if let Some(path) = cached.as_deref() {
                    // A cache that cannot be written is not a failed render: the
                    // picture is already decoded and the tile still gets it.
                    let _ = disk::write(path, &frame);
                }
                crate::frame::rgba_to_render(&frame)
            })
            .await
            .flatten();
        let _ = state.update(cx, |state, cx| {
            if monitor {
                state
                    .pool_preview
                    .finish_source(stamp, media, tick, rendered);
            } else {
                state.pool_preview.finish(stamp, media, tick, rendered);
            }
            cx.notify();
        });
    })
    .detach();
}
