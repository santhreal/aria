//! Every timeline lane kind, as data.
//!
//! A lane kind used to be an enum variant plus a hand-written arm in each place
//! that cared: what it accepts, whether it is audio, where it sinks in the lane
//! stack, the name a freshly created lane gets, the wire name an agent prints,
//! the label a header shows. Four crates carried their own copy of the wire-name
//! match alone, so a lane kind could be placeable, describable in three of them,
//! and unnamed in the fourth, with nothing red.
//!
//! A row here carries the lane's identity and every predicate those arms were
//! deciding. Placement is the interesting one: a lane accepts imported media if
//! its row says so, and accepts generated content if the generator's own row in
//! [`crate::clip::generator_catalog`] names this lane. That is one fact in one
//! place instead of a variant-by-variant matrix that had to agree with
//! `for_generator` by hand.
//!
//! # Fail-by-default
//!
//! `index_of` is an exhaustive match over [`TrackKind`], so a new lane kind
//! cannot compile until it names a row. `a_row_per_lane_kind` proves the arm
//! points at that kind's own row rather than reusing a neighbour's.

use crate::clip::generator_catalog::generator_spec;
use crate::clip::{Clip, ClipSource};
use crate::error::ModelError;
use crate::track::{TrackDisplay, TrackKind};

/// Where a lane sits in the lane stack, bottom to top.
///
/// The main video lane is a zone the *flag* selects, not one a lane kind does:
/// exactly one `Video` lane carries it and the rest stay in [`LaneZone::Stack`].
/// It is named here because the ordering rule needs all four ranks, and leaving
/// it out made `2` mean "above main" with nothing saying so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LaneZone {
    /// Below the audio floor: the bus lanes every other audio lane can route
    /// into. They hold no clips, so nothing is dragged past them, and they read
    /// as the bottom of the mix the way a mixer's masters do.
    Bus,
    /// The audio floor.
    Floor,
    /// The main video lane. No row declares it; [`Track::main`] does.
    ///
    /// [`Track::main`]: crate::Track::main
    Main,
    /// Visual lanes stacked above the main lane.
    Stack,
    /// The text ceiling.
    Ceiling,
}

impl LaneZone {
    /// Sort rank, bottom to top.
    pub const fn rank(self) -> u8 {
        self as u8
    }
}

/// What one lane kind is and what it accepts.
///
/// Every column is a question some surface used to answer with its own match
/// arm.
#[derive(Debug, Clone, Copy)]
pub struct TrackKindSpec {
    /// The lane kind this row describes.
    pub kind: TrackKind,
    /// Stable wire name. What an agent, a Python binding, or a projection
    /// prints, and what parses back to the kind.
    pub id: &'static str,
    /// Operator-facing lane label.
    pub label: &'static str,
    /// Name a freshly created lane of this kind gets when the caller has none.
    pub default_name: &'static str,
    /// Whether the lane's clips are sound rather than picture.
    pub is_audio: bool,
    /// Whether the lane's clips composite into the picture stack.
    pub is_visual: bool,
    /// Whether imported media may be placed here. Generated content is decided
    /// by the generator's own row, not by a list on this one.
    pub accepts_media: bool,
    /// Whether clips on this lane composite as frames, and can therefore take a
    /// transition at their junctions. Audio has no frame; a canvas-wide pass
    /// cannot be nested inside a transition.
    pub supports_transitions: bool,
    /// Whether a lane of this kind can be the main track.
    pub can_be_main: bool,
    /// Whether a media clip's own audio belongs to this lane. Callers still
    /// layer mute, solo, and detach state on top.
    pub carries_audio: bool,
    /// Whether "extract audio" is offered for a clip here: detaching a picture
    /// clip's sound onto a linked audio-lane companion.
    pub extracts_audio: bool,
    /// Whether the lane holds no clips of its own and exists to sum what other
    /// lanes route into it: a submix. Routing destinations are chosen by this
    /// column, and implicit cleanup leaves such a lane alone when it is empty,
    /// because empty is what it always is.
    pub is_bus: bool,
    /// Whether an assistant may create a lane of this kind. The lanes an
    /// operator builds a sequence out of, plus the buses they mix into; the
    /// canvas-wide effect, filter and adjustment lanes are applied to clips
    /// instead, so an assistant has no reason to add one.
    pub agent_creates: bool,
    /// Where the lane sinks or rises to in the lane stack.
    pub zone: LaneZone,
    /// Lane height a freshly created lane of this kind gets, in logical pixels.
    /// One height ships for every lane today; a per-kind height is this column.
    pub default_height: u32,
}

