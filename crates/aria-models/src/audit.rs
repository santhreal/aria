//! What an audit can look for, as data.
//!
//! The model answers a couple of dozen "does this clip carry X?" questions —
//! a custom crop, a keyframed volume envelope, detected beats, sound that a
//! separated companion took over. Every one of them was a method with no
//! caller outside the crate that defines it: a speed ramp could go on a clip
//! in one shot and then could not be found again three hundred clips later.
//!
//! A row here states one question, so a surface that finds those clips
//! walks [`clip_facts`] instead of writing a predicate per row.
//! `index_of` is an exhaustive match over [`ClipFact`], so a question added
//! without a row does not compile.
//!
//! Project-wide counts live in the sibling [`project_facts`] table for the same
//! reason: `media_count`, `bin_count`, `marker_count` and `has_transitions` are
//! the numbers to check before an export, and none of them had a
//! surface either.

use crate::clip::Clip;
use crate::ids::ClipId;
use crate::timeline::Timeline;

#[path = "audit_project.rs"]
mod project_facts;

pub use project_facts::{ProjectFact, ProjectFactSpec, project_facts};

/// One thing an audit can look for on a single clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClipFact {
    /// Framing differs from the full frame.
    CustomCrop,
    /// Level, balance, or a fade differs from the default mix.
    CustomAudio,
    /// A keyframed volume envelope rather than a flat gain.
    VolumeEnvelope,
    /// A keyframed pan envelope rather than a fixed balance.
    PanEnvelope,
    /// A non-flat playback-rate ramp.
    SpeedCurve,
    /// Detected beat markers.
    Beats,
    /// Pitch rides the playback speed instead of being preserved.
    PitchRidesSpeed,
    /// The clip's own media audio plays on its own lane.
    OwnAudio,
    /// A linked audio-lane companion took the sound over.
    DetachedAudio,
    /// Another clip starts exactly where this one ends: a junction a
    /// transition can take.
    AtACut,
}

/// What one clip question is called, and how it is answered.
///
/// The timeline is passed alongside the clip because some questions are about
/// the clip's place in the sequence (its lane, its neighbours) rather than
/// about the clip alone.
#[derive(Debug, Clone, Copy)]
pub struct ClipFactSpec {
    /// The question this row describes.
    pub fact: ClipFact,
    /// Stable wire / UI id.
    pub id: &'static str,
    /// Operator-facing label, phrased as what the clip carries.
    pub label: &'static str,
    /// Answer it for one clip.
    pub test: fn(&Timeline, &Clip) -> bool,
}

/// Whether any other clip on the same lane starts exactly where `clip` ends.
///
/// The lane owns the abutment rule ([`crate::Track::clips_abut`]); this only
/// asks it about every neighbour, because "is there a cut here" is what a
/// transition needs and a clip alone cannot say.
fn at_a_cut(timeline: &Timeline, clip: &Clip) -> bool {
    let Some(track) = timeline.track_of(clip.id).and_then(|id| timeline.track(id)) else {
        return false;
    };
    track
        .clips_ordered()
        .iter()
        .any(|other| other.id != clip.id && track.clips_abut(clip.id, other.id))
}

/// The shipped questions, in the order a surface offers them.
const SHIPPED: &[ClipFactSpec] = &[
    ClipFactSpec {
        fact: ClipFact::CustomCrop,
        id: "custom_crop",
        label: "Cropped or mirrored",
        test: |_, clip| clip.has_custom_crop(),
    },
    ClipFactSpec {
        fact: ClipFact::CustomAudio,
        id: "custom_audio",
        label: "Audio mix changed",
        test: |_, clip| clip.has_custom_audio(),
    },
    ClipFactSpec {
        fact: ClipFact::VolumeEnvelope,
        id: "volume_envelope",
        label: "Volume envelope",
        test: |_, clip| clip.has_volume_envelope(),
    },
    ClipFactSpec {
        fact: ClipFact::PanEnvelope,
        id: "pan_envelope",
        label: "Pan envelope",
        test: |_, clip| clip.has_pan_envelope(),
    },
    ClipFactSpec {
        fact: ClipFact::SpeedCurve,
        id: "speed_curve",
        label: "Speed ramp",
        test: |_, clip| clip.has_speed_curve(),
    },
    ClipFactSpec {
        fact: ClipFact::Beats,
        id: "beats",
        label: "Detected beats",
        test: |_, clip| clip.has_beats(),
    },
    ClipFactSpec {
        fact: ClipFact::PitchRidesSpeed,
        id: "pitch_rides_speed",
        label: "Pitch rides speed",
        test: |_, clip| clip.audio_pitch_factor() != 1.0,
    },
    ClipFactSpec {
        fact: ClipFact::OwnAudio,
        id: "own_audio",
        label: "Sounds on its own lane",
        test: |timeline, clip| timeline.carries_own_audio(clip.id),
    },
    ClipFactSpec {
        fact: ClipFact::DetachedAudio,
        id: "detached_audio",
        label: "Audio separated to a lane",
        test: |timeline, clip| timeline.detached_to_audio_lane(clip.id),
    },
    ClipFactSpec {
        fact: ClipFact::AtACut,
        id: "at_a_cut",
        label: "Abuts the next clip",
        test: at_a_cut,
    },
];

/// Which row describes `fact`.
///
/// Exhaustive on purpose: a question added without a row does not compile.
const fn index_of(fact: ClipFact) -> usize {
    match fact {
        ClipFact::CustomCrop => 0,
        ClipFact::CustomAudio => 1,
        ClipFact::VolumeEnvelope => 2,
        ClipFact::PanEnvelope => 3,
        ClipFact::SpeedCurve => 4,
        ClipFact::Beats => 5,
        ClipFact::PitchRidesSpeed => 6,
        ClipFact::OwnAudio => 7,
        ClipFact::DetachedAudio => 8,
        ClipFact::AtACut => 9,
    }
}

/// Every question, in declaration order. A surface walks this instead of
/// keeping its own list, so a new question appears there by existing.
pub const fn clip_facts() -> &'static [ClipFactSpec] {
    SHIPPED
}

/// The row with this wire id.
pub fn clip_fact_by_id(id: &str) -> Option<&'static ClipFactSpec> {
    SHIPPED.iter().find(|spec| spec.id == id)
}

impl ClipFact {
    /// The row describing this question.
    pub const fn spec(self) -> &'static ClipFactSpec {
        &SHIPPED[index_of(self)]
    }

    /// Stable wire / UI id.
    pub const fn id(self) -> &'static str {
        self.spec().id
    }

    /// Operator-facing label.
    pub const fn label(self) -> &'static str {
        self.spec().label
    }

    /// Answer this question for one clip of `timeline`.
    pub fn holds(self, timeline: &Timeline, clip: &Clip) -> bool {
        (self.spec().test)(timeline, clip)
    }
}

/// Every clip in `timeline` this question holds for, in lane then start order.
pub fn clips_with_fact(timeline: &Timeline, fact: ClipFact) -> Vec<ClipId> {
    let test = fact.spec().test;
    timeline
        .tracks_ordered()
        .flat_map(|track| track.clips_ordered())
        .filter(|clip| test(timeline, clip))
        .map(|clip| clip.id)
        .collect()
}

#[cfg(test)]
#[path = "audit_tests.rs"]
mod tests;
