//! Nested sequences: build one from a selection, open it, leave it, place it
//! again.
//!
//! The engine has carried `CreateNestedSequence`, `PlaceNestedSequence` and
//! `OpenSequence` for as long as it has had sequences, and all three declare
//! `Reach::Operator` — a command the operator is supposed to be able to send.
//! Nothing sent them: the three names appeared nowhere in this crate, so a
//! nest could be created by the assistant or the CLI and then never opened
//! from the editor. These are the verbs the palette rows and the Sequence menu
//! call, and they are on `AppState` rather than in the rows so a test can drive
//! them headless.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ModelError, RationalTime, SequenceId, TimeRange};

use super::AppState;

impl AppState {
    /// Nest the selected clips into a sequence of their own.
    ///
    /// The selection is replaced by one clip that plays the new sequence, on
    /// the lane the first selected clip came from. Refused with the engine's
    /// own error when nothing is selected, so the message an operator reads is
    /// the message the command layer would have produced anyway.
    pub fn nest_selection(&mut self, name: impl Into<String>) -> Result<(), EngineError> {
        if self.selection.is_empty() {
            return Err(EngineError::Model(ModelError::InvalidParam(
                "select the clips to nest first".into(),
            )));
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

    /// Open the nest the selection points at.
    ///
    /// Refused when the selection is not a nest: the alternative is opening
    /// whichever sequence happens to be first, which is not what was clicked.
    pub fn open_selected_nest(&mut self) -> Result<(), EngineError> {
        let sequence = self.selected_nested_sequence().ok_or_else(|| {
            EngineError::Model(ModelError::InvalidParam(
                "select a nested sequence clip to open it".into(),
            ))
        })?;
        self.open_sequence(Some(sequence))
    }

    /// Leave the open sequence for the root timeline.
    pub fn leave_sequence(&mut self) -> Result<(), EngineError> {
        if self.store.open_sequence.is_none() {
            return Err(EngineError::Model(ModelError::InvalidParam(
                "the root timeline is already open".into(),
            )));
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
        // A nest carries a picture, so the lane that takes it is the one a
        // media drop would land on.
        let track = self.drop_target_track(aria_models::TrackKind::Video, None)?;
        self.apply_command(Command::Edit(EditCommand::PlaceNestedSequence {
            track,
            sequence,
            source: TimeRange::at_rate(0, duration, rate),
            start: RationalTime::new(self.playhead.value, rate),
        }))?;
        Ok(())
    }

    /// Place the selected nest again at the playhead.
    pub fn place_selected_nest(&mut self) -> Result<(), EngineError> {
        let sequence = self.selected_nested_sequence().ok_or_else(|| {
            EngineError::Model(ModelError::InvalidParam(
                "select a nested sequence clip to place another copy of it".into(),
            ))
        })?;
        self.place_nested_sequence(sequence)
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
    /// name an operator recognises.
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
