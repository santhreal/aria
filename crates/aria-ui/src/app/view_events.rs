//! What the window posts for plugins: the view as of each drawn frame, the
//! pool items a projection shows missing, and the rows plugins add to a
//! context menu.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::sync::Arc;

use aria_core::RationalTime;
use aria_models::{ClipId, MediaId, TrackId};
use aria_plugin::{
    ContextKey, ContextKeys, ContextMenuRequested, MediaOffline, MenuEntry, MenuLocation,
    MenuTarget, ViewFrame,
};

use super::AppState;
use crate::types::{ActivePanel, ContextTarget};

/// A row plugins add to a context menu.
#[derive(Clone)]
pub(crate) enum ContextPluginRow {
    /// A row a `ContextMenuRequested` handler added. Its code runs on a thread
    /// of its own.
    Hook(MenuEntry),
    /// A command placed in the menu. It runs like a menu bar row, on the
    /// window's thread, with a failure reported.
    Command {
        command: &'static str,
        label: &'static str,
        /// The asset path of the command's icon.
        icon: Option<&'static str>,
    },
}

impl ContextPluginRow {
    /// The text the row draws.
    pub(crate) fn label(&self) -> &str {
        match self {
            Self::Hook(entry) => &entry.label,
            Self::Command { label, .. } => label,
        }
    }

    /// The asset path of the icon the row draws.
    pub(crate) fn icon(&self) -> Option<&'static str> {
        match self {
            Self::Hook(_) => None,
            Self::Command { icon, .. } => *icon,
        }
    }
}

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
                speed: self.shuttle.as_plugin_speed(),
            },
            self.ctx.events(),
        );
        self.ctx.set_context_keys(self.context_keys());
    }

    /// The editor state plugin enablement conditions read.
    pub(crate) fn context_keys(&self) -> ContextKeys {
        let selected = self.selection.len();
        let focus = match self.active_panel {
            ActivePanel::Media => ContextKey::FocusMedia,
            ActivePanel::Preview => ContextKey::FocusViewer,
            ActivePanel::Timeline => ContextKey::FocusTimeline,
            ActivePanel::Inspector => ContextKey::FocusInspector,
            ActivePanel::Ai => ContextKey::FocusAssistant,
            ActivePanel::Mixer => ContextKey::FocusMixer,
        };
        let clips = self
            .store
            .sequence
            .tracks
            .iter()
            .any(|track| !track.clips.is_empty());
        ContextKeys::NONE
            .with(ContextKey::Clips, clips)
            .with(ContextKey::Selection, selected > 0)
            .with(ContextKey::SingleSelection, selected == 1)
            .with(ContextKey::MarkIn, self.mark_in.is_some())
            .with(ContextKey::MarkOut, self.mark_out.is_some())
            .with(
                ContextKey::InOut,
                self.mark_in.is_some() && self.mark_out.is_some(),
            )
            .with(ContextKey::Playing, self.playing)
            .with(focus, true)
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

    /// The rows plugins add to a menu opened on `target`: the commands placed
    /// in its location, then the rows handlers add, each in mount order.
    ///
    /// Handlers run here, on the window's thread. A handler that panics ends
    /// the round; the rows added before it are kept.
    pub(crate) fn plugin_menu_rows(&self, target: &ContextTarget) -> Arc<[ContextPluginRow]> {
        let location = match target {
            ContextTarget::Clip { .. } => Some(MenuLocation::Clip),
            ContextTarget::Track { .. } => Some(MenuLocation::Track),
            ContextTarget::PoolItem { .. } => Some(MenuLocation::Media),
            ContextTarget::Preview => Some(MenuLocation::Viewer),
            ContextTarget::Transition { .. } => Some(MenuLocation::Transition),
            ContextTarget::Effect { .. } => Some(MenuLocation::EffectHeader),
            ContextTarget::ClipBadge { .. } => Some(MenuLocation::ClipBadge),
            ContextTarget::Recent { .. } => None,
        };
        let mut rows: Vec<ContextPluginRow> = location
            .map(|location| crate::chrome::plugin_menus::rows_in(&self.ctx, location))
            .unwrap_or_default()
            .into_iter()
            .map(|row| ContextPluginRow::Command {
                command: row.command,
                label: row.label,
                icon: row.icon,
            })
            .collect();
        let bus = self.ctx.event_bus();
        if !bus.has_handlers::<ContextMenuRequested>() {
            return rows.into();
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
            ContextTarget::Effect { clip, index } => MenuTarget::Effect {
                clip: ClipId::from_raw(*clip),
                index: *index,
            },
            ContextTarget::ClipBadge { clip, track } => MenuTarget::ClipBadge {
                clip: ClipId::from_raw(*clip),
                track: TrackId::from_raw(*track),
            },
        };
        let mut request = ContextMenuRequested::new(target, self.host.snapshot());
        if catch_unwind(AssertUnwindSafe(|| bus.emit_waterfall(&mut request))).is_err() {
            tracing::error!("a plugin's context menu handler panicked");
        }
        rows.extend(request.entries.into_iter().map(ContextPluginRow::Hook));
        rows.into()
    }

    /// Run one plugin row of a context menu.
    pub(crate) fn run_context_plugin_row(&mut self, row: &ContextPluginRow) {
        match row {
            ContextPluginRow::Hook(entry) => self.run_plugin_row(Arc::clone(&entry.run)),
            ContextPluginRow::Command { command, .. } => {
                self.run_plugin_command(command);
            }
        }
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
