//! Nested sequences: build one from a selection, open it, leave it, place it
//! again.
//!
//! The engine has carried `CreateNestedSequence`, `PlaceNestedSequence` and
//! `OpenSequence` for as long as it has had sequences, and all three declare
//! `Reach::Operator`, a command a UI surface is supposed to send.
//! Nothing sent them: the three names appeared nowhere in this crate, so a
//! nest could be created by the assistant or the CLI and then never opened
//! from the editor. These are the verbs the palette rows and the Sequence menu
//! call, and they are on `AppState` rather than in the rows so a test can drive
//! them headless.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ModelError, RationalTime, SequenceId, TimeRange};

use super::AppState;
use super::decline::Decline;

impl AppState {
    /// Nest the selected clips into a sequence of their own.
    ///
    /// The selection is replaced by one clip that plays the new sequence, on
    /// the lane the first selected clip came from. With nothing selected it
    /// declines on the status line and sends nothing.
    pub fn nest_selection(&mut self, name: impl Into<String>) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            self.decline(Decline::NoSelection);
            return Ok(());
        }
        let clips = self.selection.to_vec();
        self.apply_command(Command::Edit(EditCommand::CreateNestedSequence {
            name: name.into(),
            clips,
        }))?;
        Ok(())
    }

    /// Nest the selection under a generated name.
    ///
    /// The name counts the sequences that already exist rather than the nests
    /// on this timeline, so two nests never share a name in one project.
    pub fn nest_selection_named_in_order(&mut self) -> Result<(), EngineError> {
        let name = format!("Nest {}", self.store.sequences.len() + 1);
        self.nest_selection(name)
    }

    /// Open a sequence for editing, or the root timeline with `None`.
    ///
    /// The selection is dropped first: it names clips of the timeline being
    /// left, and every edit that follows lands on the one being opened.
    pub fn open_sequence(&mut self, sequence: Option<SequenceId>) -> Result<(), EngineError> {
        self.selection.clear();
        self.apply_command(Command::Edit(EditCommand::OpenSequence { sequence }))?;
        // A nest has its own length, and the playhead was standing in the
        // timeline that was just left. Clamp it rather than keep a tick that
        // may be past the end of what is now open.
        let tick = self.playhead.value;
        self.set_playhead(tick);
        Ok(())
    }

    /// New Sequence (Ctrl+N): an empty sequence with one video and one audio
    /// lane, at the open timeline's rate and canvas, opened for editing.
    ///
    /// Named after the count of sequences the project holds, so two new
    /// sequences never share a name.
    pub fn new_sequence(&mut self) -> Result<(), EngineError> {
        let name = format!("Sequence {}", self.store.sequences.len() + 1);
        self.selection.clear();
        self.apply_command(Command::Edit(EditCommand::CreateSequence { name }))?;
        self.set_playhead(0);
        Ok(())
    }

    /// Open the nest the selection points at.
    ///
    /// Declines when the selection is not a nest: the alternative is opening
    /// whichever sequence happens to be first, which is not what was clicked.
    pub fn open_selected_nest(&mut self) -> Result<(), EngineError> {
        match self.selected_nest_or_decline() {
            Some(sequence) => self.open_sequence(Some(sequence)),
            None => Ok(()),
        }
    }

    /// Leave the open sequence for the root timeline. Declines on the root
    /// timeline itself.
    pub fn leave_sequence(&mut self) -> Result<(), EngineError> {
        if self.store.open_sequence.is_none() {
            self.decline(Decline::AtRootSequence);
            return Ok(());
        }
        self.open_sequence(None)
    }

    /// Place `sequence` on the drop-target lane at the playhead.
    ///
    /// The whole nest goes in: its own duration is the source range, which is
    /// what makes a second copy of a nest a copy rather than a trim of one.
    pub fn place_nested_sequence(&mut self, sequence: SequenceId) -> Result<(), EngineError> {
        let rate = self.store.sequence.frame_rate;
        let duration = self
            .store
            .sequences
            .iter()
            .find(|row| row.id == sequence.raw())
            .map(|row| row.duration_ticks)
            .ok_or(EngineError::Model(ModelError::UnknownSequence(sequence)))?;
        if duration <= 0 {
            return Err(EngineError::Model(ModelError::InvalidRange));
        }
        // A nest holds a picture, so the lane that takes it is the one a
        // media drop would land on.
        self.one_step_named(
            |label: &&'static str| Some(*label),
            |state| {
                let track = state.drop_target_track(aria_models::TrackKind::Video, None)?;
                let edit = EditCommand::PlaceNestedSequence {
                    track,
                    sequence,
                    source: TimeRange::at_rate(0, duration, rate),
                    start: RationalTime::new(state.playhead.value, rate),
                };
                let label = edit.label();
                state.apply_command(Command::Edit(edit))?;
                Ok(label)
            },
        )?;
        Ok(())
    }

    /// Place the selected nest again at the playhead. Declines when the
    /// selection is not a nest.
    pub fn place_selected_nest(&mut self) -> Result<(), EngineError> {
        match self.selected_nest_or_decline() {
            Some(sequence) => self.place_nested_sequence(sequence),
            None => Ok(()),
        }
    }

    /// The nest the selection plays, or nothing plus the status line that
    /// states why: no clip is selected, or the selected clip plays no nest.
    fn selected_nest_or_decline(&mut self) -> Option<SequenceId> {
        if self.selection.is_empty() {
            self.decline(Decline::NoSelection);
            return None;
        }
        let nest = self.selected_nested_sequence();
        if nest.is_none() {
            self.decline(Decline::NotANest);
        }
        nest
    }

    /// The sequence the selection plays, when a single nest is selected.
    pub fn selected_nested_sequence(&self) -> Option<SequenceId> {
        let selected = self.selection.first()?;
        let raw = selected.raw();
        self.store
            .sequence
            .tracks
            .iter()
            .flat_map(|track| track.clips.iter())
            .find(|clip| clip.id == raw)
            .and_then(|clip| clip.nested_sequence)
            .map(SequenceId::from_raw)
    }

    /// The name of the open sequence, or `None` on the root timeline.
    ///
    /// What a breadcrumb draws: the store carries the id, and an id is not a
    /// readable name.
    pub fn open_sequence_name(&self) -> Option<&str> {
        let open = self.store.open_sequence?;
        self.store
            .sequences
            .iter()
            .find(|row| row.id == open)
            .map(|row| row.name.as_str())
    }
}

#[cfg(test)]
#[path = "nested_tests.rs"]
mod tests;
