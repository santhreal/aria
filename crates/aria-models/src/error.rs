use thiserror::Error;

use crate::clip::SlotMedia;
use crate::ids::{BinId, ClipId, MarkerId, MediaId, SequenceId, TrackId};
use crate::media::MediaKind;
use crate::schema::ProjectSchema;
use crate::time::{Rational, TimeError};
use crate::track::TrackKind;

/// Errors from model mutations that would violate a referential or layout
/// invariant.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ModelError {
    #[error("unsupported project schema (found {found:?}, expected {expected:?})")]
    UnsupportedProjectSchema {
        found: ProjectSchema,
        expected: ProjectSchema,
    },

    #[error("invalid project file: {0}")]
    InvalidProjectFile(String),

    #[error("unknown track: {0}")]
    UnknownTrack(TrackId),

    #[error("unknown media: {0}")]
    UnknownMedia(MediaId),

    #[error("unknown clip: {0}")]
    UnknownClip(ClipId),

    #[error("clip id {0} already exists on the timeline")]
    DuplicateClip(ClipId),

    #[error("unknown marker: {0}")]
    UnknownMarker(MarkerId),

    #[error("unknown bin: {0}")]
    UnknownBin(BinId),

    #[error("bin {0} cannot be moved inside itself or one of its own bins")]
    BinCycle(BinId),
    #[error("unknown sequence: {0}")]
    UnknownSequence(SequenceId),

    #[error("sequence {0} cannot contain itself, directly or transitively")]
    SequenceCycle(SequenceId),

    #[error("clip overlaps an existing clip on {0}")]
    Overlap(TrackId),

    #[error("track {track} ({kind:?}) cannot hold this clip")]
    IncompatibleTrackKind { track: TrackId, kind: TrackKind },

    #[error("media {0} is still referenced by one or more clips")]
    MediaReferenced(MediaId),

    #[error("source range is outside the media bounds")]
    SourceOutOfBounds,

    #[error("invalid time range (negative or zero duration where positive required)")]
    InvalidRange,

    #[error("invalid transform: {0}")]
    InvalidTransform(String),

    #[error("invalid parameter: {0}")]
    InvalidParam(String),

    #[error("rate mismatch: expected {expected:?}, got {got:?}")]
    RateMismatch { expected: Rational, got: Rational },

    #[error("time arithmetic overflow")]
    TimeOverflow,

    // --- templates --------------------------------------------------------
    #[error("template slot {slot} accepts {accepts:?} but {found:?} media was supplied")]
    SlotMediaMismatch {
        slot: ClipId,
        accepts: SlotMedia,
        found: MediaKind,
    },

    #[error("media supplied for template slot {slot} cannot cover its locked duration")]
    SlotDurationUnmet { slot: ClipId },

    #[error("template slot {slot} is speed-ramped or reversed; filling it is not supported")]
    SlotRetimeUnsupported { slot: ClipId },

    #[error("{given} picks supplied for a template with {slots} fill slots")]
    TooManyPicks { given: usize, slots: usize },

    // --- multicam ---------------------------------------------------------
    #[error("unknown multicam group: {0}")]
    UnknownMulticamGroup(crate::ids::MulticamGroupId),

    #[error("multicam group must have at least one angle")]
    EmptyMulticamGroup,

    #[error("duplicate media {0} in multicam group angles")]
    DuplicateMulticamMedia(MediaId),

    #[error("multicam angle index {index} out of bounds (group has {total} angles)")]
    MulticamAngleOutOfBounds { index: usize, total: usize },

    #[error("multicam switches must be sorted and unique by tick")]
    InvalidMulticamCuts,

    #[error("multicam angle offset is not finite: {0}")]
    InvalidMulticamOffset(String),

    // --- audio routing ----------------------------------------------------
    #[error("track {0} is not a bus, so nothing can be routed into it")]
    NotABus(TrackId),

    #[error("track {0} carries no sound, so it has nothing to route")]
    LaneCarriesNoSound(TrackId),

    #[error("routing track {0} there would feed it back into itself")]
    RoutingCycle(TrackId),

    #[error("track {track} already sends to bus {to}")]
    DuplicateSend { track: TrackId, to: TrackId },

    #[error("track {track} has no send {index}")]
    UnknownSend { track: TrackId, index: usize },

    // --- audio inserts ----------------------------------------------------
    #[error("unknown audio effect: {0}")]
    UnknownAudioEffect(String),

    #[error("clip {0} carries no sound, so it has nothing to process")]
    ClipCarriesNoSound(ClipId),

    #[error("insert {index} is past the end of a chain of {len}")]
    UnknownInsert { index: usize, len: usize },

    #[error("a lane or the master holds at most {0} inserts")]
    InsertsFull(usize),
}

/// Bridge the shared `aria-core` time errors into [`ModelError`] so model
/// methods that propagate rational-time arithmetic with `?` keep returning a
/// single error type.
impl From<TimeError> for ModelError {
    fn from(err: TimeError) -> Self {
        match err {
            TimeError::RateMismatch { expected, got } => ModelError::RateMismatch { expected, got },
            TimeError::Overflow => ModelError::TimeOverflow,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_error_display_formatting() {
        assert_eq!(
            ModelError::InvalidProjectFile("corrupted".into()).to_string(),
            "invalid project file: corrupted"
        );
        assert_eq!(
            ModelError::UnknownTrack(TrackId::from_raw(5)).to_string(),
            "unknown track: TrackId#5"
        );
        assert_eq!(
            ModelError::UnknownClip(ClipId::from_raw(12)).to_string(),
            "unknown clip: ClipId#12"
        );
        assert_eq!(
            ModelError::SourceOutOfBounds.to_string(),
            "source range is outside the media bounds"
        );
        assert_eq!(
            ModelError::InvalidRange.to_string(),
            "invalid time range (negative or zero duration where positive required)"
        );
    }
}
