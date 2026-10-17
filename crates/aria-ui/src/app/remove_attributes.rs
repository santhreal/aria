//! Edit > Remove Attributes: the dialog's form and the edit it commits.

use aria_commands::{Command, EditCommand};
use aria_models::{ClipAttribute, ClipId};

use super::{AppState, DialogKind};

/// What the Remove Attributes dialog offers: each group a selected clip holds,
/// in [`ClipAttribute::ALL`] order, and whether its box is ticked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RemoveAttributesForm {
    pub rows: Vec<(ClipAttribute, bool)>,
    /// The groups each selected clip holds, read when the dialog opened.
    pub held: Vec<(ClipId, Vec<ClipAttribute>)>,
}

impl AppState {
    /// Open Remove Attributes for the selection, offering every group some
    /// selected clip holds, each ticked.
    ///
    /// Reports on the status line instead when nothing is selected or the
    /// selected clips hold nothing a new clip does not.
    pub fn open_remove_attributes(&mut self) {
        if self.selection.is_empty() {
            self.status_message = Some("Select a clip to remove attributes from.".into());
            return;
        }
        let held: Vec<(ClipId, Vec<ClipAttribute>)> = {
            let engine = self.engine.lock();
            let project = engine.project();
            self.selection
                .iter()
                .filter_map(|&id| {
                    let clip = project.clip(id)?;
                    let groups: Vec<ClipAttribute> = ClipAttribute::ALL
                        .into_iter()
                        .filter(|attribute| attribute.is_set(clip))
                        .collect();
                    (!groups.is_empty()).then_some((id, groups))
                })
                .collect()
        };
        let rows: Vec<(ClipAttribute, bool)> = ClipAttribute::ALL
            .into_iter()
            .filter(|attribute| held.iter().any(|(_, groups)| groups.contains(attribute)))
            .map(|attribute| (attribute, true))
            .collect();
        if rows.is_empty() {
            self.status_message = Some("The selected clips hold no attributes to remove.".into());
            return;
        }
        self.remove_attributes = RemoveAttributesForm { rows, held };
        self.open_dialog(DialogKind::RemoveAttributes);
    }

    /// Tick or untick one group of the open dialog.
    pub fn toggle_remove_attribute(&mut self, attribute: ClipAttribute) {
        if let Some(row) = self
            .remove_attributes
            .rows
            .iter_mut()
            .find(|(each, _)| *each == attribute)
        {
            row.1 = !row.1;
        }
    }

    /// Remove the ticked groups from every selected clip that holds them, as
    /// one undo step, and close the dialog. A refusal keeps the dialog open
    /// and reports the reason.
    pub fn apply_remove_attributes(&mut self) {
        let chosen: Vec<ClipAttribute> = self
            .remove_attributes
            .rows
            .iter()
            .filter_map(|&(attribute, ticked)| ticked.then_some(attribute))
            .collect();
        if chosen.is_empty() {
            self.status_message = Some("Tick an attribute to remove.".into());
            return;
        }
        let plan: Vec<(ClipId, Vec<ClipAttribute>)> = self
            .remove_attributes
            .held
            .iter()
            .map(|(clip, groups)| {
                let groups: Vec<ClipAttribute> = groups
                    .iter()
                    .copied()
                    .filter(|attribute| chosen.contains(attribute))
                    .collect();
                (*clip, groups)
            })
            .filter(|(_, groups)| !groups.is_empty())
            .collect();
        let clips = plan.len();
        let removed = self.one_step(|this| {
            for (clip, attributes) in plan {
                this.apply_command(Command::Edit(EditCommand::RemoveAttributes {
                    clip,
                    attributes,
                }))?;
            }
            Ok(())
        });
        match removed {
            Ok(()) => {
                self.open_dialog(DialogKind::None);
                self.status_message = Some(format!("Removed attributes from {clips} clip(s)"));
            }
            Err(err) => self.report_error(&err),
        }
    }
}

#[cfg(test)]
#[path = "remove_attributes_tests.rs"]
mod tests;
