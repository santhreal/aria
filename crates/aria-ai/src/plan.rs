//! What a rehearsed plan's ids become when the plan is replayed.
//!
//! A dry run applies each step so the next one is validated against the plan so
//! far, then rolls the whole rehearsal back. Everything it created goes with it,
//! so replaying the plan creates those entities again and the engine hands out
//! fresh ids. A step recorded against a rehearsal id would name nothing then, or
//! name whatever holds that id now. So the replay notes what it created in place
//! of what the rehearsal created, and rewrites the steps that follow.

use std::collections::HashMap;

use aria_commands::EditOutcome;

use crate::WireCommand;

/// The entity one outcome created, in the terms the id maps are keyed by.
///
/// Exhaustive over [`EditOutcome`] on purpose: an outcome that creates a new
/// kind of entity does not compile until it says which map it belongs in, so a
/// new creating outcome cannot quietly replay against an id nobody rewrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Made {
    Clip(u64),
    Track(u64),
    Marker(u64),
    /// The outcome changed something that already existed, or created an entity
    /// no wire command can name. `WireCommand` carries no bin and no sequence,
    /// so a plan cannot hold a reference to either.
    Nothing,
}

impl Made {
    /// Read what an outcome created.
    pub fn of(outcome: &EditOutcome) -> Self {
        match outcome {
            EditOutcome::Created(clip) => Self::Clip(clip.raw()),
            EditOutcome::CreatedTrack(track) => Self::Track(track.raw()),
            EditOutcome::CreatedMarker(marker) => Self::Marker(marker.raw()),
            EditOutcome::Updated(_)
            | EditOutcome::Removed(_)
            | EditOutcome::RemovedTrack(_)
            | EditOutcome::ShiftedTrack(_)
            | EditOutcome::UpdatedTrack(_)
            | EditOutcome::UpdatedMarker(_)
            | EditOutcome::RemovedMarker(_)
            | EditOutcome::UpdatedCanvas
            | EditOutcome::UpdatedSequenceRate
            | EditOutcome::UpdatedAudioLayout
            | EditOutcome::UpdatedTimeDisplay
            | EditOutcome::UpdatedMaster
            | EditOutcome::UpdatedProject
            | EditOutcome::CreatedBin(_)
            | EditOutcome::UpdatedPool
            | EditOutcome::OpenedSequence
            | EditOutcome::UpdatedSourcePatch => Self::Nothing,
        }
    }
}

/// Rehearsal ids mapped onto the ids a replay created in their place.
#[derive(Debug, Default)]
pub struct Ids {
    clips: HashMap<u64, u64>,
    tracks: HashMap<u64, u64>,
    markers: HashMap<u64, u64>,
}

impl Ids {
    /// Rewrite one step's references before it is validated.
    pub fn rewrite(&self, command: &mut WireCommand) {
        command.remap_ids(&self.clips, &self.tracks, &self.markers);
    }

    /// Note what the replay created where the rehearsal created something.
    ///
    /// A step whose rehearsal produced nothing records nothing, and so does one
    /// whose replay produced a different kind of entity: only an id standing in
    /// for an id of the same kind is a substitution.
    pub fn note(&mut self, rehearsed: Option<EditOutcome>, replayed: EditOutcome) {
        let Some(rehearsed) = rehearsed else {
            return;
        };
        match (Made::of(&rehearsed), Made::of(&replayed)) {
            (Made::Clip(was), Made::Clip(now)) => {
                self.clips.insert(was, now);
            }
            (Made::Track(was), Made::Track(now)) => {
                self.tracks.insert(was, now);
            }
            (Made::Marker(was), Made::Marker(now)) => {
                self.markers.insert(was, now);
            }
            (Made::Clip(_) | Made::Track(_) | Made::Marker(_) | Made::Nothing, _) => {}
        }
    }

    /// Whether anything the replay created stands in for a rehearsal id.
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty() && self.tracks.is_empty() && self.markers.is_empty()
    }
}

#[cfg(test)]
mod tests;
