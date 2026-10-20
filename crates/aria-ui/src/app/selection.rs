//! `AppState` selection, and the projection memo that decides whether a poll
//! rebuilds the store.
//!
//! The selection methods are a pure move out of `app.rs` to stay under the file
//! ceiling. They are inherent methods, so they keep their existing paths — a
//! method belongs to the type, not to the file it is written in. No caller
//! changes. [`ProjectionKey`] lives here because `sync_from_engine` does.

use super::*;

impl AppState {
    /// Add an effect to the selection, or say there is none.
    ///
    /// The palette offers a row per catalog effect, so this is a keyboard press
    /// as well as a click in the rack: a press that finds nothing selected has
    /// to answer, or the row reads as a key the editor ignored.
    pub fn add_effect_to_selected(
        &mut self,
        effect_id: impl Into<String>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        let effect_id = effect_id.into();
        self.apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: effect_id.clone(),
        }))?;
        self.last_effect = Some(effect_id);
        Ok(())
    }

    pub fn remove_track(&mut self, track: TrackId) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::RemoveTrack { track }))?;
        Ok(())
    }

    pub fn nudge_selected(&mut self, frames: i64) -> Result<(), EngineError> {
        if self.selected_edge.is_some() {
            return self.trim_selected_edge(frames);
        }
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(ui) = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == clip.raw())
            .cloned()
        else {
            return Ok(());
        };
        let start = (ui.timeline_start_ticks + frames).max(0);
        self.apply_command(Command::Edit(self.move_command(
            clip,
            TrackId::from_raw(ui.track_id),
            RationalTime::new(start, self.playhead.rate),
        )))?;
        Ok(())
    }

    /// Select a clip's In or Out edit point for trimming.
    pub fn select_edge(&mut self, clip: ClipId, side: crate::types::TrimSide) {
        self.selection = vec![clip];
        self.selected_edge = Some(crate::types::SelectedEdge {
            clip: clip.raw(),
            side,
        });
        self.selected_gap = None;
        self.refresh_projected_selection();
    }

    /// Trim the currently selected edit point by `delta` ticks using the active
    /// trim tool or edit mode (Roll, Ripple, or Standard trim).
    pub fn trim_selected_edge(&mut self, delta: i64) -> Result<(), EngineError> {
        let Some(edge) = self.selected_edge else {
            return Ok(());
        };
        if delta == 0 {
            return Ok(());
        }
        let clip = ClipId::from_raw(edge.clip);
        let Some(ui) = self.ui_clip(clip) else {
            return Ok(());
        };
        let left = edge.side == crate::types::TrimSide::In;
        match self.tool.edge_drag() {
            Some(crate::tool_catalog::EdgeDrag::Roll) => {
                let (before, after) = self.abutting(clip);
                let me = crate::app::trim_gestures::Neighbour {
                    clip,
                    start: ui.timeline_start_ticks,
                    end: ui.timeline_end_ticks,
                };
                let pair = if left {
                    before.map(|prev| (prev, me))
                } else {
                    after.map(|next| (me, next))
                };
                if let Some((cut_left, cut_right)) = pair {
                    return self.roll_cut(cut_left, cut_right, delta);
                }
            }
            Some(crate::tool_catalog::EdgeDrag::Ripple) => {
                let track = TrackId::from_raw(ui.track_id);
                return self.ripple_trim(
                    clip,
                    track,
                    ui.timeline_start_ticks,
                    ui.timeline_end_ticks,
                    left,
                    delta,
                );
            }
            Some(crate::tool_catalog::EdgeDrag::Rate) => {
                let old_dur = ui.timeline_end_ticks - ui.timeline_start_ticks;
                let new_dur = if left {
                    old_dur - delta
                } else {
                    old_dur + delta
                };
                return self.rate_stretch(clip, old_dur, new_dur);
            }
            None => {}
        }
        if self.edit_mode == crate::types::EditMode::Ripple {
            let track = TrackId::from_raw(ui.track_id);
            return self.ripple_trim(
                clip,
                track,
                ui.timeline_start_ticks,
                ui.timeline_end_ticks,
                left,
                delta,
            );
        }
        let (start, end) = if left {
            let start = (ui.timeline_start_ticks + delta)
                .max(0)
                .min(ui.timeline_end_ticks - 1);
            (start, ui.timeline_end_ticks)
        } else {
            let end = (ui.timeline_end_ticks + delta).max(ui.timeline_start_ticks + 1);
            (ui.timeline_start_ticks, end)
        };
        self.trim_clip(clip, start, end)
    }

    /// Detect an empty gap before a clip on `track` containing `at_tick`.
    pub fn gap_at(&self, track: TrackId, at_tick: i64) -> Option<crate::types::SelectedGap> {
        let at_tick = at_tick.max(0);
        let lane = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.id == track.raw())?;
        if lane.locked || lane.clips.is_empty() {
            return None;
        }
        let mut cursor = 0i64;
        for clip in &lane.clips {
            if at_tick >= clip.timeline_start_ticks && at_tick < clip.timeline_end_ticks {
                return None;
            }
            if clip.timeline_start_ticks > cursor
                && at_tick >= cursor
                && at_tick < clip.timeline_start_ticks
            {
                return Some(crate::types::SelectedGap {
                    track: track.raw(),
                    start_ticks: cursor,
                    end_ticks: clip.timeline_start_ticks,
                });
            }
            cursor = cursor.max(clip.timeline_end_ticks);
        }
        None
    }

    /// Select the empty gap at `at_tick` on `track`, or clear the selection if
    /// the click landed past the last clip on the lane.
    pub fn select_gap_at(
        &mut self,
        track: TrackId,
        at_tick: i64,
    ) -> Option<crate::types::SelectedGap> {
        let gap = self.gap_at(track, at_tick);
        self.selection.clear();
        self.selected_edge = None;
        self.selected_gap = gap;
        self.refresh_projected_selection();
        gap
    }

    /// Ripple-delete a gap on a lane by shifting subsequent clips left.
    pub fn close_gap(&mut self, gap: crate::types::SelectedGap) -> Result<(), EngineError> {
        let delta = gap.start_ticks - gap.end_ticks;
        self.selected_gap = None;
        if delta >= 0 {
            return Ok(());
        }
        let rate = self.playhead.rate;
        self.apply_command(Command::Edit(EditCommand::ShiftClips {
            track: TrackId::from_raw(gap.track),
            from: RationalTime::new(gap.end_ticks, rate),
            delta: RationalTime::new(delta, rate),
        }))?;
        Ok(())
    }

    /// Track Select Forward (`A`) or Backward (`Shift+A`) from `at_tick`.
    ///
    /// Selects clips on `track` when `all_tracks` is `false`, or across every
    /// unlocked lane when `all_tracks` is `true` (`Shift` modifier on click).
    /// Linked companions on unlocked lanes are included so linked A/V pairs stay
    /// together.
    pub fn select_track_clips(
        &mut self,
        track: TrackId,
        at_tick: i64,
        dir: crate::tool_catalog::TrackSelectDir,
        all_tracks: bool,
    ) {
        let at_tick = at_tick.max(0);
        let mut selected: Vec<ClipId> = Vec::new();
        let mut links: Vec<u64> = Vec::new();
        let mut groups: Vec<u64> = Vec::new();
        for lane in &self.store.sequence.tracks {
            if lane.locked || (!all_tracks && lane.id != track.raw()) {
                continue;
            }
            for clip in &lane.clips {
                if clip.locked {
                    continue;
                }
                let matches = match dir {
                    crate::tool_catalog::TrackSelectDir::Forward => {
                        clip.timeline_end_ticks > at_tick
                    }
                    crate::tool_catalog::TrackSelectDir::Backward => {
                        clip.timeline_start_ticks <= at_tick
                    }
                };
                if matches {
                    let id = ClipId::from_raw(clip.id);
                    if !selected.contains(&id) {
                        selected.push(id);
                    }
                    if let Some(link) = clip.linked_to
                        && !links.contains(&link)
                    {
                        links.push(link);
                    }
                    if let Some(group) = clip.grouped_to
                        && !groups.contains(&group)
                    {
                        groups.push(group);
                    }
                }
            }
        }
        if !links.is_empty() || !groups.is_empty() {
            for lane in &self.store.sequence.tracks {
                if lane.locked {
                    continue;
                }
                for clip in &lane.clips {
                    if !clip.locked
                        && (clip.linked_to.is_some_and(|link| links.contains(&link))
                            || clip.grouped_to.is_some_and(|group| groups.contains(&group)))
                    {
                        let id = ClipId::from_raw(clip.id);
                        if !selected.contains(&id) {
                            selected.push(id);
                        }
                    }
                }
            }
        }
        self.selection = selected;
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
    }

    /// Split the clip at `at_tick` on `track` (or across all unlocked lanes when
    /// `all_tracks` is `true`).
    pub fn split_at_tick(
        &mut self,
        track: TrackId,
        at_tick: i64,
        all_tracks: bool,
    ) -> Result<(), EngineError> {
        let at_tick = at_tick.max(0);
        let mut targets: Vec<ClipId> = Vec::new();
        for lane in &self.store.sequence.tracks {
            if lane.locked || (!all_tracks && lane.id != track.raw()) {
                continue;
            }
            for clip in &lane.clips {
                if !clip.locked
                    && at_tick > clip.timeline_start_ticks
                    && at_tick < clip.timeline_end_ticks
                {
                    targets.push(ClipId::from_raw(clip.id));
                }
            }
        }
        if targets.is_empty() {
            return Ok(());
        }
        let rate = self.playhead.rate;
        let at = RationalTime::new(at_tick, rate);
        self.one_step(|this| {
            for clip in targets {
                if this.ui_clip(clip).is_some_and(|ui| {
                    at_tick > ui.timeline_start_ticks && at_tick < ui.timeline_end_ticks
                }) {
                    this.apply_command(Command::Edit(EditCommand::SplitClip { clip, at }))?;
                }
            }
            Ok(())
        })
    }

    /// The projected row for `id`, borrowed.
    ///
    /// The projection owns the rows for the whole frame, so a reader that only
    /// looks at one takes no copy of it. [`Self::ui_clip`] is the same lookup for
    /// a caller that needs to keep the row past the borrow.
    pub fn ui_clip_ref(&self, id: ClipId) -> Option<&UiClip> {
        let id = id.raw();
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|t| t.clips.iter())
            .find(|c| c.id == id)
    }

    pub fn ui_clip(&self, id: ClipId) -> Option<UiClip> {
        self.ui_clip_ref(id).cloned()
    }

    pub fn selected_ui_clip(&self) -> Option<UiClip> {
        self.ui_clip(self.selected_clip()?)
    }

    /// The selected row, borrowed. See [`Self::ui_clip_ref`].
    pub fn selected_ui_clip_ref(&self) -> Option<&UiClip> {
        self.ui_clip_ref(self.selected_clip()?)
    }

    pub fn toggle_clip_in_selection(&mut self, id: ClipId) {
        let group = self.ui_clip_ref(id).and_then(|clip| clip.grouped_to);
        if let Some(idx) = self.selection.iter().position(|c| *c == id) {
            self.selection.remove(idx);
            if let Some(group) = group {
                let members: Vec<ClipId> = self
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .filter(|t| !t.locked)
                    .flat_map(|t| t.clips.iter())
                    .filter(|c| !c.locked && c.grouped_to == Some(group))
                    .map(|c| ClipId::from_raw(c.id))
                    .collect();
                self.selection.retain(|c| !members.contains(c));
            }
        } else {
            self.selection.push(id);
            if let Some(group) = group {
                let members: Vec<ClipId> = self
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .filter(|t| !t.locked)
                    .flat_map(|t| t.clips.iter())
                    .filter(|c| !c.locked && c.grouped_to == Some(group))
                    .map(|c| ClipId::from_raw(c.id))
                    .collect();
                for member in members {
                    if !self.selection.contains(&member) {
                        self.selection.push(member);
                    }
                }
            }
        }
        self.rebuild_store();
    }

    pub fn add_clip_to_selection(&mut self, id: ClipId) {
        if !self.selection.contains(&id) {
            self.selection.push(id);
        }
        self.rebuild_store();
    }

    pub fn move_clip(
        &mut self,
        clip: ClipId,
        track: TrackId,
        start: i64,
    ) -> Result<(), EngineError> {
        self.snap_guide = None;
        self.apply_command(Command::Edit(self.move_command(
            clip,
            track,
            RationalTime::new(start.max(0), self.playhead.rate),
        )))?;
        Ok(())
    }

    /// The move a drag gesture sends: one that includes the clip's linked
    /// companions, or with linked selection off one that leaves them.
    pub(crate) fn move_command(
        &self,
        clip: ClipId,
        to_track: TrackId,
        start: RationalTime,
    ) -> EditCommand {
        if self.linked_selection {
            EditCommand::MoveClip {
                clip,
                to_track,
                start,
            }
        } else {
            EditCommand::MoveClipUnlinked {
                clip,
                to_track,
                start,
            }
        }
    }

    /// The trim a drag gesture sends, by the same rule as
    /// [`Self::move_command`].
    pub(crate) fn trim_command(&self, clip: ClipId, timeline: TimeRange) -> EditCommand {
        if self.linked_selection {
            EditCommand::TrimClip { clip, timeline }
        } else {
            EditCommand::TrimClipUnlinked { clip, timeline }
        }
    }

    pub fn ripple_move_clip(
        &mut self,
        clip: ClipId,
        track: TrackId,
        start: i64,
    ) -> Result<(), EngineError> {
        self.snap_guide = None;
        self.apply_command(Command::Edit(EditCommand::RippleMoveClip {
            clip,
            to_track: track,
            start: RationalTime::new(start.max(0), self.playhead.rate),
        }))?;
        Ok(())
    }

    pub fn trim_clip(
        &mut self,
        clip: ClipId,
        start_ticks: i64,
        end_ticks: i64,
    ) -> Result<(), EngineError> {
        let start = start_ticks.max(0);
        let end = end_ticks.max(start + 1);
        self.snap_guide = None;
        self.apply_command(Command::Edit(self.trim_command(
            clip,
            TimeRange::at_rate(start, end - start, self.playhead.rate),
        )))?;
        Ok(())
    }

    pub fn duplicate_selected(&mut self) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::DuplicateClip {
            clip: ClipId::from_raw(ui.id),
            to_track: TrackId::from_raw(ui.track_id),
            start: RationalTime::new(ui.timeline_end_ticks.max(0), self.playhead.rate),
        }))?;
        Ok(())
    }

    pub fn set_track_muted(&mut self, track: TrackId, muted: bool) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetTrackMuted { track, muted }))?;
        Ok(())
    }

    pub fn set_track_locked(&mut self, track: TrackId, locked: bool) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetTrackLocked { track, locked }))?;
        Ok(())
    }

    pub fn set_track_enabled(&mut self, track: TrackId, enabled: bool) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetTrackEnabled {
            track,
            enabled,
        }))?;
        Ok(())
    }

    pub fn set_selected_transform(
        &mut self,
        transform: UiClipTransform,
    ) -> Result<(), EngineError> {
        let at = self.transform_edit_tick();
        self.apply_transform(transform, at)
    }

    pub(super) fn transform_edit_tick(&self) -> Option<aria_models::RationalTime> {
        let clip = self.selected_ui_clip()?;
        let animated = [
            aria_models::ClipParam::Position,
            aria_models::ClipParam::Scale,
            aria_models::ClipParam::Rotation,
            aria_models::ClipParam::Opacity,
        ]
        .iter()
        .any(|param| !self.keyframes_of(*param).is_empty());
        let inside = self.playhead.value >= clip.timeline_start_ticks
            && self.playhead.value < clip.timeline_end_ticks;
        (animated && inside).then_some(self.playhead)
    }

    pub(super) fn apply_transform(
        &mut self,
        transform: UiClipTransform,
        at: Option<aria_models::RationalTime>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipTransform {
            clip,
            transform: transform.into(),
            at,
        }))?;
        Ok(())
    }

    pub fn reset_selected_transform(&mut self) -> Result<(), EngineError> {
        self.apply_transform(UiClipTransform::default(), None)
    }

    pub fn remove_effect_at(&mut self, index: usize) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::RemoveEffect { clip, index }))?;
        Ok(())
    }

    pub fn toggle_expanded(&mut self, clip: u64) {
        let open = std::sync::Arc::make_mut(&mut self.expanded_clips);
        if let Some(idx) = open.iter().position(|id| *id == clip) {
            open.remove(idx);
        } else {
            open.push(clip);
        }
        self.refresh_expanded_models();
    }

    pub fn open_context(&mut self, target: ContextTarget, x: f32, y: f32) {
        self.context_plugin_rows = self.plugin_menu_rows(&target);
        self.context_menu = Some(target);
        let max_x = (self.last_window_w - 180.0).max(8.0);
        let max_y = (self.last_window_h - 240.0).max(8.0);
        self.context_at = (x.clamp(8.0, max_x), y.clamp(8.0, max_y));
    }

    pub fn close_context(&mut self) {
        self.context_menu = None;
    }

    pub fn report_error(&mut self, err: impl ReportableError) {
        self.last_error = Some(err.into_error_message());
    }

    pub fn persist_settings_later(&mut self, cx: &mut Context<Self>) {
        self.save_generation = self.save_generation.saturating_add(1);
        let generation = self.save_generation;
        let settings = self.ui_settings.clone();
        let data_dir = self.data_dir.clone();
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SAVE_DEBOUNCE_MS))
                .await;
            let _ = this.update(cx, |state, _cx| {
                if state.save_generation != generation {
                    return;
                }
                if let Err(err) = settings.save(&data_dir) {
                    tracing::warn!(error = %err, "could not persist ui-settings");
                }
            });
        })
        .detach();
    }

    /// The projection, for a write.
    ///
    /// The panels share the store for the length of a frame, which is what makes
    /// reading it free; taking it back for a write copies it only while one of
    /// those frames is still holding the old one. Every writer goes through here,
    /// so there is no path that mutates a projection a panel is drawing.
    pub(crate) fn store_mut(&mut self) -> &mut UiEditorStore {
        std::sync::Arc::make_mut(&mut self.store)
    }

    /// Apply a selection-only change to the existing projection.
    ///
    /// Selection changes no model data. Re-walking every track and clip would
    /// reproduce the same store except for its selected ids and flags.
    pub(crate) fn refresh_projected_selection(&mut self) {
        let held = self.projected.take();
        let (selected, key) = {
            let mut engine = self.engine.lock();
            let rate = engine.project().timeline().frame_rate;
            let at = RationalTime::new(self.playhead.value, rate);
            let selected = selected_detail::SelectedDetail::of(&mut engine, &self.selection, at);
            let mut key = ProjectionKey::of_with_basis(
                &engine,
                self.playhead,
                &self.selection,
                held.as_ref().and_then(|held| held.basis.as_ref()),
            );
            // The store keeps the names it was built with, so the key keeps
            // the catalog revisions they were read at.
            if let Some(held) = &held {
                key.catalogs = held.catalogs;
            }
            (selected, key)
        };
        let mut basis = held.and_then(|held| held.basis);
        let store = std::sync::Arc::make_mut(&mut self.store);
        store.selection.clear();
        store
            .selection
            .extend(self.selection.iter().map(|id| id.raw()));
        crate::projection::reselect(store, &self.selection, basis.as_mut());
        self.selected = selected;
        self.projected = Some(ProjectionKey { basis, ..key });
    }

    /// Take a fresh projection as the one the panels draw.
    ///
    /// The two things the engine knows nothing about — the zoom level and the
    /// scroll offset — are written on the way in, so no reader ever sees a
    /// projection carrying a stale view, and the reconciliation that follows a
    /// new projection cannot be forgotten at one of the call sites.
    ///
    /// The selected clip's detail arrives in the same snapshot and is adopted
    /// here, so a caller cannot take one and forget the other: an inspector
    /// value that disagreed with the store beside it would be worse than the
    /// lock it replaced.
    fn adopt(&mut self, snapshot: Snapshot) {
        // Counted, because the whole point of the memo and of the playhead-only
        // path is that this is rare: a test can hold the number flat across a
        // scrub instead of trusting a comment.
        self.projections += 1;
        let mut store = snapshot.store;
        store.zoom = self.zoom;
        store.scroll_x = self.scroll_x;
        self.store = std::sync::Arc::new(store);
        if let Some(key) = &mut self.projected {
            key.basis = Some(snapshot.basis);
        }
        self.refresh_clock_text();
        self.selected = snapshot.selected;
        self.facts = snapshot.facts;
        // The open property rows draw their clips' models, which this
        // projection may have moved.
        if !self.expanded_clips.is_empty() || !self.expanded_models.is_empty() {
            self.refresh_expanded_models();
        }
        self.reconcile_bins();
        self.announce_offline_media();
        self.reconcile_source_view();
        // Reset the lazy magnet index so the next drag frame builds it once
        // from the newly adopted sequence.
        self.snap_index = std::sync::OnceLock::new();
        // A report on screen follows the edit that just landed. Closed, it
        // costs nothing: the counts are walks of the whole project and nobody
        // is reading them.
        if self.dialog == crate::app::DialogKind::ProjectReport {
            self.measure_project_report();
        }
    }

    /// Poll the engine and re-project only when something the projection reads
    /// has moved.
    ///
    /// Every gesture frame calls this — a trim drag calls it twice per frame —
    /// and a projection walks every track, clip and keyframe. So the inputs are
    /// read under the lock first and compared against the key the current store
    /// was built from; an unchanged poll returns before allocating anything.
    ///
    /// A poll where only the playhead moved — every frame of playback and every
    /// frame of a scrub — re-samples what is read at the playhead and keeps the
    /// store it already has.
    ///
    /// A package waiting for an effect, transition or generator the project
    /// uses loads first, with the engine lock released, and the store is then
    /// built at the same revision so it reads what the package registered.
    pub(crate) fn sync_from_engine(&mut self) {
        self.sync(true);
    }

    /// Poll as [`Self::sync_from_engine`] does, unless another thread holds
    /// the engine. The preview worker holds it for the length of each frame it
    /// renders, so a playback vsync that waited for it would wait out a
    /// render.
    pub(crate) fn sync_from_engine_if_free(&mut self) {
        self.sync(false);
    }

    /// The poll of [`Self::sync_from_engine`]; `wait` waits for an engine
    /// another thread holds, and without it a held engine is left unpolled.
    fn sync(&mut self, wait: bool) {
        let mut poll = self.poll_engine(true, wait);
        if let Some(Poll::Load(wanted)) = &poll {
            for event in wanted {
                self.load_plugins_for(event);
            }
            // A package that did not load is asked for again by the next
            // poll, not by this one.
            self.projected = None;
            poll = self.poll_engine(false, wait);
        }
        match poll {
            None | Some(Poll::Held | Poll::Load(_)) => {}
            Some(Poll::Moved(at, selected)) => {
                self.playhead = at;
                self.store_mut().playhead_ticks = at.value;
                self.refresh_clock_text();
                self.selected = selected;
                if let Some(key) = &mut self.projected {
                    key.playhead = at;
                }
            }
            Some(Poll::Rebuilt(snapshot)) => {
                // `apply_command` advances this selectively before projection.
                // Any other engine mutation reaches this fallback and drops
                // every cell, so no direct engine path can retain stale pixels.
                self.filmstrips.sync_revision(snapshot.revision);
                let edited = snapshot.revision != self.revision;
                self.revision = snapshot.revision;
                self.playhead = snapshot.playhead;
                self.adopt(snapshot);
                if edited {
                    self.picture_changed();
                }
            }
        }
    }

    /// What [`Self::sync_from_engine`] has to do, decided under the engine
    /// lock. `None` when the store already covers the engine. With `load`,
    /// the events of waiting packages the project uses come first. Without
    /// `wait`, an engine another thread holds is [`Poll::Held`].
    fn poll_engine(&mut self, load: bool, wait: bool) -> Option<Poll> {
        let mut engine = if wait {
            self.engine.lock()
        } else {
            match self.engine.try_lock() {
                Some(engine) => engine,
                None => return Some(Poll::Held),
            }
        };
        if load {
            let wanted = self
                .ctx
                .project_activations(engine.project(), engine.revision());
            if !wanted.is_empty() {
                return Some(Poll::Load(wanted));
            }
        }
        // Decided as plain answers first: the arms below write the key back,
        // which they could not do while the match still held it.
        let (covered, playhead_only) = match self.projected.as_ref() {
            Some(key) if key.covers(&engine, self.playhead, &self.selection) => (true, false),
            Some(key) => (false, key.covers_except_playhead(&engine, &self.selection)),
            None => (false, false),
        };
        if covered {
            return None;
        }
        Some(if playhead_only {
            // The projection reads the playhead as one field (pinned by
            // `a_projection_reads_the_playhead_only_as_the_playhead`), so the
            // store is already right and the walk would produce the store it
            // already has. What is genuinely sampled at the playhead is the
            // selected clip's detail, so that is all this re-takes: a scrub
            // frame costs one resolve instead of a walk of every track, clip
            // and keyframe plus a magnet index rebuild.
            let rate = engine.project().timeline().frame_rate;
            let at = RationalTime::new(self.playhead.value, rate);
            let selected = match &self.selected {
                // Nothing but the playhead moved, so the clip, its peers and
                // their property rows are the values already held: only what
                // is sampled at this instant is taken again.
                Some(detail) if !self.selection.is_empty() => {
                    Some(detail.resampled(&mut engine, at))
                }
                _ => selected_detail::SelectedDetail::of(&mut engine, &self.selection, at),
            };
            Poll::Moved(at, selected)
        } else {
            let previous = self.projected.take().and_then(|key| key.basis);
            let snapshot = Snapshot::from_engine(
                &mut engine,
                self.playhead,
                &self.selection,
                previous.as_ref().map(|basis| crate::projection::Previous {
                    store: &self.store,
                    basis,
                }),
            );
            self.projected = Some(ProjectionKey::of_with_basis(
                &engine,
                self.playhead,
                &self.selection,
                Some(&snapshot.basis),
            ));
            Poll::Rebuilt(snapshot)
        })
    }

    /// Re-project unconditionally.
    ///
    /// The caller changed something the projection reads and knows it, so there
    /// is nothing to compare; the key is refreshed on the way through so the
    /// next poll can still short-circuit.
    pub(crate) fn rebuild_store(&mut self) {
        let snapshot = {
            let mut engine = self.engine.lock();
            let previous = self.projected.take().and_then(|key| key.basis);
            let snapshot = Snapshot::from_engine(
                &mut engine,
                self.playhead,
                &self.selection,
                previous.as_ref().map(|basis| crate::projection::Previous {
                    store: &self.store,
                    basis,
                }),
            );
            self.projected = Some(ProjectionKey::of_with_basis(
                &engine,
                self.playhead,
                &self.selection,
                Some(&snapshot.basis),
            ));
            snapshot
        };
        self.adopt(snapshot);
    }
}