impl TrackKindSpec {
    /// Sort rank in the lane stack, taking into account whether this lane is designated as main.
    pub const fn zone_rank(&self, is_main: bool) -> u8 {
        if is_main && self.can_be_main {
            LaneZone::Main.rank()
        } else {
            self.zone.rank()
        }
    }

    ///
    /// Imported media follows [`TrackKindSpec::accepts_media`]; generated
    /// content follows the generator's declared lane, so the placement rule and
    /// [`TrackKind::for_generator`] cannot disagree.
    pub fn accepts_content(&self, content: &ClipSource) -> bool {
        match content {
            ClipSource::Media { .. }
            | ClipSource::Multicam { .. }
            | ClipSource::Sequence { .. } => self.accepts_media,
            ClipSource::Generated(generator) => generator_spec(generator).lane == self.kind,
        }
    }

    /// Whether `clip` may be placed on a lane of this kind.
    pub fn accepts_clip(&self, clip: &Clip) -> bool {
        self.accepts_content(&clip.content)
    }

    /// Whether a lane of this kind appears in the mixer at all: it either holds
    /// sound of its own, or it is a bus that sums other lanes.
    ///
    /// The one owner of that question. `aria-render` decides what to decode
    /// with it, and the mixer surface decides what to draw.
    pub const fn in_mixer(&self) -> bool {
        self.is_audio || self.carries_audio || self.is_bus
    }
}

/// Which kind of intermediate a clip on a lane bakes to.
///
/// The lane decides it, from its own [`TrackKindSpec`] row rather than from a
/// list kept beside it: a lane kind added later picks its medium by filling
/// that row in, and one that is neither picture nor sound is refused by name
/// instead of being rendered as the wrong thing.
///
/// It lives with the catalog rather than with the render that reads it because
/// two peers ask the question — the pass that renders a clip, and the assistant
/// tool that asks for one — and a second copy of the mapping is how they would
/// come to disagree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BakeMedium {
    /// A picture file, carrying its own sound when the clip has any.
    Picture,
    /// A sound file, with no picture in it at all.
    Sound,
}

impl BakeMedium {
    /// The medium a lane of `kind` bakes to, or a refusal naming the lane.
    ///
    /// Reads the lane's row: a bus holds no clips to bake, a picture lane bakes
    /// a picture, and a sound lane bakes sound.
    pub fn for_lane(kind: TrackKind) -> Result<Self, ModelError> {
        let spec = kind.spec();
        if spec.is_bus {
            return Err(ModelError::InvalidParam(format!(
                "{} sums other lanes and holds no clip to render",
                spec.label
            )));
        }
        if spec.is_visual {
            return Ok(Self::Picture);
        }
        if spec.is_audio {
            return Ok(Self::Sound);
        }
        Err(ModelError::InvalidParam(format!(
            "a clip on {} is neither picture nor sound, so nothing can be rendered to stand in for it",
            spec.label
        )))
    }
}

