//! Project open/save, Home, dialogs, and file picks.

use std::path::PathBuf;

use aria_commands::{Command, EditCommand, ProjectCommand};
use aria_engine::{ApplyOutcome, EngineError};
use aria_models::{ClipId, Filter, MediaId, Param};
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
        {
            let mut engine = self.engine.lock();
            engine.new_session();
        }
        self.project_path = None;
        self.waveforms.clear();
        self.clear_recovery();
        self.selection.clear();
        // A new project is a new conversation: carrying the last one over would
        // have the model answering about a project that is no longer open.
        self.load_conversation();
        self.enter_editor();
        self.sync_from_engine();
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
        self.context_menu = None;
        self.open_menu = None;
    }

    pub fn go_home(&mut self) {
        if self.playing {
            self.toggle_playback();
        }
        self.dialog = DialogKind::None;
        self.context_menu = None;
        self.open_menu = None;
        self.route = UiRoute::Home;
        // The list is about files on disk, and anything could have happened to
        // them while the editor was open.
        self.refresh_recents();
    }

    pub fn toggle_menu(&mut self, menu: AppMenu) {
        self.open_menu = if self.open_menu == Some(menu) {
            None
        } else {
            Some(menu)
        };
    }

    pub fn close_menu(&mut self) {
        self.open_menu = None;
    }

    pub fn pick_and_open_project(&mut self) {
        self.request_pick(
            Ask::OpenFile(FileSpec::new("Open project").filter("Aria project", &["aria"])),
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

    pub fn pick_and_import(&mut self) {
        self.request_pick(
            Ask::OpenFiles(FileSpec::new("Import media")),
            |state, answer| {
                let paths = answer.all();
                if paths.is_empty() {
                    return;
                }
                if let Err(err) = state.import_dropped_paths(paths) {
                    state.report_error(&err);
                }
            },
        );
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
                    (1, Some(one)) => format!("Relinked {repaired} — {one}"),
                    _ => format!("Relinked {repaired}, {left} still missing"),
                });
            },
        );
    }

    pub fn add_marker_at_playhead(&mut self) -> Result<(), EngineError> {
        let name = self.next_marker_name();
        let color = self.next_marker_color();
        self.apply_command(Command::Edit(EditCommand::AddMarker {
            at: self.playhead,
            name: name.clone(),
            color: Some(color),
        }))?;
        self.status_message = Some(format!("Added {name}"));
        Ok(())
    }

    pub fn set_mark_in(&mut self) {
        self.mark_in = Some(self.playhead.value.max(0));
        if let Some(out) = self.mark_out {
            if out < self.playhead.value {
                self.mark_out = None;
            }
        }
        self.status_message = Some("Mark In".into());
    }

    pub fn set_mark_out(&mut self) {
        self.mark_out = Some(self.playhead.value.max(0));
        if let Some(inn) = self.mark_in {
            if inn > self.playhead.value {
                self.mark_in = None;
            }
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
        if let Some(tick) = self.mark_in {
            self.set_playhead(tick);
        }
    }

    pub fn go_to_out(&mut self) {
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

    pub fn delete_selected(&mut self) -> Result<(), EngineError> {
        match self.edit_mode {
            crate::types::EditMode::Ripple => self.ripple_delete_selected(),
            crate::types::EditMode::Overwrite => {
                let Some(clip) = self.selected_clip() else {
                    return Ok(());
                };
                self.apply_command(Command::Edit(EditCommand::RemoveClip { clip }))?;
                self.clear_selection();
                Ok(())
            }
        }
    }

    pub fn select_clip_linked(&mut self, id: ClipId, with_link: bool) {
        if !with_link {
            self.select_clip(id);
            return;
        }
        let Some(link) = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.id == id.raw())
            .and_then(|clip| clip.linked_to)
        else {
            self.select_clip(id);
            return;
        };
        self.selection = self
            .store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| clip.id == id.raw() || clip.linked_to == Some(link))
            .map(|clip| ClipId::from_raw(clip.id))
            .collect();
        if !self.selection.contains(&id) {
            self.selection.insert(0, id);
        }
        self.rebuild_store();
    }

    pub fn set_selected_filter_amount(&mut self, amount: f32) -> Result<(), EngineError> {
        let Some(ui) = self.selected_ui_clip() else {
            return Ok(());
        };
        let Some(filter) = ui.filter else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipFilter {
            clip: ClipId::from_raw(ui.id),
            filter: Some(Filter {
                id: filter.id.to_string(),
                intensity: Param::Constant(amount.clamp(0.0, 1.0)),
            }),
        }))?;
        Ok(())
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

    pub fn set_selected_filter(&mut self, id: impl Into<String>) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        self.apply_command(Command::Edit(EditCommand::SetClipFilter {
            clip,
            filter: Some(Filter::new(id)),
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

    pub fn reset_split(&mut self, which: &str) {
        match which {
            "sidebar" => self.ui_settings.sidebar_width = crate::settings::SIDEBAR_DEFAULT,
            "inspector" => self.ui_settings.inspector_width = crate::settings::INSPECTOR_DEFAULT,
            "timeline" => {
                self.ui_settings.timeline_height = crate::settings::TIMELINE_DEFAULT_HEIGHT
            }
            _ => {}
        }
    }

    /// Leave the project. Guarded: closing is exactly the mis-click the unsaved
    /// prompt exists for, so there is no unguarded path to Home from a menu, a
    /// chord, or the breadcrumb.
    pub fn close_project(&mut self) {
        self.request_go_home();
    }

    pub fn set_home_rail(&mut self, rail: HomeRail) {
        self.ui_settings.home_rail = rail;
    }

    /// Choose how the pool draws itself. Persisted, because the operator who
    /// wants a dense list of four hundred sources wants it on the next launch
    /// too.
    pub fn set_pool_view(&mut self, view: crate::types::PoolView) {
        self.ui_settings.pool_view = view;
    }

    /// How the pool draws itself now.
    pub fn pool_view(&self) -> crate::types::PoolView {
        self.ui_settings.pool_view
    }

    /// A drop on Home. The plugins decide what each path is; a project opens, and
    /// anything else needs a session to land in, so one is created first.
    pub fn drop_on_home(&mut self, paths: Vec<PathBuf>) -> Result<(), EngineError> {
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
        if self.route == UiRoute::Home && !matches!(kind, DialogKind::None | DialogKind::Settings) {
            return;
        }
        if kind != DialogKind::Palette {
            self.palette_query.clear();
            self.palette_index = 0;
        }
        // Leaving the unsaved-changes prompt any other way than answering it —
        // Escape, or a dialog opened over it — drops the parked intent, so it
        // cannot fire later against a decision the operator never made.
        if kind != DialogKind::UnsavedChanges {
            self.pending_intent = None;
        }
        self.dialog = kind;
        // The report's counts are walks of the whole project, so they are taken
        // when the operator asks for them and refreshed on any edit that lands
        // while the report stays open (see `adopt`). Drawing them never asks the
        // session.
        if kind == DialogKind::ProjectReport {
            self.measure_project_report();
        }
    }
}
