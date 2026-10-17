//! What the keyboard map's editing, marking and navigation keys do.
//!
//! Each verb here is the body of one or two shipped bindings in
//! `keys::bindings_*`. They read the projected sequence and act on the targeted
//! lanes, the selection, or the In to Out range, and every multi-command edit is
//! one undo step through [`AppState::one_step`]. A press that has nothing to act
//! on writes a status line rather than doing nothing silently.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipId, RationalTime, TrackId};

use super::AppState;
use crate::types::{ActivePanel, SidebarTab, TrimSide, UiClip, UiTrack};

/// How much one track-height step grows or shrinks every lane.
const TRACK_HEIGHT_STEP: f32 = 1.25;

impl AppState {
    /// The lanes an edit that names no lane acts on: targeted and unlocked.
    fn targeted_lanes(&self) -> impl Iterator<Item = &UiTrack> {
        self.store
            .sequence
            .tracks
            .iter()
            .filter(|lane| lane.targeted && !lane.locked)
    }

    /// The unlocked clips under the playhead on the targeted lanes, a clip
    /// starting on the playhead included and one ending on it excluded.
    fn clips_at_playhead(&self) -> impl Iterator<Item = &UiClip> {
        let at = self.playhead.value;
        self.targeted_lanes()
            .flat_map(|lane| lane.clips.iter())
            .filter(move |clip| {
                !clip.locked && clip.timeline_start_ticks <= at && at < clip.timeline_end_ticks
            })
    }

    /// The clip a playhead trim acts on: the selected clip when the playhead is
    /// inside it, otherwise the first targeted clip under the playhead.
    fn playhead_trim_target(&self) -> Option<UiClip> {
        let at = self.playhead.value;
        let inside = |clip: &UiClip| clip.timeline_start_ticks < at && at < clip.timeline_end_ticks;
        if let Some(selected) = self.selected_ui_clip_ref()
            && inside(selected)
        {
            return Some(selected.clone());
        }
        self.clips_at_playhead().find(|clip| inside(clip)).cloned()
    }

    /// Every cut on the lanes `all` names, sorted and without repeats: each
    /// clip's start and end. `all` takes every lane; otherwise the targeted ones.
    fn edit_points(&self, all: bool) -> Vec<i64> {
        let mut points: Vec<i64> = self
            .store
            .sequence
            .tracks
            .iter()
            .filter(|lane| all || (lane.targeted && !lane.locked))
            .flat_map(|lane| lane.clips.iter())
            .flat_map(|clip| [clip.timeline_start_ticks, clip.timeline_end_ticks])
            .collect();
        points.sort_unstable();
        points.dedup();
        points
    }

    /// Move the playhead to the next or previous cut on the targeted lanes, or
    /// on every lane when `all` is set.
    pub fn go_to_edit(&mut self, forward: bool, all: bool) {
        let at = self.playhead.value;
        let points = self.edit_points(all);
        let found = if forward {
            points.into_iter().find(|tick| *tick > at)
        } else {
            points.into_iter().rev().find(|tick| *tick < at)
        };
        match found {
            Some(tick) => self.set_playhead(tick),
            None if forward => self.status_message = Some("No later edit".into()),
            None => self.status_message = Some("No earlier edit".into()),
        }
    }

