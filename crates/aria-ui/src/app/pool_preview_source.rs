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
    /// editor that has one. Returns whether the monitor moved.
    pub fn hover_pool_item(&mut self, media: u64, tick: i64) -> bool {
        self.pool_hover = Some((media, tick));
        let Some(view) = self.sources.focused_mut() else {
            return false;
        };
        if view.media != media {
            return false;
        }
        let before = view.tick;
        view.tick = tick.clamp(0, view.duration_ticks.saturating_sub(1).max(0));
        view.tick != before
    }

    /// The pointer moved over the tile of `media` to `tick`, or off every tile
    /// with `None`. A tile raises [`crate::app::PoolTileChanged`], which
    /// redraws only the bodies that draw tiles, and a moved monitor raises
    /// [`crate::app::FrameReady`] too, which redraws the bodies that draw the
    /// Viewer's frame.
    pub fn hover_pool_tile(&mut self, hover: Option<(u64, i64)>, cx: &mut gpui::Context<AppState>) {
        let monitor = match hover {
            Some((media, tick)) => self.hover_pool_item(media, tick),
            None => {
                self.clear_pool_hover();
                false
            }
        };
        cx.emit(crate::app::PoolTileChanged);
        if monitor {
            cx.emit(crate::app::FrameReady);
        }
    }

    pub fn clear_pool_hover(&mut self) {
        self.pool_hover = None;
    }

    pub fn pool_tile_tick(&self, media: u64) -> i64 {
        match self.pool_hover {
            Some((hovered, tick)) if hovered == media => tick,
            _ => self
                .sources
                .tabs()
                .iter()
                .find(|view| view.media == media)
                .map_or(0, |view| view.tick),
        }
    }

    /// Space: over the pool with an item selected, open or put away the
    /// selection's Source tab; on a Source tab in the Viewer, play or stop the
    /// take; anywhere else, including the Effects panel, the sidebar's other
    /// tabs and a pool with nothing selected, show Program and play or stop
    /// the sequence.
    pub fn space(&mut self) {
        if self.pool_has_keys() && self.pool_selection.focus().is_some() {
            self.toggle_source_monitor();
            return;
        }
        if self.source_has_keys() {
            self.toggle_source_playback();
            return;
        }
        self.show_program();
        self.toggle_playback();
    }

    /// Shift+Space: on a Source tab in the Viewer, play its In to Out;
    /// anywhere else, show Program and play the work area.
    pub fn shift_space(&mut self) {
        if self.source_has_keys() {
            self.play_source_in_to_out();
            return;
        }
        self.show_program();
        self.play_work_area();
    }

    /// Space over the pool: open the selected item in a Source tab, or return
    /// to Program when that tab already has the keys.
    pub fn toggle_source_monitor(&mut self) -> bool {
        let Some(media) = self.pool_selection.focus() else {
            self.status_message = Some("Select a pool item to preview it".into());
            return false;
        };
        if self.sources.focused().is_some_and(|v| v.media == media) {
            self.show_program();
            return false;
        }
        self.open_source(media)
    }

    /// Open `media` in a Source tab and focus it, or focus the tab already open
    /// on it with its position and marks. Stops sequence playback, since the
    /// transport keys now reach the source. Returns whether a tab has the keys.
    pub fn open_source(&mut self, media: u64) -> bool {
        let Some((name, duration_ticks)) = self.viewable_source(media) else {
            return false;
        };
        if self.playing {
            self.toggle_playback();
        }
        let tick = self.pool_tile_tick(media);
        self.sources.open(SourceView {
            media,
            name,
            duration_ticks,
            tick,
            mark_in: None,
            mark_out: None,
            clip: None,
        });
        true
    }

    /// The name and length of `media` when a Source tab can show it. A missing
    /// take, or one with no picture, reports why on the status line.
    pub(crate) fn viewable_source(&mut self, media: u64) -> Option<(gpui::SharedString, i64)> {
        let Some((name, duration_ticks, kind)) = self
            .store
            .media
            .iter()
            .find(|item| item.id == media && !item.missing)
            .map(|item| (item.name.clone(), item.duration_ticks, item.kind))
        else {
            self.status_message = Some("That source cannot be previewed".into());
            return None;
        };
        if kind == crate::types::UiClipKind::Audio {
            self.status_message = Some("An audio source has no picture to show".into());
            return None;
        }
        Some((name, duration_ticks))
    }

    /// Bring every Source tab in line with the pool. A source that left the
    /// pool, or went missing, closes its tab. A re-probe can shorten a source
    /// under a mark set on the longer one; a mark past the end is dropped rather
    /// than clamped, since a silently moved out point is a wrong edit.
    ///
    /// A tab linked to a timeline clip reads its marks from the clip, so an
    /// undo or a trim on the timeline shows in the tab. A linked clip that left
    /// the open sequence leaves a plain Source tab with the last window marked.
    pub(crate) fn reconcile_source_view(&mut self) {
        let media = &self.store.media;
        let tracks = &self.store.sequence.tracks;
        self.sources.retain(|view| {
            let Some(item) = media
                .iter()
                .find(|item| item.id == view.media && !item.missing)
            else {
                return false;
            };
            let duration_ticks = item.duration_ticks;
            view.name = item.name.clone();
            view.duration_ticks = duration_ticks;
            view.tick = view.tick.clamp(0, duration_ticks.saturating_sub(1).max(0));
            view.mark_in = view.mark_in.filter(|start| *start < duration_ticks);
            view.mark_out = view.mark_out.filter(|end| *end <= duration_ticks);
            if let Some(clip) = view.clip {
                match tracks
                    .iter()
                    .flat_map(|track| track.clips.iter())
                    .find(|item| item.id == clip && item.media_id == Some(view.media))
                {
                    Some(item) => {
                        view.mark_in = Some(item.source_start_ticks);
                        view.mark_out = Some(item.source_end_ticks);
                    }
                    None => view.clip = None,
                }
            }
            true
        });
    }

    /// Mark In at the focused Source tab's frame. A tab linked to a timeline
    /// clip trims the clip's head to that frame instead.
    pub fn set_source_in(&mut self) {
        let Some(view) = self.sources.focused_mut() else {
            return;
        };
        if view.clip.is_some() {
            let (tick, (_, out)) = (view.tick, view.window());
            self.trim_linked_source(tick, out);
            return;
        }
        view.mark_in = Some(view.tick);
        if view.mark_out.is_some_and(|out| out <= view.tick) {
            view.mark_out = None;
        }
        self.status_message = Some("Source In".into());
    }

    /// Mark Out after the focused Source tab's frame. A tab linked to a
    /// timeline clip trims the clip's tail to that frame instead.
    pub fn set_source_out(&mut self) {
        let Some(view) = self.sources.focused_mut() else {
            return;
        };
        let out = (view.tick + 1).min(view.duration_ticks.max(1));
        if view.clip.is_some() {
            let (start, _) = view.window();
            self.trim_linked_source(start, out);
            return;
        }
        view.mark_out = Some(out);
        if view.mark_in.is_some_and(|start| start >= out) {
            view.mark_in = None;
        }
        self.status_message = Some("Source Out".into());
    }

    /// Clear both source marks. A tab linked to a timeline clip extends the
    /// clip over the whole take instead.
    pub fn clear_source_marks(&mut self) {
        let Some(view) = self.sources.focused_mut() else {
            return;
        };
        if view.clip.is_some() {
            let end = view.duration_ticks.max(1);
            self.trim_linked_source(0, end);
            return;
        }
        view.mark_in = None;
        view.mark_out = None;
        self.status_message = Some("Cleared Source In and Out".into());
    }

    /// Whether any media selected in the pool is online: what
    /// [`Self::place_pool_selection`] places.
    pub(crate) fn pool_selection_online(&self) -> bool {
        self.pool_selection
            .ids()
            .iter()
            .any(|id| self.media_online(*id))
    }

    fn media_online(&self, id: u64) -> bool {
        self.store
            .media
            .iter()
            .any(|item| item.id == id && !item.missing)
    }

    /// Insert or Overwrite with no Source tab in front: the selected pool items,
    /// in pick order, placed end to end from the playhead through source
    /// patching, as one undo step. Insert (`Ripple`) pushes the patched and
    /// sync-locked lanes right; Overwrite covers what is there. Missing items
    /// are skipped.
    pub(crate) fn place_pool_selection(
        &mut self,
        mode: crate::types::EditMode,
    ) -> Result<(), EngineError> {
        let online: Vec<u64> = self
            .pool_selection
            .ids()
            .iter()
            .copied()
            .filter(|id| self.media_online(*id))
            .collect();
        if online.is_empty() {
            self.status_message =
                Some("Open a source in the Viewer, or select pool items, to place".into());
            return Ok(());
        }
        let ripple = mode == crate::types::EditMode::Ripple;
        let rate = self.playhead.rate;
        let mut at = self.playhead.value;
        let mut placed = 0usize;
        for id in online {
            let media = MediaId::from_raw(id);
            let Some(source) = self
                .engine
                .lock()
                .project()
                .media(media)
                .map(|item| item.full_range())
            else {
                continue;
            };
            let sent = self.place_source(crate::app::source_patch::SourcePlacement {
                media,
                source,
                at: aria_models::RationalTime::new(at, rate),
                fill: None,
                ripple,
                aimed: None,
            })?;
            if !sent {
                continue;
            }
            if placed > 0 {
                self.engine.lock().coalesce_last_two_undo_steps();
            }
            placed += 1;
            at += aria_models::resample(source.duration, rate).value.max(1);
        }
        if placed > 1 {
            // The merge changes the History list and not the project.
            self.sync_from_engine();
        }
        self.status_message = Some(match (ripple, placed) {
            (_, 0) => "Nothing was placed".into(),
            (true, 1) => "Insert".into(),
            (false, 1) => "Overwrite".into(),
            (true, n) => format!("Inserted {n} items"),
            (false, n) => format!("Overwrote with {n} items"),
        });
        Ok(())
    }

    pub fn mark_in_here(&mut self) {
        if self.sources.focused().is_some() {
            self.set_source_in();
        } else {
            self.set_mark_in();
        }
    }
}