/// The shipped lane kinds, in declaration order.
pub const SHIPPED: &[TrackKindSpec] = &[
    TrackKindSpec {
        kind: TrackKind::Video,
        id: "video",
        label: "Overlay",
        default_name: "V1",
        is_audio: false,
        is_visual: true,
        accepts_media: true,
        supports_transitions: true,
        can_be_main: true,
        carries_audio: true,
        extracts_audio: true,
        is_bus: false,
        agent_creates: true,
        zone: LaneZone::Stack,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Audio,
        id: "audio",
        label: "Audio",
        default_name: "A1",
        is_audio: true,
        is_visual: false,
        accepts_media: true,
        supports_transitions: false,
        can_be_main: false,
        carries_audio: true,
        extracts_audio: false,
        is_bus: false,
        agent_creates: true,
        zone: LaneZone::Floor,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Text,
        id: "text",
        label: "Text",
        default_name: "T1",
        is_audio: false,
        is_visual: true,
        accepts_media: false,
        supports_transitions: true,
        can_be_main: false,
        carries_audio: false,
        extracts_audio: false,
        is_bus: false,
        agent_creates: true,
        zone: LaneZone::Ceiling,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Sticker,
        id: "sticker",
        label: "Sticker",
        default_name: "S1",
        is_audio: false,
        is_visual: true,
        accepts_media: false,
        supports_transitions: true,
        can_be_main: false,
        carries_audio: false,
        extracts_audio: false,
        is_bus: false,
        agent_creates: true,
        zone: LaneZone::Stack,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Effect,
        id: "effect",
        label: "Effects",
        default_name: "E1",
        is_audio: false,
        is_visual: true,
        accepts_media: false,
        supports_transitions: false,
        can_be_main: false,
        carries_audio: false,
        extracts_audio: false,
        is_bus: false,
        agent_creates: false,
        zone: LaneZone::Stack,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Filter,
        id: "filter",
        label: "Filters",
        default_name: "F1",
        is_audio: false,
        is_visual: true,
        accepts_media: false,
        supports_transitions: false,
        can_be_main: false,
        carries_audio: false,
        extracts_audio: false,
        is_bus: false,
        agent_creates: false,
        zone: LaneZone::Stack,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Adjustment,
        id: "adjustment",
        label: "Adjust",
        default_name: "FX1",
        is_audio: false,
        is_visual: true,
        accepts_media: false,
        supports_transitions: false,
        can_be_main: false,
        carries_audio: false,
        extracts_audio: false,
        is_bus: false,
        agent_creates: false,
        zone: LaneZone::Stack,
        default_height: TrackDisplay::DEFAULT_HEIGHT,
    },
    TrackKindSpec {
        kind: TrackKind::Submix,
        id: "submix",
        label: "Submix",
        default_name: "Bus 1",
        is_audio: true,
        is_visual: false,
        accepts_media: false,
        supports_transitions: false,
        can_be_main: false,
        carries_audio: false,
        extracts_audio: false,
        is_bus: true,
        agent_creates: true,
        zone: LaneZone::Bus,
        default_height: TrackDisplay::BUS_HEIGHT,
    },
];

/// Which row in [`SHIPPED`] describes `kind`.
///
/// Exhaustive on purpose, and the whole fail-by-default mechanism for this axis:
/// a lane kind added to `track.rs` and nowhere else does not compile.
const fn index_of(kind: TrackKind) -> usize {
    match kind {
        TrackKind::Video => 0,
        TrackKind::Audio => 1,
        TrackKind::Text => 2,
        TrackKind::Sticker => 3,
        TrackKind::Effect => 4,
        TrackKind::Filter => 5,
        TrackKind::Adjustment => 6,
        TrackKind::Submix => 7,
    }
}

/// Every lane kind, in declaration order.
///
/// A surface that offers "add a lane" walks this instead of keeping its own
/// list, so a new lane kind appears there by existing.
pub const fn track_kind_specs() -> &'static [TrackKindSpec] {
    SHIPPED
}

/// The row describing `kind`.
pub const fn track_kind_spec(kind: TrackKind) -> &'static TrackKindSpec {
    &SHIPPED[index_of(kind)]
}

