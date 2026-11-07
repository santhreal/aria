//! `AppState` selection and clip editing, and the [`ReportableError`] values
//! `report_error` accepts.
//!
//! These are inherent methods, so they keep their existing paths — a method
//! belongs to the type, not to the file it is written in. The engine poll and
//! its projection memo are in `engine_sync.rs`.

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
        self.move_selection_by(frames)
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
    ///
    /// This is the edge drag the tool would send, without the magnet, so a key
    /// stops at the same limit a drag does.
    pub fn trim_selected_edge(&mut self, delta: i64) -> Result<(), EngineError> {
        if delta == 0 {
            return Ok(());
        }
        let Some((gesture, clip)) = self.selected_edge_gesture() else {
            return Ok(());
        };
        let drag = crate::surfaces::timeline_gesture::Drag { clip, delta };
        crate::surfaces::timeline_gesture::apply(self, gesture, drag)
    }

    /// The edge drag a key sends to the selected edit point, and the clip it
    /// is on. A roll with no clip across the cut trims the edge instead.
    pub(crate) fn selected_edge_gesture(
        &self,
    ) -> Option<(crate::surfaces::timeline_gesture::Gesture, ClipId)> {
        use crate::surfaces::timeline_gesture::{Gesture, Span};
        use crate::tool_catalog::EdgeDrag;
        let edge = self.selected_edge?;
        let clip = ClipId::from_raw(edge.clip);
        let ui = self.ui_clip_ref(clip)?;
        let left = edge.side == crate::types::TrimSide::In;
        let drag = match self.tool.edge_drag() {
            Some(EdgeDrag::Roll) => {
                let (before, after) = self.abutting(clip);
                (if left { before } else { after }).map(|_| EdgeDrag::Roll)
            }
            other => other,
        }
        .or((self.edit_mode == crate::types::EditMode::Ripple).then_some(EdgeDrag::Ripple));
        let span = Span {
            start: ui.timeline_start_ticks,
            end: ui.timeline_end_ticks,
            threshold: 0,
            magnet: false,
        };
        Some((Gesture::Edge { drag, left, span }, clip))
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

    /// Drop every reference the session holds into the open project: the
    /// clip, edge and gap selections, the clipboard, the parent pick whip,
    /// the open property rows with their open groups and selected keyframes,
    /// the picked pool items, the open folder and its rename, the relink
    /// notes, the Source tabs, and the In and Out marks, which are positions
    /// on the old project's timeline.
    ///
    /// A project file keeps its ids and every launch counts from 1 again, so a
    /// reference kept across a project replacement names nothing in the new
    /// project, or another clip with the same id.
    pub(crate) fn forget_project_references(&mut self) {
        self.clear_selection();
        self.clipboard = None;
        self.parent_whip = None;
        self.expanded_clips = std::sync::Arc::default();
        self.prop_rows.forget_clips();
        self.open_bin(None);
        self.pool_selection.clear();
        self.bin_rename = None;
        self.relink_notes.clear();
        self.mark_in = None;
        self.mark_out = None;
        self.sources.retain(|_| false);
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

    /// Expand `selected` by unlocked grouped companions, and while
    /// [`Self::linked_selection`] is on by unlocked linked companions.
    pub(crate) fn selection_companions(&self, mut selected: Vec<ClipId>) -> Vec<ClipId> {
        let follow_links = self.linked_selection;
        let mut links: Vec<u64> = Vec::new();
        let mut groups: Vec<u64> = Vec::new();
        for &id in &selected {
            let Some(clip) = self.ui_clip_ref(id) else {
                continue;
            };
            if follow_links
                && let Some(link) = clip.linked_to
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
        if links.is_empty() && groups.is_empty() {
            return selected;
        }
        loop {
            let mut grew = false;
            for track in &self.store.sequence.tracks {
                if track.locked {
                    continue;
                }
                for clip in &track.clips {
                    if clip.locked {
                        continue;
                    }
                    let link = clip.linked_to.filter(|_| follow_links);
                    let matches = link.is_some_and(|l| links.contains(&l))
                        || clip.grouped_to.is_some_and(|g| groups.contains(&g));
                    if matches {
                        let member = ClipId::from_raw(clip.id);
                        if !selected.contains(&member) {
                            selected.push(member);
                            grew = true;
                        }
                        if let Some(l) = link
                            && !links.contains(&l)
                        {
                            links.push(l);
                            grew = true;
                        }
                        if let Some(g) = clip.grouped_to
                            && !groups.contains(&g)
                        {
                            groups.push(g);
                            grew = true;
                        }
                    }
                }
            }
            if !grew {
                break;
            }
        }
        selected
    }

    /// Track Select Forward (`A`) or Backward (`Shift+A`) from `at_tick`.
    ///
    /// Selects clips on `track` when `all_tracks` is `false`, or across every
    /// unlocked lane when `all_tracks` is `true` (`Shift` modifier on click).
    /// Grouped companions, and while [`Self::linked_selection`] is on linked
    /// companions, on unlocked lanes are included.
    pub fn select_track_clips(
        &mut self,
        track: TrackId,
        at_tick: i64,
        dir: crate::tool_catalog::TrackSelectDir,
        all_tracks: bool,
    ) {
        let at_tick = at_tick.max(0);
        let mut selected: Vec<ClipId> = Vec::new();
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
                }
            }
        }
        self.selection = self.selection_companions(selected);
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
        let at = RationalTime::new(at_tick, self.playhead.rate);
        self.one_step(|this| this.cut_clips_at(&targets, at))
    }

    /// Cut each of `clips` that still spans `at`, inside the caller's undo
    /// step, and link the tails whose heads share a link
    /// ([`Project::split_tail_links`]): a picture and its sound stay paired on
    /// both sides of the cut, and a tail cut without its partners stays
    /// unlinked.
    ///
    /// [`Project::split_tail_links`]: aria_models::Project::split_tail_links
    pub(crate) fn cut_clips_at(
        &mut self,
        clips: &[ClipId],
        at: RationalTime,
    ) -> Result<(), EngineError> {
        let mut cuts = Vec::with_capacity(clips.len());
        for &clip in clips {
            let spans = self.ui_clip_ref(clip).is_some_and(|ui| {
                at.value > ui.timeline_start_ticks && at.value < ui.timeline_end_ticks
            });
            if !spans {
                continue;
            }
            let outcome = self.apply_command(Command::Edit(EditCommand::SplitClip { clip, at }))?;
            if let ApplyOutcome::Edited(aria_commands::EditOutcome::Created(tail)) = outcome {
                cuts.push((clip, tail));
            }
        }
        let groups = self.engine.lock().project().split_tail_links(&cuts);
        for clips in groups {
            self.apply_command(Command::Edit(EditCommand::LinkClips { clips }))?;
        }
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

    /// Select `id` (with its link when `with_link` is set), or when `id` is
    /// already in the selection keep the selected set and put `id` first so
    /// Effect Controls and the property rows follow the pressed clip.
    pub fn focus_or_select_clip(&mut self, id: ClipId, with_link: bool) {
        if !with_link {
            self.select_clip_linked(id, false);
            return;
        }
        let Some(idx) = self.selection.iter().position(|c| *c == id) else {
            self.select_clip_linked(id, true);
            return;
        };
        if idx > 0 {
            let clip = self.selection.remove(idx);
            self.selection.insert(0, clip);
        }
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
        if let Err(err) = self.refresh_moments() {
            self.report_error(err);
        }
    }

    pub fn toggle_clip_in_selection(&mut self, id: ClipId) {
        let members = self.selection_companions(vec![id]);
        if self.selection.contains(&id) {
            self.selection.retain(|c| !members.contains(c));
        } else {
            for member in members {
                if !self.selection.contains(&member) {
                    self.selection.push(member);
                }
            }
        }
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
    }

    pub fn add_clip_to_selection(&mut self, id: ClipId) {
        if !self.selection.contains(&id) {
            self.selection.push(id);
        }
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
    }

    /// The move a clip drag drops: it covers what it lands on, by the rule of
    /// an overwrite edit. With linked selection off its linked companions stay
    /// where they are.
    pub fn move_clip(
        &mut self,
        clip: ClipId,
        to_track: TrackId,
        start: i64,
    ) -> Result<(), EngineError> {
        self.snap_guide = None;
        let start = RationalTime::new(start.max(0), self.playhead.rate);
        let command = if self.linked_selection {
            EditCommand::OverwriteMoveClip {
                clip,
                to_track,
                start,
            }
        } else {
            EditCommand::OverwriteMoveClipUnlinked {
                clip,
                to_track,
                start,
            }
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }

    /// The move a step chord, a typed offset or a slide sends: one that refuses
    /// to land on another clip. It includes the clip's linked companions, or
    /// with linked selection off it leaves them.
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

    /// Move `clip` alone along its lane until it places its file where the
    /// linked partner [`aria_models::Project::link_drifts`] measures it
    /// against does, so the offset its bar draws clears. The partner stays
    /// where it is. A clip already in sync, or one the move would start
    /// before the sequence, stays put with the reason on the status bar; a
    /// move onto another clip fails as any move without links does.
    pub fn move_into_sync(&mut self, clip: ClipId) -> Result<(), EngineError> {
        let (drift, start, rate) = {
            let engine = self.engine.lock();
            let project = engine.project();
            let model = project
                .clip(clip)
                .ok_or(aria_models::ModelError::UnknownClip(clip))?;
            (
                project.link_drifts().get(&clip).copied(),
                model.timeline.start.value,
                project.timeline().frame_rate,
            )
        };
        let Some(drift) = drift else {
            self.status_message = Some("The clip is in sync with its linked clips".into());
            return Ok(());
        };
        let to = start - drift;
        if to < 0 {
            self.status_message =
                Some("Moving the clip into sync would start it before the sequence".into());
            return Ok(());
        }
        let to_track = self
            .track_of(clip)
            .ok_or(aria_models::ModelError::UnknownClip(clip))?;
        self.snap_guide = None;
        self.apply_command(Command::Edit(EditCommand::MoveClipUnlinked {
            clip,
            to_track,
            start: RationalTime::new(to, rate),
        }))?;
        self.status_message = Some("Moved into sync".into());
        Ok(())
    }

    /// Read where a held clip drag lets go, for the frame that draws it
    /// there: a move or a ripple move sets [`Self::held_move`] and the magnet
    /// it lands on, any other drag clears it. The clips the move takes are
    /// those [`Self::move_clip`] moves, read once per drag off `project`, the
    /// project the drag holds.
    pub(crate) fn hold_move(
        &mut self,
        gesture: crate::surfaces::timeline_gesture::Gesture,
        drag: crate::surfaces::timeline_gesture::Drag,
        project: &aria_models::Project,
    ) {
        use crate::surfaces::timeline_gesture::{Gesture, landing};
        let (track, span, ripple) = match gesture {
            Gesture::Body {
                drag: None,
                track,
                span,
            } => (track, span, false),
            Gesture::RippleBody { track, span } => (track, span, true),
            Gesture::Body { drag: Some(_), .. }
            | Gesture::Edge { .. }
            | Gesture::Transition { .. }
            | Gesture::Fade { .. } => {
                self.held_move = None;
                return;
            }
        };
        let (start, guide) = landing(self, &drag, &span);
        self.snap_guide = guide;
        let linked = self.linked_selection;
        if let Some(held) = &mut self.held_move
            && held.linked == linked
            && held.members.first().map(|member| member.id) == Some(drag.clip.raw())
        {
            held.track = track.raw();
            held.start = start;
            held.ripple = ripple;
            return;
        }
        let companions = if linked {
            project.move_companions(drag.clip)
        } else {
            project.group_companions(drag.clip)
        };
        let members: std::sync::Arc<[UiClip]> = std::iter::once(drag.clip)
            .chain(companions.into_iter().filter(|id| *id != drag.clip))
            .filter_map(|id| self.ui_clip_ref(id).cloned())
            .collect();
        self.held_move = (!members.is_empty()).then(|| crate::types::HeldMove {
            members,
            linked,
            track: track.raw(),
            start,
            ripple,
        });
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
        self.snap_guide = None;
        self.apply_command(Command::Edit(self.trim_command(
            clip,
            TimeRange::at_rate(start_ticks, end_ticks - start_ticks, self.playhead.rate),
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

    /// Reset the selected picture clip's transform to the default, dropping
    /// its animation. A clip already at the default declines.
    pub fn reset_selected_transform(&mut self) -> Result<(), EngineError> {
        if self.selected_has_framing() && !self.selected_transform_resets() {
            self.decline(super::decline::Decline::DefaultTransform);
            return Ok(());
        }
        self.apply_transform(UiClipTransform::default(), None)
    }

    /// Whether Reset Transform changes the selected clip: its transform is
    /// animated or holds a value off the default. Found by running the model's
    /// own reset on a copy, so it holds whatever the reset writes. The Viewer
    /// menu and the reset read this.
    pub fn selected_transform_resets(&self) -> bool {
        self.selected_model().is_some_and(|model| {
            let mut reset = model.transform.clone();
            reset.set_constant(UiClipTransform::default().into());
            reset != model.transform
        })
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

    /// Open the menu on `target` at the pointer, `x` and `y` in window
    /// coordinates. The drawn menu places itself against the pointer from its
    /// measured size.
    ///
    /// A menu on a keyframe selects it unless it is already in the selection,
    /// so the rows that act on the selected keyframes act on it. A menu on a
    /// Path keyframe makes its mask the active one, as a press on it does.
    pub fn open_context(&mut self, target: ContextTarget, x: f32, y: f32) {
        match target {
            ContextTarget::Keyframe { clip, param, tick } => {
                self.select_timeline_keyframe(ClipId::from_raw(clip), param, tick, false);
            }
            ContextTarget::MaskPathKey { clip, mask, .. } => {
                self.focus_mask_path(ClipId::from_raw(clip), mask);
            }
            _ => {}
        }
        self.context_plugin_rows = self.plugin_menu_rows(&target);
        self.context_menu = Some(target);
        self.context_at = (x, y);
    }

    pub fn close_context(&mut self) {
        self.context_menu = None;
    }

    /// Post `err` on the error line, started with a capital when it is written
    /// in lower case, as an error from below the editor is. A refusal of an
    /// edit of the selection is marked so a new selection clears it.
    pub fn report_error(&mut self, err: impl ReportableError) {
        let typed = err.refuses_the_selection();
        let line = super::status::as_sentence(err.into_error_message());
        self.error_on_selection = typed || super::status::about_selection(&line);
        self.last_error = Some(line);
    }

    pub fn persist_settings_later(&mut self, cx: &mut Context<Self>) {
        self.save_generation = self.save_generation.saturating_add(1);
        let generation = self.save_generation;
        let settings = self.ui_settings.clone();
        let data_dir = self.data_dir.clone();
        let written = std::sync::Arc::clone(&self.settings_written);
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(SAVE_DEBOUNCE_MS))
                .await;
            let latest = this
                .update(cx, |state, _cx| state.save_generation == generation)
                .unwrap_or(false);
            if !latest {
                return;
            }
            // The write syncs the file to disk: no work for the UI thread.
            let saved = cx
                .background_executor()
                .spawn(async move { write_settings(&written, generation, &settings, &data_dir) })
                .await;
            if let Err(err) = saved {
                let reason = aria_models::io_reason(&err);
                let _ = this.update(cx, |state, cx| {
                    state.report_error(format!(
                        "The settings are in force but could not be saved for the next \
                         launch: {reason}"
                    ));
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Write `ui-settings.json` now, and drop a debounced write still waiting:
    /// it holds an older copy.
    pub(crate) fn write_settings_now(&mut self) -> std::io::Result<()> {
        self.save_generation = self.save_generation.saturating_add(1);
        write_settings(
            &self.settings_written,
            self.save_generation,
            &self.ui_settings,
            &self.data_dir,
        )
    }
}

/// Write `settings` as generation `generation`, unless a newer generation has
/// landed already.
pub(super) fn write_settings(
    written: &parking_lot::Mutex<u64>,
    generation: u64,
    settings: &UiSettings,
    data_dir: &std::path::Path,
) -> std::io::Result<()> {
    let mut last = written.lock();
    if *last > generation {
        return Ok(());
    }
    settings.save(data_dir)?;
    *last = generation;
    Ok(())
}

pub trait ReportableError {
    fn into_error_message(self) -> String;

    /// Whether the error refuses an edit of the selected clips. A line of
    /// text answers by its wording instead, in [`AppState::report_error`].
    fn refuses_the_selection(&self) -> bool {
        false
    }
}

impl ReportableError for &EngineError {
    fn into_error_message(self) -> String {
        // The copy names the next action; the log keeps the cause behind it.
        tracing::warn!(error = %self, "engine error reported");
        describe_engine_error(self)
    }

    fn refuses_the_selection(&self) -> bool {
        super::error_copy::refuses_the_selection(self)
    }
}

impl ReportableError for EngineError {
    fn into_error_message(self) -> String {
        (&self).into_error_message()
    }

    fn refuses_the_selection(&self) -> bool {
        super::error_copy::refuses_the_selection(self)
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
