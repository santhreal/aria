//! Whether each file the project reads is still on disk, read off the
//! window's thread: each pool item's media, and each Lottie animation and
//! `.cube` LUT a clip reads by path.
//!
//! A projection reads a file's presence from the filesystem once and keeps the
//! answer (see `projection_clip.rs`). The status ticker calls
//! [`AppState::refresh_media_presence`] on each wake, and every
//! [`PRESENCE_EVERY`] it reads every one of those paths again on a thread of
//! its own, so a file deleted, moved, or returned while the project is open
//! is drawn as it is without waiting for an edit, and a share that stops
//! answering holds that thread instead of the window.

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

/// One file, the pool item it is the file of, and whether it is missing.
struct Answer {
    media: Option<u64>,
    path: PathBuf,
    missing: bool,
}

/// Whether each file in `files` is missing.
fn read(files: Vec<(Option<u64>, PathBuf)>) -> Vec<Answer> {
    files
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

/// Every file the project reads: each pool item's, with its id, then each
/// Lottie animation and `.cube` LUT a clip on any sequence reads by path.
fn project_files(project: &aria_models::Project) -> Vec<(Option<u64>, PathBuf)> {
    let mut outside: Vec<&str> = std::iter::once(project.timeline())
        .chain(project.sequences().map(|sequence| &sequence.timeline))
        .flat_map(|timeline| timeline.tracks_ordered())
        .flat_map(|track| track.clips())
        .flat_map(|clip| clip.outside_files())
        .collect();
    outside.sort_unstable();
    outside.dedup();
    project
        .media_iter()
        .map(|media| (Some(media.id.raw()), media.path.clone()))
        .chain(outside.into_iter().map(|path| (None, PathBuf::from(path))))
        .collect()
}

impl AppState {
    /// Read every file the project reads again, on a thread of its own, when
    /// a read is due; re-project when the pool draws a file missing that is
    /// there, or one there that is missing, or when a file's answer differs
    /// from the one the last projection drew.
    ///
    /// One read is out at a time, so a share that stops answering holds one
    /// thread, however long it stays down. An engine another thread holds is
    /// left for the next wake rather than waited for.
    pub(crate) fn refresh_media_presence(&mut self, now: Instant, cx: &mut Context<Self>) {
        if !self.presence.due(now) {
            return;
        }
        let Some(files) = self
            .engine
            .try_lock()
            .map(|engine| project_files(engine.project()))
        else {
            return;
        };
        self.presence = PresenceRead {
            started: Some(now),
            out: true,
        };
        let answers =
            crate::handoff::on_a_thread(cx.background_executor(), "aria-presence", || read(files));
        cx.spawn(async move |this, cx| {
            // A read that panicked answers nothing and the next one is due.
            let answers = answers.await.unwrap_or_default();
            let _ = this.update(cx, |state, cx| {
                state.presence.out = false;
                let read: HashMap<u64, bool> = answers
                    .iter()
                    .filter_map(|answer| Some((answer.media?, answer.missing)))
                    .collect();
                let drawn_wrong = state.store.media.iter().any(|item| {
                    read.get(&item.id)
                        .is_some_and(|&missing| missing != item.missing)
                });
                let changed = crate::projection::record_presence(
                    answers
                        .into_iter()
                        .map(|answer| (answer.path, answer.missing)),
                );
                if drawn_wrong || changed {
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
