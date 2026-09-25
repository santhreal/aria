//! `AppState` source: 10 methods moved out of `pool_preview.rs`.
//!
//! A pure move to stay under the file ceiling. Inherent methods keep their
//! paths wherever they are written, because a method belongs to the type and
//! not to the file, so no caller changes.

use super::*;

impl AppState {
    /// The pointer is over `media` at `tick`. Drives the tile thumbnail, and the
    /// monitor too when it is open on that source — scrubbing a tile with its
    /// source loaded moves the big picture, as it does in the pool of every
    /// editor that has one.
    pub fn hover_pool_item(&mut self, media: u64, tick: i64) {
        self.pool_hover = Some((media, tick));
        if let Some(view) = self.source_view.as_mut()
            && view.media == media
        {
            view.tick = tick.clamp(0, view.duration_ticks.saturating_sub(1).max(0));
        }
    }

    pub fn clear_pool_hover(&mut self) {
        self.pool_hover = None;
    }

    pub fn pool_tile_tick(&self, media: u64) -> i64 {
        match self.pool_hover {
            Some((hovered, tick)) if hovered == media => tick,
            _ => match self.source_view.as_ref() {
                Some(view) if view.media == media => view.tick,
                _ => 0,
            },
        }
    }

    pub fn space(&mut self) {
        if self.active_panel == crate::types::ActivePanel::Media {
            self.toggle_source_monitor();
            return;
        }
        self.close_source_monitor();
        self.toggle_playback();
    }

    pub fn toggle_source_monitor(&mut self) -> bool {
        let Some(media) = self.pool_selection.focus() else {
            self.status_message = Some("select a pool item to preview it".into());
            return false;
        };
        if self.source_view.as_ref().is_some_and(|v| v.media == media) {
            self.close_source_monitor();
            return false;
        }
        let Some((name, duration_ticks, kind)) = self
            .store
            .media
            .iter()
            .find(|item| item.id == media && !item.missing)
            .map(|item| (item.name.clone(), item.duration_ticks, item.kind))
        else {
            self.status_message = Some("that source cannot be previewed".into());
            return false;
        };
        if kind == crate::types::UiClipKind::Audio {
            self.status_message = Some("an audio source has no picture to show".into());
            return false;
        }
        if self.playing {
            self.toggle_playback();
        }
        self.source_view = Some(SourceView {
            media,
            name,
            duration_ticks,
            tick: self.pool_tile_tick(media),
            mark_in: None,
            mark_out: None,
        });
        true
    }

    pub(crate) fn reconcile_source_view(&mut self) {
        let Some(view) = self.source_view.as_ref() else {
            return;
        };
        let Some((name, duration_ticks)) = self
            .store
            .media
            .iter()
            .find(|item| item.id == view.media && !item.missing)
            .map(|item| (item.name.clone(), item.duration_ticks))
        else {
            self.source_view = None;
            return;
        };
        let view = self.source_view.as_mut().expect("checked above");
        view.name = name;
        view.duration_ticks = duration_ticks;
        view.tick = view.tick.clamp(0, duration_ticks.saturating_sub(1).max(0));
        // A re-probe can shorten a source under a mark that was set on the
        // longer one; a mark past the end is dropped rather than clamped, since
        // a silently moved out point is a wrong edit.
        view.mark_in = view.mark_in.filter(|start| *start < duration_ticks);
        view.mark_out = view.mark_out.filter(|end| *end <= duration_ticks);
    }

    pub fn set_source_in(&mut self) {
        let Some(view) = self.source_view.as_mut() else {
            return;
        };
        view.mark_in = Some(view.tick);
        if view.mark_out.is_some_and(|out| out <= view.tick) {
            view.mark_out = None;
        }
    }

    pub fn set_source_out(&mut self) {
        let Some(view) = self.source_view.as_mut() else {
            return;
        };
        let out = (view.tick + 1).min(view.duration_ticks.max(1));
        view.mark_out = Some(out);
        if view.mark_in.is_some_and(|start| start >= out) {
            view.mark_in = None;
        }
    }

    pub fn clear_source_marks(&mut self) {
        if let Some(view) = self.source_view.as_mut() {
            view.mark_in = None;
            view.mark_out = None;
        }
    }

    pub fn mark_in_here(&mut self) {
        if self.source_view.is_some() {
            self.set_source_in();
            self.status_message = Some("Source In".into());
        } else {
            self.set_mark_in();
        }
    }
}