/// Everything a projection is built from.
///
/// `project_to_ui_with` is a pure function of the project plus the playhead,
/// the selection, the dirty flag, the history list, and the effect, transition
/// and clip type catalogs it reads names and parameter layouts from, and the
/// proxy pass
/// that follows it reads only the proxy table. So two equal keys describe the same store,
/// byte for byte, and the second projection is work with no output.
///
/// The key is built beside the projection, from the same values in the same
/// order, because a projection input missing from this list would make the memo
/// lie: the store would be stale and no reader could tell.
pub(crate) struct ProjectionKey {
    revision: u64,
    proxy_revision: u64,
    playhead: RationalTime,
    selection: Vec<ClipId>,
    dirty: bool,
    /// The history list's generation: a committed group or a merged pair
    /// changes the list without moving `revision`.
    history: u64,
    catalogs: CatalogRevisions,
    /// The lanes the store was built from, for the next projection to take
    /// what did not move. `None` until the store it describes is adopted.
    basis: Option<crate::projection::LaneBasis>,
}

impl ProjectionKey {
    fn of_with_basis(
        engine: &Engine,
        playhead: RationalTime,
        selection: &[ClipId],
        basis: Option<&crate::projection::LaneBasis>,
    ) -> Self {
        Self {
            revision: engine.revision(),
            proxy_revision: engine.proxy_revision(),
            playhead,
            selection: selection.to_vec(),
            dirty: engine.is_dirty(),
            history: engine.history_generation(),
            catalogs: CatalogRevisions::of_with_basis(engine.project(), basis),
            basis: None,
        }
    }

