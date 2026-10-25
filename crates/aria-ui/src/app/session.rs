//! Project open/save, Home, dialogs, and file picks.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_engine::{ApplyOutcome, EngineError};
use aria_models::{ClipId, Filter, MediaId};
use aria_plugin::DropAction;

use super::picker::{Ask, FileSpec};
use super::{AppState, DialogKind};
use crate::types::{AppMenu, HomeRail, UiRoute};

impl AppState {
    pub fn save_project(&mut self) -> Result<(), EngineError> {
        let path = self
            .project_path
            .clone()
            .unwrap_or_else(|| self.data_dir.join("untitled.aria"));
        self.project_path = Some(path.clone());
        self.apply_command(Command::Project(ProjectCommand::Save {
            path: path.clone(),
        }))?;
        self.remember_recent(&path);
        self.clear_recovery();
        // The conversation is keyed by the project's path, so it can only be
        // written once the project has one. A first save is what starts keeping
        // it, which is also when there is something to come back to.
        if let Err(err) = self.save_conversation() {
            tracing::warn!(error = %err, "could not save the conversation");
        }
        self.status_message = Some("Saved".into());
        Ok(())
    }

    pub fn open_project(&mut self, path: PathBuf) -> Result<(), EngineError> {
        crate::projection::invalidate_media_statuses();
        match self.apply_command(Command::Project(ProjectCommand::Open {
            path: path.clone(),
        })) {
            Ok(_) => {}
            // The structure is readable either way — what is missing is the
            // media it points at — so this falls through to the load that
            // skips resolution rather than repeating what it does, and the
            // operator is told which sources did not resolve.
            Err(EngineError::MissingMedia(missing)) => {
                self.load_project(path)?;
                self.report_error(EngineError::MissingMedia(missing));
                return Ok(());
            }
            Err(err) => return Err(err),
        }
        self.adopt_project(path);
        Ok(())
    }

    /// Open a project's structure without resolving its media.
    ///
    /// What a recovery offer and a relink-later open both want: the sequence,
    /// the pool rows, and the settings, with offline sources left offline.
    pub fn load_project(&mut self, path: PathBuf) -> Result<(), EngineError> {
        crate::projection::invalidate_media_statuses();
        self.apply_command(Command::Project(ProjectCommand::Load {
            path: path.clone(),
        }))?;
        self.adopt_project(path);
        Ok(())
    }

    /// Everything the session owes a project that just became the open one,
    /// whichever command loaded it.
    ///
    /// Media ids are per-session, so another project's peaks must not be read
    /// for this one's sources; the conversation is keyed by the path, so the
    /// path is adopted before it is read.
    fn adopt_project(&mut self, path: PathBuf) {
        self.waveforms.clear();
        self.clear_recovery();
        // Remembered before the path is adopted, so the list is built from the
        // path this call was handed rather than read back out of the state.
        self.remember_recent(&path);
        self.project_path = Some(path);
        self.load_conversation();
        self.enter_editor();
    }

    pub fn new_project(&mut self) -> Result<(), EngineError> {
        self.empty_session()?;
        self.enter_editor();
        self.sync_from_engine();
        Ok(())
    }

    /// Replace the session with an empty, untitled one, and drop what was keyed
    /// to the old one: its path, peaks, selection and recovery copy. A new
    /// project is a new conversation: carrying the last one over would have the
    /// model answering about a project that is no longer open.
    pub(super) fn empty_session(&mut self) -> Result<(), EngineError> {
        self.host.replace_session(
            &aria_plugin::Origin::Editor,
            aria_engine::Engine::new_session,
        )?;
        self.project_path = None;
        self.waveforms.clear();
        self.clear_recovery();
        self.selection.clear();
        self.load_conversation();
        Ok(())
    }