    /// Add Edit: cut at the playhead. The selected clips under the playhead
    /// when any are selected, otherwise every targeted lane; `all` cuts every
    /// unlocked lane whatever is selected.
    pub fn add_edit(&mut self, all: bool) -> Result<(), EngineError> {
        let at = self.playhead.value;
        let spans = |clip: &&UiClip| {
            !clip.locked && clip.timeline_start_ticks < at && at < clip.timeline_end_ticks
        };
        let targets: Vec<ClipId> = if all {
            self.store
                .sequence
                .tracks
                .iter()
                .filter(|lane| !lane.locked)
                .flat_map(|lane| lane.clips.iter())
                .filter(spans)
                .map(|clip| ClipId::from_raw(clip.id))
                .collect()
        } else if self.selection.is_empty() {
            self.targeted_lanes()
                .flat_map(|lane| lane.clips.iter())
                .filter(spans)
                .map(|clip| ClipId::from_raw(clip.id))
                .collect()
        } else {
            self.selection
                .iter()
                .filter(|id| self.ui_clip_ref(**id).as_ref().is_some_and(spans))
                .copied()
                .collect()
        };
        if targets.is_empty() {
            self.status_message = Some("No clip under the playhead to cut".into());
            return Ok(());
        }
        let cut = RationalTime::new(at, self.playhead.rate);
        self.one_step(|this| {
            for clip in targets {
                this.apply_command(Command::Edit(EditCommand::SplitClip { clip, at: cut }))?;
            }
            Ok(())
        })
    }