    /// Whether a projection built from these inputs would equal the one this
    /// key describes. Compares in place: the unchanged path allocates nothing.
    fn covers(&self, engine: &Engine, playhead: RationalTime, selection: &[ClipId]) -> bool {
        self.playhead == playhead && self.covers_except_playhead(engine, selection)
    }

    /// Whether the playhead is the only projection input that moved.
    ///
    /// Separate from [`Self::covers`] because a moved playhead is the common
    /// case — every frame of playback and every frame of a scrub — and it does
    /// not invalidate the store: the projection carries the tick and nothing
    /// derived from it.
    fn covers_except_playhead(&self, engine: &Engine, selection: &[ClipId]) -> bool {
        self.revision == engine.revision()
            && self.proxy_revision == engine.proxy_revision()
            && self.selection == selection
            && self.dirty == engine.is_dirty()
            && self.history == engine.history_generation()
            && self.catalogs.current()
    }
}

/// The effect, transition and clip type catalog revisions a projection read,
/// each `None` when the project holds nothing from that catalog.
///
/// A plugin that registers or drops an entry changes a clip's names, values
/// and clip type without moving the project, so a catalog the project uses is
/// a projection input. One it does not use cannot stale the store, and leaving
/// it out keeps a registration elsewhere from rebuilding a projection that
/// reads nothing of it. The walk runs once per projection, not per poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CatalogRevisions {
    effects: Option<u64>,
    transitions: Option<u64>,
    clip_types: Option<u64>,
}

