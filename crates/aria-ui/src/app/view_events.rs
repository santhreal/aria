//! What the window posts for plugins: the view as of each drawn frame, the
//! pool items a projection shows missing, and the rows plugins add to a
//! context menu.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;

use aria_core::RationalTime;
use aria_models::{ClipId, MediaId, TrackId};
use aria_plugin::{ContextMenuRequested, MediaOffline, MenuEntry, MenuTarget, ViewFrame};

use super::AppState;
use crate::types::ContextTarget;

impl AppState {
    /// Record the selection, playhead, In and Out, and transport for
    /// `EditorView`, posting a notification for each part that changed since
    /// the last frame. A frame that changed nothing copies and posts nothing.
    pub(crate) fn publish_view(&self) {
        let rate = self.playhead.rate;
        let at = |ticks: i64| RationalTime::new(ticks, rate);
        self.editor_view.publish(
            ViewFrame {
                selection: &self.selection,
                playhead: self.playhead,
                mark_in: self.mark_in.map(at),
                mark_out: self.mark_out.map(at),
                playing: self.playing,
                speed: self.shuttle,
            },
            self.ctx.events(),
        );
    }

    /// Post `MediaOffline` for each pool item the projection shows missing and
    /// the last one did not. An item that returns is forgotten, so losing it
    /// again is posted again.
    pub(crate) fn announce_offline_media(&mut self) {
        let media = &self.store.media;
        self.offline_announced
            .retain(|id| media.iter().any(|item| item.id == *id && item.missing));
        for item in media.iter().filter(|item| item.missing) {
            if self.offline_announced.insert(item.id) {
                self.ctx.events().post(MediaOffline {
                    media: MediaId::from_raw(item.id),
                    path: PathBuf::from(item.path.as_str()),
                });
            }
        }
    }

    /// The rows plugins add to a menu opened on `target`, in mount order.
    ///
    /// Handlers run here, on the window's thread. A handler that panics ends
    /// the round; the rows added before it are kept.
    pub(crate) fn plugin_menu_rows(&self, target: &ContextTarget) -> Arc<[MenuEntry]> {
        let bus = self.ctx.event_bus();
        if !bus.has_handlers::<ContextMenuRequested>() {
            return Arc::from([]);
        }
        let target = match target {
            ContextTarget::Clip { clip, track } => MenuTarget::Clip {
                clip: ClipId::from_raw(*clip),
                track: TrackId::from_raw(*track),
            },
            ContextTarget::Track { track } => MenuTarget::Track {
                track: TrackId::from_raw(*track),
            },
            ContextTarget::Transition { clip } => MenuTarget::Transition {
                clip: ClipId::from_raw(*clip),
            },
            ContextTarget::Preview => MenuTarget::Viewer,
            ContextTarget::PoolItem { media } => MenuTarget::Media {
                media: MediaId::from_raw(*media),
            },
            ContextTarget::Recent { path } => MenuTarget::Recent { path: path.clone() },
        };
        let mut request = ContextMenuRequested::new(target, self.host.snapshot());
        if catch_unwind(AssertUnwindSafe(|| bus.emit_waterfall(&mut request))).is_err() {
            tracing::error!("a plugin's context menu handler panicked");
        }
        request.entries.into()
    }

    /// Run a plugin's menu row on a thread of its own, so a row that edits
    /// through the project host or blocks does not stall the window.
    pub(crate) fn run_plugin_row(&mut self, run: Arc<dyn Fn() + Send + Sync>) {
        let spawned = std::thread::Builder::new()
            .name("plugin menu row".into())
            .spawn(move || run());
        if let Err(err) = spawned {
            self.report_error(format!("Could not start the plugin's action: {err}"));
        }
    }
}