    /// Cut the unlocked clip on `lane` that `tick` falls strictly inside.
    fn cut_lane_at(&mut self, lane: u64, tick: i64) -> Result<(), EngineError> {
        let inside = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|track| track.id == lane)
            .and_then(|track| {
                track.clips.iter().find(|clip| {
                    !clip.locked
                        && clip.timeline_start_ticks < tick
                        && tick < clip.timeline_end_ticks
                })
            })
            .map(|clip| ClipId::from_raw(clip.id));
        if let Some(clip) = inside {
            let at = RationalTime::new(tick, self.playhead.rate);
            self.apply_command(Command::Edit(EditCommand::SplitClip { clip, at }))?;
        }
        Ok(())
    }

    /// Lift (`close_gap` false) or Extract (`close_gap` true): remove the In to
    /// Out range from every targeted lane, as one undo step. Extract also slides
    /// each lane's later clips left by the range's length.
    pub fn remove_marked_range(&mut self, close_gap: bool) -> Result<(), EngineError> {
        let Some((from, to)) = self.marked_range() else {
            self.status_message = Some("Mark In and Out first".into());
            return Ok(());
        };
        let lanes: Vec<u64> = self.targeted_lanes().map(|lane| lane.id).collect();
        if lanes.is_empty() {
            self.status_message = Some("Target a track first".into());
            return Ok(());
        }
        let rate = self.playhead.rate;
        let removed = self.one_step(|this| {
            let mut removed = 0_usize;
            for lane in lanes {
                this.cut_lane_at(lane, to)?;
                this.cut_lane_at(lane, from)?;
                let (inside, later): (Vec<ClipId>, bool) = this
                    .store
                    .sequence
                    .tracks
                    .iter()
                    .find(|track| track.id == lane)
                    .map(|track| {
                        let inside = track
                            .clips
                            .iter()
                            .filter(|clip| {
                                !clip.locked
                                    && clip.timeline_start_ticks >= from
                                    && clip.timeline_end_ticks <= to
                            })
                            .map(|clip| ClipId::from_raw(clip.id))
                            .collect();
                        let later = track
                            .clips
                            .iter()
                            .any(|clip| clip.timeline_start_ticks >= to);
                        (inside, later)
                    })
                    .unwrap_or_default();
                for clip in inside {
                    this.apply_command(Command::Edit(EditCommand::RemoveClip { clip }))?;
                    removed += 1;
                }
                if close_gap && later {
                    this.apply_command(Command::Edit(EditCommand::ShiftClips {
                        track: TrackId::from_raw(lane),
                        from: RationalTime::new(to, rate),
                        delta: RationalTime::new(from - to, rate),
                    }))?;
                }
            }
            Ok(removed)
        })?;
        self.clear_selection();
        self.set_playhead(from);
        let verb = if close_gap { "Extracted" } else { "Lifted" };
        self.status_message = Some(format!("{verb} {removed} clip(s) from In to Out"));
        Ok(())
    }

    /// Delete: remove the selected clips and leave their gaps. A selected gap
    /// is closed instead, which is what deleting a gap means.
    pub fn clear_selected(&mut self) -> Result<(), EngineError> {
        if let Some(gap) = self.selected_gap.take() {
            return self.close_gap(gap);
        }
        let clips = self.selection.clone();
        if clips.is_empty() {
            return Ok(());
        }
        self.one_step(|this| {
            for clip in clips {
                if this.ui_clip_ref(clip).is_some() {
                    this.apply_command(Command::Edit(EditCommand::RemoveClip { clip }))?;
                }
            }
            Ok(())
        })?;
        self.clear_selection();
        Ok(())
    }

    /// Q / W: trim the head or tail of the clip under the playhead to the
    /// playhead and close the gap the trim leaves.
    pub fn ripple_trim_to_playhead(&mut self, head: bool) -> Result<(), EngineError> {
        let Some(clip) = self.playhead_trim_target() else {
            self.status_message = Some("No clip under the playhead to trim".into());
            return Ok(());
        };
        let at = self.playhead.value;
        let (start, end) = (clip.timeline_start_ticks, clip.timeline_end_ticks);
        let track = TrackId::from_raw(clip.track_id);
        let id = ClipId::from_raw(clip.id);
        if head {
            self.ripple_trim(id, track, start, end, true, at - start)?;
            // The head now sits where the clip used to start, and so does the
            // frame that was under the playhead.
            self.set_playhead(start);
        } else {
            self.ripple_trim(id, track, start, end, false, at - end)?;
        }
        Ok(())
    }

    /// Alt+[ / Alt+]: trim the head or tail of the clip under the playhead to
    /// the playhead, leaving the gap.
    pub fn trim_to_playhead(&mut self, head: bool) -> Result<(), EngineError> {
        let Some(clip) = self.playhead_trim_target() else {
            self.status_message = Some("No clip under the playhead to trim".into());
            return Ok(());
        };
        let at = self.playhead.value;
        let (start, end) = if head {
            (at, clip.timeline_end_ticks)
        } else {
            (clip.timeline_start_ticks, at)
        };
        self.trim_clip(ClipId::from_raw(clip.id), start, end)
    }

    /// Ctrl+arrow: trim the selected edge by `frames`, or move the selected
    /// clip when no edge is selected.
    pub fn trim_selected_edges(&mut self, frames: i64) -> Result<(), EngineError> {
        if self.selected_edge.is_none() && self.selection.is_empty() {
            self.status_message = Some("Select an edit point or a clip first".into());
            return Ok(());
        }
        self.nudge_selected(frames)
    }

    /// D: select the clips under the playhead on the targeted lanes.
    pub fn select_at_playhead(&mut self) {
        let picked: Vec<ClipId> = self
            .clips_at_playhead()
            .map(|clip| ClipId::from_raw(clip.id))
            .collect();
        if picked.is_empty() {
            self.status_message = Some("No clip under the playhead on a targeted track".into());
            return;
        }
        self.selection = picked;
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
    }

    /// X: mark In and Out around the clip under the playhead on the targeted
    /// lanes.
    pub fn mark_clip_at_playhead(&mut self) {
        let Some((from, to, name)) = self.clips_at_playhead().next().map(|clip| {
            (
                clip.timeline_start_ticks,
                clip.timeline_end_ticks,
                clip.name.clone(),
            )
        }) else {
            self.status_message = Some("No clip under the playhead on a targeted track".into());
            return;
        };
        self.mark_in = Some(from);
        self.mark_out = Some(to);
        self.status_message = Some(format!("Marked {name}"));
    }

    /// Ctrl+Shift+X: clear In and Out, on the focused Source tab when one has
    /// the keys.
    pub fn clear_marks_here(&mut self) {
        if self.sources.focused().is_some() {
            self.clear_source_marks();
            return;
        }
        self.clear_mark_in();
        self.clear_mark_out();
        self.status_message = Some("Cleared In and Out".into());
    }

    /// M: add a marker at the playhead. Pressed on a marker that already sits
    /// on the playhead's frame, it opens the Marker Edit dialog for that marker.
    pub fn add_or_edit_marker(&mut self) -> Result<(), EngineError> {
        let at = self.playhead.value;
        let here = self
            .store
            .sequence
            .markers
            .iter()
            .find(|marker| marker.tick == at)
            .map(|marker| marker.id);
        match here {
            Some(marker) => {
                self.open_marker_edit(marker);
                Ok(())
            }
            None => self.add_marker_at_playhead(),
        }
    }

    /// Ctrl+Shift+Alt+M: delete every marker, as one undo step.
    pub fn clear_all_markers(&mut self) -> Result<(), EngineError> {
        let markers: Vec<u64> = self
            .store
            .sequence
            .markers
            .iter()
            .map(|marker| marker.id)
            .collect();
        if markers.is_empty() {
            self.status_message = Some("No markers to clear".into());
            return Ok(());
        }
        let count = markers.len();
        self.one_step(|this| {
            for marker in markers {
                this.remove_marker(marker)?;
            }
            Ok(())
        })?;
        self.status_message = Some(format!("Cleared {count} marker(s)"));
        Ok(())
    }

    /// Ctrl+L: link the selection, or unlink it when it is already one group.
    pub fn toggle_link_selected(&mut self) -> Result<(), EngineError> {
        if self.selection_is_linked() {
            self.unlink_selected()
        } else {
            self.link_selected()
        }
    }

    /// S: snapping on or off.
    pub fn toggle_snapping(&mut self) {
        self.snap_enabled = !self.snap_enabled;
        self.status_message = Some(
            if self.snap_enabled {
                "Snapping on"
            } else {
                "Snapping off"
            }
            .into(),
        );
    }

    /// Sequence > Linked Selection on or off.
    pub fn toggle_linked_selection(&mut self) {
        self.linked_selection ^= true;
        self.status_message = Some(
            if self.linked_selection {
                "Linked selection on: linked clips select, move, and trim together"
            } else {
                "Linked selection off: linked clips select, move, and trim alone; groups stay together"
            }
            .into(),
        );
    }

    /// Ctrl+= / Ctrl+-: every lane taller or shorter by one step, as one undo
    /// step. The engine clamps each height into the lane's range.
    pub fn step_track_heights(&mut self, taller: bool) -> Result<(), EngineError> {
        let lanes: Vec<(u64, f32)> = self
            .store
            .sequence
            .tracks
            .iter()
            .map(|lane| (lane.id, lane.height))
            .collect();
        if lanes.is_empty() {
            return Ok(());
        }
        self.one_step(|this| {
            for (lane, height) in lanes {
                let next = if taller {
                    height * TRACK_HEIGHT_STEP
                } else {
                    height / TRACK_HEIGHT_STEP
                };
                this.set_lane_height(lane, next)?;
            }
            Ok(())
        })
    }

    /// `]` / `[`: the selected clip's gain up or down by `decibels`.
    pub fn step_selected_gain(&mut self, decibels: f32) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            self.status_message = Some("Select a clip first".into());
            return Ok(());
        }
        self.nudge_selected_volume_db(decibels)
    }

    /// Shift+Alt+Left / Shift+Alt+Right: the playhead to the selected clip's
    /// previous or next keyframe on any property.
    pub fn go_to_keyframe(&mut self, forward: bool) {
        let at = self.playhead.value;
        let Some(clip) = self.selected_ui_clip_ref() else {
            self.status_message = Some("Select a clip first".into());
            return;
        };
        let start = clip.timeline_start_ticks;
        let ticks = clip.keyframe_ticks.iter().map(|local| start + local);
        let found = if forward {
            ticks.filter(|tick| *tick > at).min()
        } else {
            ticks.filter(|tick| *tick < at).max()
        };
        match found {
            Some(tick) => self.set_playhead(tick),
            None if forward => self.status_message = Some("No later keyframe".into()),
            None => self.status_message = Some("No earlier keyframe".into()),
        }
    }

    /// Ctrl+T (picture) and Ctrl+Shift+T (audio): the default transition of
    /// that kind at the selected edge, or at the cut nearest the playhead among
    /// the targeted lanes that take it. A selected edge on a lane of the other
    /// kind declines with the chord that fits it.
    pub fn add_default_transition(
        &mut self,
        media: aria_models::TransitionMedia,
    ) -> Result<(), EngineError> {
        let Some(id) = super::clip_transitions::default_transition(media) else {
            self.status_message = Some("No transition is registered".into());
            return Ok(());
        };
        let junction = match self.selected_edge {
            Some(edge) if edge.side == TrimSide::Out => Some(ClipId::from_raw(edge.clip)),
            Some(edge) => self
                .abutting(ClipId::from_raw(edge.clip))
                .0
                .map(|left| left.clip),
            None => {
                let at = self.playhead.value;
                self.targeted_lanes()
                    .flat_map(|lane| lane.clips.iter())
                    .filter(|clip| clip.transition_media == Some(media))
                    .min_by_key(|clip| (clip.timeline_end_ticks - at).abs())
                    .map(|clip| ClipId::from_raw(clip.id))
            }
        };
        let Some(clip) = junction else {
            self.status_message = Some("No cut between two clips to put a transition on".into());
            return Ok(());
        };
        self.set_clip_transition(clip, &id)
    }

    /// An arrow in the Viewer: move the selected clip by `dx`, `dy` canvas
    /// pixels. With nothing selected the arrow does what it does on the
    /// timeline: Left and Right step the playhead, Up and Down go to a cut.
    pub fn viewer_arrow(&mut self, dx: f32, dy: f32) -> Result<(), EngineError> {
        // A Source tab has no clip to nudge: Left and Right step its frame.
        if self.source_has_keys() {
            if dy == 0.0 {
                let frames = dx as i64;
                self.step_source(frames.signum() * frames.abs().min(5));
            }
            return Ok(());
        }
        // The pose at the playhead, which is the picture on screen; the
        // projected row holds the clip's first frame.
        let Some(mut transform) = self.selected_pose() else {
            if dy == 0.0 {
                let frames = dx as i64;
                self.set_playhead(self.playhead.value + frames.signum() * frames.abs().min(5));
            } else {
                self.go_to_edit(dy > 0.0, false);
            }
            return Ok(());
        };
        let [width, height] = self.canvas_size();
        // The arrow moves the picture on the canvas; a child's position is
        // written back into its parent's frame.
        [transform.position_x, transform.position_y] = crate::app::canvas_overlay::moved_on_canvas(
            self.selected_parent_frame(),
            [transform.position_x, transform.position_y],
            [dx / width.max(1) as f32, dy / height.max(1) as f32],
        );
        self.set_selected_transform(transform)
    }

    /// Shift+1 to Shift+9: bring a pane forward, opening its tab in its default
    /// frame when closed.
    pub fn focus_pane(&mut self, pane: ActivePanel) {
        let panel_id = pane.panel_id();
        let default_frame = crate::dock::default_frame_for(panel_id);
        self.dock_tree.open_in(panel_id, default_frame);
        match pane {
            ActivePanel::Media => {
                self.ui_settings.sidebar_collapsed = false;
                self.sidebar_tab = SidebarTab::Media;
            }
            ActivePanel::Inspector => self.ui_settings.inspector_collapsed = false,
            ActivePanel::Preview | ActivePanel::Timeline | ActivePanel::Ai | ActivePanel::Mixer => {
            }
        }
        self.active_panel = pane;
        self.sync_scope_demand();
    }

    /// Toggle a dock panel tab (`Shift+6` for Mixer, `Shift+0` for Assistant):
    /// focuses and activates the panel when not active, or returns to the
    /// Viewer when already active.
    pub fn toggle_pane_tab(&mut self, pane: ActivePanel) {
        if self.active_panel == pane && self.dock_tree.is_active(pane.panel_id()) {
            self.focus_pane(ActivePanel::Preview);
        } else {
            self.focus_pane(pane);
        }
    }

    /// Bring a dock panel forward by id, opening its tab in its default frame
    /// when closed and setting `active_panel` to its focus route.
    pub fn focus_dock_panel(&mut self, panel: crate::dock::PanelId) {
        let default_frame = if let Some(host) = crate::plugins::panel_host::host_of(&self.ctx)
            && let Some(entry) = host.get(panel)
            && let Some(dock) = entry.dock
        {
            dock.default_frame
        } else {
            crate::dock::default_frame_for(panel)
        };
        self.dock_tree.open_in(panel, default_frame);
        self.sync_panes_from_dock_tree();
    }

    /// Toggle a dock panel open/closed from the Window menu.
    pub fn toggle_dock_panel(&mut self, panel: crate::dock::PanelId) {
        if self.dock_tree.is_open(panel) {
            self.dock_tree.close(panel);
        } else {
            let default_frame = if let Some(host) = crate::plugins::panel_host::host_of(&self.ctx)
                && let Some(entry) = host.get(panel)
                && let Some(dock) = entry.dock
            {
                dock.default_frame
            } else {
                crate::dock::default_frame_for(panel)
            };
            self.dock_tree.open_in(panel, default_frame);
        }
        self.sync_panes_from_dock_tree();
    }

    /// ` (backquote): maximize the focused frame to fill the editor area, or
    /// restore the split tree.
    pub fn toggle_maximize_focused(&mut self) {
        let focused = self.dock_tree.focused;
        self.dock_tree.toggle_maximize(focused);
    }

    /// Ctrl+Tab / Ctrl+Shift+Tab: cycle `focused` through frames in reading
    /// order.
    pub fn cycle_pane(&mut self, forward: bool) {
        if let Some(frame_id) = self.dock_tree.find(self.active_panel.panel_id())
            && self
                .dock_tree
                .frame(self.dock_tree.focused)
                .and_then(|f| ActivePanel::from_panel_id(f.active_panel()))
                != Some(self.active_panel)
            && self
                .dock_tree
                .frame(self.dock_tree.focused)
                .is_some_and(|f| ActivePanel::from_panel_id(f.active_panel()).is_some())
        {
            self.dock_tree.focused = frame_id;
        }
        if forward {
            self.dock_tree.next_frame();
        } else {
            self.dock_tree.prev_frame();
        }
        self.sync_panes_from_dock_tree();
    }

    /// Ctrl+F: find in the project: the media pool forward with its search
    /// box holding the keyboard.
    pub fn find_in_project(&mut self) {
        self.focus_pane(ActivePanel::Media);
        self.focus_media_search(true);
    }

    /// Ctrl+Alt+K: Settings, filtered to the key bindings.
    pub fn open_keyboard_shortcuts(&mut self) {
        self.settings_form.filter = "Keys".into();
        self.open_dialog(super::DialogKind::Settings);
    }

    /// Ctrl+Alt+S: write the project to a chosen file without adopting it. The
    /// open project keeps its own path and its unsaved state.
    pub fn save_project_copy(&mut self) {
        use super::picker::{Ask, FileSpec};
        self.request_pick(
            Ask::SaveFile(FileSpec::new("Save a Copy").filter("Aria project", &["aria"])),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                let written = state.engine.lock().project().save_to_file(&path);
                match written {
                    Ok(()) => {
                        state.status_message = Some(format!("Saved a copy to {}", path.display()));
                    }
                    Err(err) => state.report_error(format!(
                        "Could not save a copy to {}: {err}. Choose a folder you can write to.",
                        path.display()
                    )),
                }
            },
        );
    }

    /// Ctrl+X: copy the selection, then clear it from the timeline.
    pub fn cut_selected(&mut self) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            return Ok(());
        }
        self.copy_selected();
        self.clear_selected()
    }

    /// Shift+Alt+1 to Shift+Alt+9: the workspace in that place of the
    /// switcher.
    pub(crate) fn switch_workspace_slot(&mut self, slot: usize, cx: &mut gpui::Context<Self>) {
        let Some(id) = self.workspace_rows().get(slot).map(|row| row.id) else {
            self.status_message = Some(format!("No workspace in place {}", slot + 1));
            return;
        };
        self.apply_workspace(&id, cx);
    }

    /// Shift+2: the Viewer forward. Pressed with the Viewer already focused it
    /// switches between the program and the source shown last, with that
    /// source's position and marks.
    pub fn viewer_key(&mut self) {
        if self.active_panel != ActivePanel::Preview {
            self.focus_pane(ActivePanel::Preview);
            return;
        }
        let from_program = self.sources.focus_index().is_none();
        if !self.sources.swap_program_and_recent() {
            self.status_message = Some("No source has been open in the Viewer".into());
            return;
        }
        if from_program {
            if self.playing {
                self.toggle_playback();
            }
            self.reconcile_source_view();
        }
    }

    /// The dock panel the keyboard is in.
    ///
    /// The focused frame's tab when its key route is the route keystrokes
    /// take, or when it has no route of its own. Otherwise the route's own
    /// panel, because a click inside a pane moves the route without focusing
    /// its frame. Several panels share one route, such as Effects with the pool
    /// and Info with the Inspector, so an action on the focused panel reads
    /// this rather than the route.
    pub fn keyed_panel(&self) -> crate::dock::PanelId {
        self.dock_tree
            .frame(self.dock_tree.focused)
            .map(|frame| frame.active_panel())
            .filter(|panel| {
                ActivePanel::from_panel_id(panel).is_none_or(|route| route == self.active_panel)
            })
            .unwrap_or_else(|| self.active_panel.panel_id())
    }

    /// Whether the keyboard is over the pool: the Project panel holds the
    /// keys, on a sidebar tab that draws the pool.
    pub(crate) fn pool_has_keys(&self) -> bool {
        self.keyed_panel() == crate::dock::PANEL_PROJECT && self.sidebar_tab.shows_pool()
    }

    /// Ctrl+W: close the active tab of the focused frame. In the Timeline it
    /// closes the open nested sequence and returns to the root first; in the
    /// Viewer it closes the focused Source tab first.
    pub fn close_focused(&mut self) -> Result<(), EngineError> {
        if self.active_panel == ActivePanel::Preview
            && let Some(index) = self.sources.focus_index()
        {
            self.close_source_tab(index);
            return Ok(());
        }
        if self.active_panel == ActivePanel::Timeline && self.store.open_sequence.is_some() {
            return self.leave_sequence();
        }
        if !self.dock_tree.close(self.keyed_panel()) {
            match self.active_panel {
                ActivePanel::Media => {
                    self.ui_settings.sidebar_collapsed = true;
                    self.active_panel = ActivePanel::Timeline;
                }
                ActivePanel::Inspector => {
                    self.ui_settings.inspector_collapsed = true;
                    self.active_panel = ActivePanel::Timeline;
                }
                ActivePanel::Ai | ActivePanel::Mixer => self.active_panel = ActivePanel::Preview,
                ActivePanel::Preview => {
                    self.status_message = Some("The Program tab cannot be closed".into());
                }
                ActivePanel::Timeline => {
                    self.status_message = Some("The root sequence has no tab to close".into());
                }
            }
        } else {
            self.sync_panes_from_dock_tree();
        }
        Ok(())
    }
}