impl CatalogRevisions {
    fn of_with_basis(
        project: &aria_models::Project,
        basis: Option<&crate::projection::LaneBasis>,
    ) -> Self {
        let (mut effects, transitions, clip_types) = match basis {
            Some(basis) => basis.catalog_usage(),
            None => {
                let (mut effects, mut transitions, mut clip_types) = (false, false, false);
                for track in project.timeline().tracks_ordered() {
                    transitions |= !track.transitions().is_empty();
                    for clip in track.clips() {
                        effects |= !clip.effects.is_empty();
                        clip_types |= clip.clip_type.is_some();
                    }
                }
                (effects, transitions, clip_types)
            }
        };
        // A clip type is drawn from its generator's effect row.
        effects |= clip_types;
        Self {
            effects: effects.then(|| aria_models::effect_catalog().revision()),
            transitions: transitions.then(|| aria_models::transition_catalog().revision()),
            clip_types: clip_types.then(|| aria_models::clip_type_catalog().revision()),
        }
    }

    /// Whether every catalog these revisions hold is still at that revision.
    fn current(&self) -> bool {
        let at = |held: Option<u64>, now: fn() -> u64| held.is_none_or(|held| held == now());
        at(self.effects, || aria_models::effect_catalog().revision())
            && at(self.transitions, || {
                aria_models::transition_catalog().revision()
            })
            && at(self.clip_types, || {
                aria_models::clip_type_catalog().revision()
            })
    }
}

/// What a poll found, decided under the lock and applied after it is dropped.
enum Poll {
    /// Another thread held the engine, and nothing was polled.
    Held,
    /// Waiting packages the project uses, to load before the store is built.
    Load(Vec<aria_plugin::Activation>),
    /// Only the playhead moved: the tick, and the detail sampled at it.
    Moved(RationalTime, Option<selected_detail::SelectedDetail>),
    /// Something the projection reads changed, so it was rebuilt.
    Rebuilt(Snapshot),
}

#[cfg(test)]
#[path = "projection_memo_tests.rs"]
mod projection_memo_tests;
pub trait ReportableError {
    fn into_error_message(self) -> String;
}

impl ReportableError for &EngineError {
    fn into_error_message(self) -> String {
        describe_engine_error(self)
    }
}

impl ReportableError for EngineError {
    fn into_error_message(self) -> String {
        describe_engine_error(&self)
    }
}

impl ReportableError for &str {
    fn into_error_message(self) -> String {
        self.to_string()
    }
}

impl ReportableError for String {
    fn into_error_message(self) -> String {
        self
    }
}

impl ReportableError for &String {
    fn into_error_message(self) -> String {
        self.clone()
    }
}
