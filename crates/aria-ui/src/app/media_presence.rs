//! Whether each media file the pool shows is still on disk, read off the
//! window's thread.
//!
//! A projection reads a file's presence from the filesystem once and keeps the
//! answer (see `projection_clip.rs`). The status ticker calls
//! [`AppState::refresh_media_presence`] on each wake, and every
//! [`PRESENCE_EVERY`] it reads every media path of the project again on a
//! thread of its own, so a file deleted, moved, or returned while the project
//! is open is drawn as it is without waiting for an edit, and a share that
//! stops answering holds that thread instead of the window.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui::Context;

use super::AppState;

/// How often the media paths are read again.
pub(crate) const PRESENCE_EVERY: Duration = Duration::from_secs(2);

/// When the media files were last read, and whether a read is still out.
#[derive(Debug, Default)]
pub(crate) struct PresenceRead {
    started: Option<Instant>,
    out: bool,
}

impl PresenceRead {
    /// Whether a read may start at `now`: none is out, and the last one
    /// started [`PRESENCE_EVERY`] or more before it.
    fn due(&self, now: Instant) -> bool {
        !self.out
            && self
                .started
                .is_none_or(|started| now.saturating_duration_since(started) >= PRESENCE_EVERY)
    }
}

/// One pool item's file, and whether it is missing.
struct Answer {
    media: u64,
    path: PathBuf,
    missing: bool,
}

/// Whether each item's file in `media` is missing.
fn read(media: Vec<(u64, PathBuf)>) -> Vec<Answer> {
    media
        .into_iter()
        .map(|(media, path)| {
            let missing = std::fs::metadata(&path).is_err();
            Answer {
                media,
                path,
                missing,
            }
        })
        .collect()
}

impl AppState {
    /// Read every media path of the project again, on a thread of its own,
    /// when a read is due; re-project when the pool draws a file missing that
    /// is there, or one there that is missing.
    ///
    /// One read is out at a time, so a share that stops answering holds one
    /// thread, however long it stays down. An engine another thread holds is
    /// left for the next wake rather than waited for.
    pub(crate) fn refresh_media_presence(&mut self, now: Instant, cx: &mut Context<Self>) {
        if !self.presence.due(now) {
            return;
        }
        let Some(media) = self.engine.try_lock().map(|engine| {
            engine
                .project()
                .media_iter()
                .map(|media| (media.id.raw(), media.path.clone()))
                .collect::<Vec<_>>()
        }) else {
            return;
        };
        self.presence = PresenceRead {
            started: Some(now),
            out: true,
        };
        let answers =
            crate::handoff::on_a_thread(cx.background_executor(), "aria-presence", || read(media));
        cx.spawn(async move |this, cx| {
            // A read that panicked answers nothing and the next one is due.
            let answers = answers.await.unwrap_or_default();
            let _ = this.update(cx, |state, cx| {
                state.presence.out = false;
                let read: HashMap<u64, bool> = answers
                    .iter()
                    .map(|answer| (answer.media, answer.missing))
                    .collect();
                let drawn_wrong = state.store.media.iter().any(|item| {
                    read.get(&item.id)
                        .is_some_and(|&missing| missing != item.missing)
                });
                crate::projection::record_presence(
                    answers
                        .into_iter()
                        .map(|answer| (answer.path, answer.missing)),
                );
                if drawn_wrong {
                    state.projected = None;
                    state.sync_from_engine();
                    cx.notify();
                }
            });
        })
        .detach();
    }
}

#[cfg(test)]
#[path = "media_presence_tests.rs"]
mod tests;
