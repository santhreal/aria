//! Secondary sequences stored within a [`Project`](crate::Project).
//!
//! A sequence is an independent timeline with its own tracks, clips, frame rate,
//! and canvas settings. Placing a sequence as a clip on another timeline embeds
//! its rendering as a single composite unit (nested sequence / pre-comp).

use serde::{Deserialize, Serialize};

use crate::ids::SequenceId;
use crate::time::Rational;
use crate::timeline::Timeline;

/// A secondary sequence in a [`Project`](crate::Project)'s sequence store.
// A timeline is deliberately not comparable (its clip index and track order are
// derived state), so neither is a sequence: identity is the id.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sequence {
    pub id: SequenceId,
    pub name: String,
    pub timeline: Timeline,
}

impl Sequence {
    /// Create a sequence with a fresh id and an empty timeline running at `frame_rate`.
    pub fn new(name: impl Into<String>, frame_rate: Rational) -> Self {
        Self {
            id: SequenceId::next(),
            name: name.into(),
            timeline: Timeline::new(frame_rate),
        }
    }

    /// Create a sequence with an explicit id and timeline.
    pub fn from_parts(id: SequenceId, name: impl Into<String>, timeline: Timeline) -> Self {
        Self {
            id,
            name: name.into(),
            timeline,
        }
    }

    /// Create a sequence with a fresh id wrapping an existing timeline.
    pub fn with_timeline(name: impl Into<String>, timeline: Timeline) -> Self {
        Self {
            id: SequenceId::next(),
            name: name.into(),
            timeline,
        }
    }
}