    /// Export the open project, blocking, with the sheet's own choices.
    ///
    /// The sheet used to be ignored here: a blocking export wrote the project's
    /// native output whatever the rows said. It sends the same
    /// [`aria_core::ExportFormat`] the job path sends, and the refusals — an
    /// empty sequence, a codec this machine cannot write — come from the render
    /// path rather than a second copy of the rule in this file.
    pub fn export_to(&mut self, path: PathBuf) -> Result<ApplyOutcome, EngineError> {
        let format = self.export_format();
        self.apply_command(Command::Project(ProjectCommand::Export { path, format }))
    }

    pub fn enter_editor(&mut self) {
        self.route = UiRoute::Editor;
        self.dialog = DialogKind::None;
        self.close_popovers();
    }

    pub fn go_home(&mut self) {
        if self.playing {
            self.toggle_playback();
        }
        self.dialog = DialogKind::None;
        self.close_popovers();
        self.route = UiRoute::Home;
        // The list is about files on disk, and anything could have happened to
        // them while the editor was open.
        self.refresh_recents();
    }

    /// A click on a title: open its drop with nothing lit, or close it. A drop
    /// a button opened is not the title's drop, so the click opens the title's
    /// own in its place.
    pub fn toggle_menu(&mut self, menu: AppMenu) {
        self.open_menu = if self.open_menu == Some(menu) && self.menu_owner.is_none() {
            None
        } else {
            Some(menu)
        };
        self.menu_row = None;
        self.menu_flyout = None;
        self.menu_owner = None;
    }

    pub fn close_menu(&mut self) {
        self.open_menu = None;
        self.menu_row = None;
        self.menu_flyout = None;
        self.menu_owner = None;
    }

    /// Close every popover: the menu bar's drop and flyout, the context menu
    /// and the tool variants. A dialog and a change of screen call this,
    /// because a popover left open under a dialog keeps the keys the dialog
    /// owns: the menu bar answers letters ahead of a focused box.
    pub(crate) fn close_popovers(&mut self) {
        self.close_menu();
        self.close_context();
        self.close_tool_variants();
    }

