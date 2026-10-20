//! Saving the current project as a template, and applying one.
//!
//! `SaveTemplate` and `ApplyTemplate` had no operator path, so the whole template
//! feature — replaceable slots, editable text, the `.ariat` bundle — was reachable
//! only from the model. The two halves live together because they share the file
//! filter and the slot vocabulary: a template's slots are what an apply fills, and
//! what a save is only useful for having marked.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};

use aria_commands::{Command, ProjectCommand, TemplatePick};
use aria_engine::EngineError;
use aria_models::{Template, TemplateMeta};

use super::AppState;
use super::picker::{Ask, FileSpec};

/// The bundle extension, shared by the save and open dialogs.
pub const TEMPLATE_EXTENSION: &str = "ariat";

impl AppState {
    /// How many visual fill slots the current project carries. This mirrors
    /// `Template::slots`, which excludes an audio-only slot because that one is
    /// swapped separately: the shown count must be the count an apply will
    /// prompt to fill.
    pub fn template_slot_count(&self) -> usize {
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .filter(|clip| {
                clip.replaceable
                    .as_ref()
                    .is_some_and(|slot| slot.accepts != aria_models::SlotMedia::AudioOnly)
            })
            .count()
    }

    /// Write the current project to a `.ariat` bundle. The project itself is
    /// untouched: no dirty-flag change and no project-path change, so saving a
    /// template is not a Save As in disguise.
    pub fn save_template(&mut self, path: PathBuf, name: String) -> Result<(), EngineError> {
        let name = if name.trim().is_empty() {
            self.store.project_name.to_string()
        } else {
            name
        };
        let slots = self.template_slot_count();
        self.apply_command(Command::Project(ProjectCommand::SaveTemplate {
            path: path.clone(),
            meta: TemplateMeta::new(name),
        }))?;
        self.status_message = Some(format!(
            "Saved template to {} ({slots} slot{})",
            path.display(),
            if slots == 1 { "" } else { "s" }
        ));
        Ok(())
    }

    /// Ask for a destination and save the template there. The name comes from the
    /// project, which already has a chosen name.
    pub fn pick_and_save_template(&mut self) {
        let name = self.store.project_name.to_string();
        self.request_pick(
            Ask::SaveFile(
                FileSpec::new("Save as Template")
                    .filter("Aria template", &[TEMPLATE_EXTENSION])
                    .file_name(format!("{name}.{TEMPLATE_EXTENSION}")),
            ),
            move |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                if let Err(err) = state.save_template(path, name) {
                    state.report_error(&err);
                }
            },
        );
    }

    /// The slots a bundle on disk exposes, in fill order, as `(label, accepts)`
    /// pairs. Read before the picks are collected so the prompt requests the media
    /// a template accepts, rather than opening a blind file
    /// dialog per slot.
    pub fn template_slots(&self, path: &Path) -> Result<Vec<String>, EngineError> {
        let template = Template::load_from_file(path)?;
        Ok(template
            .slots()
            .iter()
            .enumerate()
            .map(|(index, clip)| {
                let slot = clip.replaceable.as_ref();
                match slot.and_then(|s| s.label.clone()) {
                    Some(label) => label,
                    None => format!("Slot {}", index + 1),
                }
            })
            .collect())
    }

    /// Replace the session with a template filled by `picks`. Fewer picks than
    /// slots keeps the template's own sample media in the unfilled slots, which is
    /// how a template previews before it is filled.
    pub fn apply_template(
        &mut self,
        path: PathBuf,
        picks: Vec<TemplatePick>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Project(ProjectCommand::ApplyTemplate {
            path,
            picks,
        }))?;
        self.project_path = None;
        self.selection.clear();
        self.playhead = aria_models::RationalTime::new(0, self.playhead.rate);
        self.route = crate::types::UiRoute::Editor;
        self.rebuild_store();
        Ok(())
    }

    /// Ask for one file per slot, then apply. Cancelling the slot picker stops asking
    /// and applies what was chosen so far, so a template can be opened unfilled.
    ///
    /// Separate from the bundle picker because a cloud template arrives with its path
    /// already known and must still be filled the same way.
    pub fn fill_and_apply_template(&mut self, path: PathBuf) {
        let labels = match self.template_slots(&path) {
            Ok(labels) => labels,
            Err(err) => {
                self.report_error(&err);
                return;
            }
        };
        self.fill_next_slot(path, labels.into(), Vec::new());
    }

    /// Ask for one slot's media, then ask for the next. Cancelling stops asking
    /// and applies what was chosen so far.
    ///
    /// One question at a time rather than one dialog per slot at once: a dialog
    /// is answered by a person, and the answer to the next slot is only worth
    /// asking for once this one has been given.
    fn fill_next_slot(
        &mut self,
        path: PathBuf,
        mut labels: VecDeque<String>,
        picks: Vec<TemplatePick>,
    ) {
        let Some(label) = labels.pop_front() else {
            if let Err(err) = self.apply_template(path, picks) {
                self.report_error(&err);
            }
            return;
        };
        self.request_pick(
            Ask::OpenFile(FileSpec::new(format!("Media for {label}")).filter(
                "Media",
                &crate::app::media::import::media_import_extensions(),
            )),
            move |state, answer| {
                let mut picks = picks;
                let Some(file) = answer.one() else {
                    if let Err(err) = state.apply_template(path, picks) {
                        state.report_error(&err);
                    }
                    return;
                };
                picks.push(TemplatePick {
                    path: file,
                    source_in: None,
                });
                state.fill_next_slot(path, labels, picks);
            },
        );
    }

    /// Pick a bundle, then fill and apply it.
    pub fn pick_and_apply_template(&mut self) {
        self.request_pick(
            Ask::OpenFile(
                FileSpec::new("Use template").filter("Aria template", &[TEMPLATE_EXTENSION]),
            ),
            |state, answer| {
                let Some(path) = answer.one() else {
                    return;
                };
                state.fill_and_apply_template(path);
            },
        );
    }
}