/// The row with this wire id, for surfaces that address a lane kind by name.
pub fn track_kind_by_id(id: &str) -> Option<&'static TrackKindSpec> {
    SHIPPED.iter().find(|spec| spec.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clip::generator_catalog;

    /// The exhaustive `index_of` match is the compile-time gate; this is the
    /// proof that each arm points at its *own* row rather than a neighbour's,
    /// which would compile and silently mis-describe a lane.
    ///
    /// Both sides are derived from [`SHIPPED`]: nothing here counts variants.
    #[test]
    fn a_row_per_lane_kind() {
        for (index, spec) in SHIPPED.iter().enumerate() {
            assert_eq!(
                index_of(spec.kind),
                index,
                "the '{}' row resolves to '{}' instead",
                spec.id,
                SHIPPED[index_of(spec.kind)].id
            );
            assert_eq!(
                track_kind_spec(spec.kind).id,
                spec.id,
                "track_kind_spec did not return the '{}' row",
                spec.id
            );
            assert_eq!(
                track_kind_by_id(spec.id).map(|found| found.kind),
                Some(spec.kind),
                "'{}' is not reachable by id",
                spec.id
            );
        }
    }

    #[test]
    fn every_row_has_a_distinct_id_name_and_label() {
        for column in [
            SHIPPED.iter().map(|s| s.id).collect::<Vec<_>>(),
            SHIPPED.iter().map(|s| s.default_name).collect(),
        ] {
            let mut sorted = column.clone();
            sorted.sort_unstable();
            sorted.dedup();
            assert_eq!(
                sorted.len(),
                column.len(),
                "two lane rows share a value in {column:?}"
            );
        }
        for spec in SHIPPED {
            assert!(!spec.label.is_empty(), "'{}' has no label", spec.id);
            assert!(
                spec.default_height >= *TrackDisplay::HEIGHT.start()
                    && spec.default_height <= *TrackDisplay::HEIGHT.end(),
                "'{}' ships a height outside the accepted range",
                spec.id
            );
        }
    }

    /// Audio and picture are complementary here: a lane carries one or the
    /// other, never both and never neither.
    #[test]
    fn audio_and_visual_are_opposites() {
        for spec in SHIPPED {
            assert_ne!(
                spec.is_audio, spec.is_visual,
                "'{}' claims is_audio={} and is_visual={}",
                spec.id, spec.is_audio, spec.is_visual
            );
        }
    }

    /// A lane that takes a transition must composite frames, and only the
    /// picture stack does. A lane that can be main must be one that takes
    /// media, since the main track is the footage floor.
    #[test]
    fn lane_predicates_do_not_contradict_each_other() {
        for spec in SHIPPED {
            assert!(
                !spec.supports_transitions || spec.is_visual,
                "'{}' takes transitions without compositing frames",
                spec.id
            );
            assert!(
                !spec.can_be_main || (spec.is_visual && spec.accepts_media),
                "'{}' can be main without being a visual media lane",
                spec.id
            );
            assert!(
                !spec.extracts_audio || (spec.is_visual && spec.carries_audio),
                "'{}' extracts audio it never carried",
                spec.id
            );
            assert!(
                !spec.is_audio || spec.zone <= LaneZone::Floor,
                "'{}' is audio but does not sink to the floor or below it",
                spec.id
            );
            // A bus is the one lane that holds nothing: no imported media, no
            // generated content (no generator row names it), and no sound of
            // its own. Everything it carries was routed in.
            assert_eq!(
                spec.is_bus,
                spec.zone == LaneZone::Bus,
                "'{}' disagrees with its own zone about being a bus",
                spec.id
            );
            assert!(
                !spec.is_bus || (spec.is_audio && !spec.accepts_media && !spec.carries_audio),
                "'{}' is a bus that also holds media or sound of its own",
                spec.id
            );
            assert_eq!(
                spec.in_mixer(),
                spec.is_audio || spec.carries_audio || spec.is_bus,
                "'{}' answers the mixer question two ways",
                spec.id
            );
        }
    }

    /// Every generator names a lane that exists, and every generator is
    /// placeable on exactly the lane its row names. Both lists are derived, so
    /// a generator whose lane has no row, or a lane whose acceptance rule drifts
    /// from the generator catalog, turns this red.
    #[test]
    fn every_generator_is_placeable_on_exactly_its_declared_lane() {
        for generator_row in generator_catalog::SHIPPED {
            let content = ClipSource::Generated((generator_row.census)());
            let accepting: Vec<&str> = SHIPPED
                .iter()
                .filter(|lane| lane.accepts_content(&content))
                .map(|lane| lane.id)
                .collect();
            assert_eq!(
                accepting,
                [track_kind_spec(generator_row.lane).id],
                "'{}' is placeable on {accepting:?}",
                generator_row.id
            );
        }
    }

    /// Exactly the lanes that declare `accepts_media` take imported media, and
    /// they are the ones that carry audio or composite footage.
    #[test]
    fn media_placement_follows_the_accepts_media_column() {
        use crate::MediaId;
        let media = ClipSource::Media {
            media: MediaId::from_raw(1),
            source: crate::TimeRange::at_rate(0, 1, crate::Rational::FPS_30),
        };
        for spec in SHIPPED {
            assert_eq!(
                spec.accepts_content(&media),
                spec.accepts_media,
                "'{}' disagrees with its own accepts_media column",
                spec.id
            );
        }
    }
}