    pub fn pick_and_open_project(&mut self) {
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Open Project").filter("Aria project", &["aria"])),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                if let Err(err) = state.open_project(path) {
                    state.report_error(&err);
                }
            },
        );
    }

    /// Ask for files and add them: a file an importer reads is read into the
    /// timeline, as one undo step each, and the rest go to the media pool.
    pub fn pick_and_import(&mut self) {
        self.request_pick(Ask::OpenFiles(FileSpec::new("Import")), |state, answer| {
            let paths = state.take_preset_files(answer.all());
            state.import_paths(paths);
        });
    }

    /// Add `paths` as File > Import does: a file an importer reads is read into
    /// the timeline, as one undo step each, and the rest go to the media pool
    /// and the timeline. Every file that arrives heads File > Import Recent.
    pub fn import_paths(&mut self, paths: Vec<PathBuf>) {
        for path in &paths {
            self.load_plugins_for_file(path);
        }
        let (timelines, media): (Vec<PathBuf>, Vec<PathBuf>) = paths
            .into_iter()
            .partition(|path| self.ctx.importer_for(path).is_some());
        let mut arrived = Vec::with_capacity(timelines.len() + media.len());
        for path in timelines {
            if self.import_timeline(&path) {
                arrived.push(path);
            }
        }
        if !media.is_empty() {
            match self.import_dropped_paths(media.clone()) {
                Ok(()) => arrived.extend(media),
                Err(err) => self.report_error(&err),
            }
        }
        self.remember_imports(&arrived);
    }

    /// Relink every missing pool entry from one folder.
    ///
    /// The folder is searched by name and each same-named file is verified
    /// against what the project remembers (see `app::relink`); an entry
    /// binds only when exactly one file agrees. Every entry that stays missing
    /// keeps a reason in [`AppState::relink_notes`], which its pool row shows,
    /// so nothing is repaired or skipped silently.
    pub fn relink_all(&mut self) {
        let missing: Vec<_> = self
            .store
            .media
            .iter()
            .filter(|item| item.missing)
            .cloned()
            .collect();
        if missing.is_empty() {
            self.status_message = Some("No missing media".into());
            return;
        }
        self.request_pick(
            Ask::Folder(FileSpec::new("Relink All")),
            move |state, answer| {
                let Some(folder) = answer.one() else {
                    return;
                };
                let found = super::relink::walk(&folder);
                let plan = super::relink::plan(&missing, &found, super::relink::probe_facts);
                state.relink_notes.clear();
                let mut repaired = 0usize;
                for (id, path) in plan.binds {
                    match state.relink_media(MediaId::from_raw(id), path) {
                        Ok(()) => repaired += 1,
                        Err(err) => {
                            state
                                .relink_notes
                                .insert(id, super::describe_engine_error(&err));
                        }
                    }
                }
                // Each row keeps its own reason; the strip keeps the count,
                // except when there is exactly one — then the count is less
                // use than the name and the reason.
                let mut only: Option<String> = None;
                let refused = plan.refused.len();
                for (id, name, refusal) in plan.refused {
                    let reason = refusal.describe();
                    if refused == 1 {
                        only = Some(format!("{name}: {reason}"));
                    }
                    state.relink_notes.insert(id, reason);
                }
                let left = missing.len() - repaired;
                state.status_message = Some(match (left, only) {
                    (0, _) => format!("Relinked {repaired}"),
                    (1, Some(one)) => format!("Relinked {repaired}; {one}"),
                    _ => format!("Relinked {repaired}, {left} still missing"),
                });
            },
        );
    }

    pub fn add_marker_at_playhead(&mut self) -> Result<(), EngineError> {
        let name = self.next_marker_name();
        let color = self.next_marker_color();
        let at = self.playhead;
        let target_clip = self
            .selected_ui_clip()
            .filter(|clip| {
                clip.timeline_start_ticks <= at.value && at.value < clip.timeline_end_ticks
            })
            .map(|clip| ClipId::from_raw(clip.id));
        if let Some(clip_id) = target_clip {
            self.one_step(|this| {
                let outcome = this.apply_command(Command::Edit(EditCommand::AddMarker {
                    at,
                    name: name.clone(),
                    color: Some(color),
                }))?;
                if let aria_engine::ApplyOutcome::Edited(
                    aria_commands::EditOutcome::CreatedMarker(marker_id),
                ) = outcome
                {
                    let attached = {
                        let mut engine = this.engine.lock();
                        engine.attach_marker_to_clip(marker_id, clip_id)
                    };
                    match attached {
                        Ok(()) => this.sync_from_engine(),
                        Err(err) => {
                            this.report_error(&err);
                            return Err(err);
                        }
                    }
                }
                Ok(())
            })?;
        } else {
            self.apply_command(Command::Edit(EditCommand::AddMarker {
                at,
                name: name.clone(),
                color: Some(color),
            }))?;
        }
        self.status_message = Some(format!("Added {name}"));
        Ok(())
    }

    pub fn set_mark_in(&mut self) {
        self.mark_in = Some(self.playhead.value.max(0));
        if let Some(out) = self.mark_out
            && out < self.playhead.value
        {
            self.mark_out = None;
        }
        self.status_message = Some("Mark In".into());
    }

    pub fn set_mark_out(&mut self) {
        self.mark_out = Some(self.playhead.value.max(0));
        if let Some(inn) = self.mark_in
            && inn > self.playhead.value
        {
            self.mark_in = None;
        }
        self.status_message = Some("Mark Out".into());
    }

    pub fn clear_mark_in(&mut self) {
        self.mark_in = None;
        self.status_message = Some("Cleared In".into());
    }

    pub fn clear_mark_out(&mut self) {
        self.mark_out = None;
        self.status_message = Some("Cleared Out".into());
    }

    pub fn go_to_in(&mut self) {
        if self.source_has_keys() {
            self.source_to_in();
            return;
        }
        if let Some(tick) = self.mark_in {
            self.set_playhead(tick);
        }
    }

    pub fn go_to_out(&mut self) {
        if self.source_has_keys() {
            self.source_to_out();
            return;
        }
        if let Some(tick) = self.mark_out {
            self.set_playhead(tick);
        }
    }

    pub fn trim_selected_to_playhead(&mut self, from_start: bool) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let at = self.playhead.value;
        let start = if from_start {
            at.clamp(
                ui.timeline_start_ticks,
                ui.timeline_end_ticks.saturating_sub(1),
            )
        } else {
            ui.timeline_start_ticks
        };
        let end = if from_start {
            ui.timeline_end_ticks
        } else {
            at.clamp(ui.timeline_start_ticks + 1, ui.timeline_end_ticks)
        };
        if end <= start {
            return Ok(());
        }
        self.trim_clip(ClipId::from_raw(ui.id), start, end)
    }

    pub fn select_clip_linked(&mut self, id: ClipId, with_link: bool) {
        if !with_link {
            self.selection = vec![id];
            self.selected_edge = None;
            self.selected_gap = None;
            self.refresh_projected_selection();
            if let Err(err) = self.refresh_moments() {
                self.report_error(err);
            }
            return;
        }
        let Some(root) = self.ui_clip_ref(id) else {
            self.select_clip(id);
            return;
        };
        // Links are followed only while linked selection is on; a group is
        // an explicit grouping and always selects whole.
        let follow_links = self.linked_selection;
        let root_link = root.linked_to.filter(|_| follow_links);
        if root_link.is_none() && root.grouped_to.is_none() {
            self.select_clip(id);
            return;
        }
        let mut selected = vec![id];
        let mut links: Vec<u64> = root_link.into_iter().collect();
        let mut groups: Vec<u64> = root.grouped_to.into_iter().collect();
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
        self.selection = selected;
        self.selected_edge = None;
        self.selected_gap = None;
        self.refresh_projected_selection();
        if let Err(err) = self.refresh_moments() {
            self.report_error(err);
        }
    }

    pub fn clear_selected_filter(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipFilter {
            clip,
            filter: None,
        }))?;
        Ok(())
    }

    /// Apply the catalog filter `id`. A switch from another filter keeps its
    /// amount, keyframes included, as a new key color keeps a key's strength.
    pub fn set_selected_filter(&mut self, id: impl Into<String>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let mut filter = Filter::new(id);
        if let Some(existing) = self
            .selected_model()
            .and_then(|model| model.filter.as_ref())
        {
            filter.intensity = existing.intensity.clone();
        }
        self.apply_command(Command::Edit(EditCommand::SetClipFilter {
            clip,
            filter: Some(filter),
        }))?;
        Ok(())
    }

    pub fn save_project_as(&mut self) {
        self.request_pick(
            Ask::SaveFile(FileSpec::new("Save As").filter("Aria project", &["aria"])),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                state.project_path = Some(path);
                if let Err(err) = state.save_project() {
                    state.report_error(&err);
                }
            },
        );
    }

    pub fn select_all_unlocked(&mut self) {
        self.selection = self
            .store
            .sequence
            .tracks
            .iter()
            .filter(|track| !track.locked)
            .flat_map(|track| track.clips.iter().map(|clip| ClipId::from_raw(clip.id)))
            .collect();
        self.rebuild_store();
    }

    /// Restore the Timeline's default height in the small-window view.
    pub fn reset_timeline_height(&mut self) {
        self.ui_settings.timeline_height = crate::settings::TIMELINE_DEFAULT_HEIGHT;
    }

    /// Leave the project. Guarded: closing is exactly the mis-click the unsaved
    /// prompt exists for, so there is no unguarded path to Home from a menu, a
    /// chord, or the breadcrumb.
    pub fn close_project(&mut self) {
        self.request_go_home();
    }

    /// Show `rail`'s pane from its top. The panes share one scroll position,
    /// so a long pane left scrolled would open the next long one part way
    /// down.
    pub fn set_home_rail(&mut self, rail: HomeRail) {
        if self.ui_settings.home_rail != rail {
            self.home_scroll.set_offset(gpui::Point::default());
        }
        self.ui_settings.home_rail = rail;
    }

    /// Choose how the pool draws itself. Persisted, because a dense list of
    /// four hundred sources is wanted again on the next launch.
    pub fn set_pool_view(&mut self, view: crate::types::PoolView) {
        self.ui_settings.pool_view = view;
    }

    /// How the pool draws itself now.
    pub fn pool_view(&self) -> crate::types::PoolView {
        self.ui_settings.pool_view
    }

    /// A drop on Home. The plugins decide what each path is; a project opens, and
    /// anything else needs a session to land in, so one is created first. A
    /// plugin package installs into the app and needs no session.
    pub fn drop_on_home(&mut self, paths: Vec<PathBuf>) -> Result<(), EngineError> {
        let paths = self.take_plugin_archives(paths);
        let project = paths
            .iter()
            .find(|path| matches!(self.classify_drop(path), DropAction::OpenProject))
            .cloned();
        if let Some(project) = project {
            return self.open_project(project);
        }
        if paths.is_empty() {
            return Ok(());
        }
        self.new_project()?;
        self.handle_dropped_paths(paths)
    }

    pub fn open_sample_project(&mut self) -> Result<(), EngineError> {
        self.new_project()?;
        let still = crate::app::recents::sample_still(&self.data_dir);
        self.import_media(still)?;
        Ok(())
    }

    pub fn open_dialog(&mut self, kind: DialogKind) {
        if self.route == UiRoute::Home
            && !matches!(
                kind,
                DialogKind::None
                    | DialogKind::Settings
                    | DialogKind::InstallPlugin
                    | DialogKind::About
            )
        {
            return;
        }
        if kind != DialogKind::Palette {
            self.palette_query.clear();
            self.palette_restart();
        }
        if kind != DialogKind::None {
            self.close_popovers();
        }
        // Leaving the unsaved-changes prompt any other way than answering it —
        // Escape, or a dialog opened over it — drops the parked intent, so it
        // cannot fire later against a decision never made.
        if kind != DialogKind::UnsavedChanges {
            self.pending_intent = None;
        }
        // Likewise for the install prompt: a package left unanswered is not
        // installed, and its unpacked files are removed.
        if kind != DialogKind::InstallPlugin {
            self.abandon_plugin_installs();
        }
        if kind != DialogKind::MarkerEdit {
            self.marker_edit.focused = None;
            self.marker_edit.error = None;
        }
        // The timeline file report describes one run, so closing its dialog
        // drops it. An install prompt opened over it returns to it.
        if self.dialog == DialogKind::TimelineReport && kind == DialogKind::None {
            self.timeline_report = None;
        }
        self.dialog = kind;
        // The report's counts are walks of the whole project, so they are taken
        // on request and refreshed on any edit that lands
        // while the report stays open (see `adopt`). Drawing them never asks the
        // session.
        if kind == DialogKind::ProjectReport {
            self.measure_project_report();
        }
        // Likewise the media report: the decoder listing and the selection's
        // source are read when the dialog opens.
        if kind == DialogKind::MediaSupport {
            self.media_support_report();
        }
        // A theme file written or a theme registered while the app runs is
        // listed the next time Settings opens, and so is a key set copied into
        // `<data dir>/keymaps`.
        if kind == DialogKind::Settings {
            self.theme_files = crate::theme::file::scan(&self.data_dir, &self.ctx.themes());
            self.refresh_keymap_sets();
        }
    }
}
