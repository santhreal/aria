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
        self.apply_command(Command::Edit(EditCommand::AddEffect {
            clip,
            effect_id: effect_id.into(),
        }))?;
        Ok(())
    }

    pub fn remove_track(&mut self, track: TrackId) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::RemoveTrack { track }))?;
        Ok(())
    }

    pub fn nudge_selected(&mut self, frames: i64) -> Result<(), EngineError> {
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
        self.apply_command(Command::Edit(EditCommand::MoveClip {
            clip,
            to_track: TrackId::from_raw(ui.track_id),
            start: RationalTime::new(start, self.playhead.rate),
        }))?;
        Ok(())
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
        if let Some(idx) = self.selection.iter().position(|c| *c == id) {
            self.selection.remove(idx);
        } else {
            self.selection.push(id);
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
        self.apply_command(Command::Edit(EditCommand::MoveClip {
            clip,
            to_track: track,
            start: RationalTime::new(start.max(0), self.playhead.rate),
        }))?;
        Ok(())
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
        self.apply_command(Command::Edit(EditCommand::TrimClip {
            clip,
            timeline: TimeRange::at_rate(start, end - start, self.playhead.rate),
        }))?;
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
        let (selected, key) = {
            let engine = self.engine.lock();
            let rate = engine.project().timeline().frame_rate;
            let at = RationalTime::new(self.playhead.value, rate);
            let selected = self
                .selection
                .first()
                .and_then(|clip| selected_detail::SelectedDetail::of(engine.project(), *clip, at));
            (
                selected,
                ProjectionKey::of(&engine, self.playhead, &self.selection),
            )
        };
        let ordered: Vec<u64> = self.selection.iter().map(|id| id.raw()).collect();
        let ids: std::collections::HashSet<u64> = ordered.iter().copied().collect();
        {
            let store = self.store_mut();
            store.selection.clone_from(&ordered);
            for track in &mut store.sequence.tracks {
                for clip in &mut track.clips {
                    clip.selected = ids.contains(&clip.id);
                }
            }
        }
        self.selected = selected;
        self.projected = Some(key);
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
        self.refresh_clock_text();
        self.selected = snapshot.selected;
        self.facts = snapshot.facts;
        self.reconcile_bins();
        self.announce_offline_media();
        self.reconcile_source_view();
        // The magnets are a fact about the sequence, so this is the one moment
        // they can have changed. A drag frame reads the index and never builds
        // one.
        self.snap_index = self.build_snap_index();
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
    pub(crate) fn sync_from_engine(&mut self) {
        let poll = {
            let engine = self.engine.lock();
            // Decided as plain answers first: the arms below write the key back,
            // which they could not do while the match still held it.
            let (covered, playhead_only) = match self.projected.as_ref() {
                Some(key) if key.covers(&engine, self.playhead, &self.selection) => (true, false),
                Some(key) => (false, key.covers_except_playhead(&engine, &self.selection)),
                None => (false, false),
            };
            if covered {
                return;
            }
            if playhead_only {
                // The projection reads the playhead as one field (pinned by
                // `a_projection_reads_the_playhead_only_as_the_playhead`), so the
                // store is already right and the walk would produce the store it
                // already has. What is genuinely sampled at the playhead is the
                // selected clip's detail, so that is all this re-takes: a scrub
                // frame costs one resolve instead of a walk of every track, clip
                // and keyframe plus a magnet index rebuild.
                let rate = engine.project().timeline().frame_rate;
                let at = RationalTime::new(self.playhead.value, rate);
                let selected = self
                    .selection
                    .first()
                    .and_then(|clip| match &self.selected {
                        // Nothing but the playhead moved, so the clip and its
                        // property rows are the values already held: only what is
                        // sampled at this instant is taken again.
                        Some(detail) => Some(detail.resampled(engine.project(), *clip, at)),
                        None => selected_detail::SelectedDetail::of(engine.project(), *clip, at),
                    });
                Poll::Moved(at, selected)
            } else {
                self.projected = Some(ProjectionKey::of(&engine, self.playhead, &self.selection));
                Poll::Rebuilt(Snapshot::from_engine(
                    &engine,
                    self.playhead,
                    &self.selection,
                ))
            }
        };
        match poll {
            Poll::Moved(at, selected) => {
                self.playhead = at;
                self.store_mut().playhead_ticks = at.value;
                self.refresh_clock_text();
                self.selected = selected;
                if let Some(key) = &mut self.projected {
                    key.playhead = at;
                }
            }
            Poll::Rebuilt(snapshot) => {
                // `apply_command` advances this selectively before projection.
                // Any other engine mutation reaches this fallback and drops
                // every cell, so no direct engine path can retain stale pixels.
                self.filmstrips.sync_revision(snapshot.revision);
                self.revision = snapshot.revision;
                self.playhead = snapshot.playhead;
                self.adopt(snapshot);
            }
        }
    }

    /// Re-project unconditionally.
    ///
    /// The caller changed something the projection reads and knows it, so there
    /// is nothing to compare; the key is refreshed on the way through so the
    /// next poll can still short-circuit.
    pub(crate) fn rebuild_store(&mut self) {
        let snapshot = {
            let engine = self.engine.lock();
            self.projected = Some(ProjectionKey::of(&engine, self.playhead, &self.selection));
            Snapshot::from_engine(&engine, self.playhead, &self.selection)
        };
        self.adopt(snapshot);
    }
}

/// Everything a projection is built from.
///
/// `project_to_ui_with` is a pure function of the project plus the playhead,
/// the selection and the three history flags, and the proxy pass that follows
/// it reads only the proxy table. So two equal keys describe the same store,
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
    undo: bool,
    redo: bool,
}

impl ProjectionKey {
    fn of(engine: &Engine, playhead: RationalTime, selection: &[ClipId]) -> Self {
        Self {
            revision: engine.revision(),
            proxy_revision: engine.proxy_revision(),
            playhead,
            selection: selection.to_vec(),
            dirty: engine.is_dirty(),
            undo: engine.can_undo(),
            redo: engine.can_redo(),
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
            && self.undo == engine.can_undo()
            && self.redo == engine.can_redo()
    }
}

/// What a poll found, decided under the lock and applied after it is dropped.
enum Poll {
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
