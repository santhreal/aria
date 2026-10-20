//! The toggles a lane header draws.

use aria_engine::EngineError;
use aria_models::{SourceChannel, TrackId};

use super::Glyph;
use crate::app::AppState;
use crate::types::UiTrack;

/// One toggle in a lane header.
///
/// The header draws these rows and nothing else, so a lane control is a row here
/// or it is not in the product. The refusal branch belongs to the builder rather
/// than to each row, so seven controls cannot disagree about whether a refused
/// edit is reported.
pub struct LaneToggle {
    /// Element id prefix; the lane's own id is appended per row.
    pub id: &'static str,
    /// Whether the lane draws this button at all.
    pub shown: fn(LaneFlags) -> bool,
    /// Whether the button draws lit, given the lane it sits on.
    pub lit: fn(LaneFlags) -> bool,
    /// What the button draws in each state.
    pub glyph: Glyph,
    /// Where the header draws it, and whether it shows at rest.
    pub rest: Rest,
    /// Whether the lane is in a state this toggle shows at rest: a state a
    /// new lane does not start in, or one an edit depends on.
    pub flagged: fn(LaneFlags) -> bool,
    /// What pressing it asks the session for.
    pub run: fn(&mut AppState, LaneFlags) -> Result<(), EngineError>,
}

/// Where a lane header draws a toggle, and whether it shows before the pointer
/// is over the header. A toggle hidden at rest is still laid out, so the
/// controls never move under the pointer, and it shows at rest while its
/// lane is `flagged`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rest {
    /// Before the badge, always shown.
    Lead,
    /// After the name, shown on header hover.
    Hover,
    /// Shown on audio lanes, and on header hover on other lanes.
    Audio,
}

impl Rest {
    /// Whether the toggle shows with the pointer away from an unflagged lane
    /// that is `audio` or not.
    pub fn at_rest(self, audio: bool) -> bool {
        match self {
            Rest::Lead => true,
            Rest::Hover => false,
            Rest::Audio => audio,
        }
    }
}

impl LaneToggle {
    /// Whether the header draws this toggle with the pointer away, on `lane`,
    /// an audio lane when `audio`.
    pub fn shows_at_rest(&self, lane: LaneFlags, audio: bool) -> bool {
        self.rest.at_rest(audio) || (self.flagged)(lane)
    }
}

/// The lane facts a toggle reads and acts on.
///
/// Copied out of the projection while the header draws and carried into the
/// press, so a toggle acts on the drawn state. Small and `Copy` on
/// purpose: there is one of these per toggle per lane per frame, and a press must
/// not cost a copy of the lane's name.
#[derive(Clone, Copy)]
pub struct LaneFlags {
    pub id: u64,
    pub muted: bool,
    pub solo: bool,
    pub locked: bool,
    pub enabled: bool,
    pub collapsed: bool,
    pub targeted: bool,
    pub sync_lock: bool,
    /// The source channel the lane takes, if it takes one.
    pub channel: Option<SourceChannel>,
    /// A Source tab's edit lands the lane's channel here.
    pub patched: bool,
}

impl LaneFlags {
    /// Read one lane's flags out of the projection.
    pub fn of(lane: &UiTrack) -> Self {
        Self {
            id: lane.id,
            muted: lane.muted,
            solo: lane.solo,
            locked: lane.locked,
            enabled: lane.enabled,
            collapsed: lane.collapsed,
            targeted: lane.targeted,
            sync_lock: lane.sync_lock,
            channel: lane.channel,
            patched: lane.patched,
        }
    }
}

/// A button every lane draws.
const fn every_lane(_: LaneFlags) -> bool {
    true
}

/// Every toggle a lane header offers, in the order it draws them.
pub const LANE_TOGGLES: &[LaneToggle] = &[
    // Source patching: the channel letter is lit on the lane a Source tab's
    // picture or sound lands on. A lane that takes no source draws none.
    LaneToggle {
        id: "patch",
        shown: |lane| lane.channel.is_some(),
        lit: |lane| lane.patched,
        glyph: Glyph::Channel,
        rest: Rest::Lead,
        flagged: |lane| lane.patched,
        run: |state, lane| state.toggle_source_patch(lane),
    },
    LaneToggle {
        id: "mute",
        shown: every_lane,
        lit: |lane| lane.muted,
        glyph: Glyph::Icons {
            on: crate::icons::VOLUME_X,
            off: crate::icons::VOLUME,
        },
        rest: Rest::Audio,
        flagged: |lane| lane.muted,
        run: |state, lane| state.set_track_muted(TrackId::from_raw(lane.id), !lane.muted),
    },
    LaneToggle {
        id: "solo",
        shown: every_lane,
        lit: |lane| lane.solo,
        glyph: Glyph::Icons {
            on: crate::icons::HEADPHONES,
            off: crate::icons::HEADPHONES,
        },
        rest: Rest::Audio,
        flagged: |lane| lane.solo,
        run: |state, lane| state.toggle_track_solo(TrackId::from_raw(lane.id)),
    },
    LaneToggle {
        id: "collapse",
        shown: every_lane,
        lit: |lane| lane.collapsed,
        glyph: Glyph::Chevron,
        rest: Rest::Hover,
        flagged: |lane| lane.collapsed,
        run: |state, lane| state.toggle_lane_collapsed(lane.id),
    },
    LaneToggle {
        id: "lock",
        shown: every_lane,
        lit: |lane| lane.locked,
        glyph: Glyph::Icons {
            on: crate::icons::LOCK,
            off: crate::icons::LOCK_OPEN,
        },
        rest: Rest::Hover,
        flagged: |lane| lane.locked,
        run: |state, lane| state.set_track_locked(TrackId::from_raw(lane.id), !lane.locked),
    },
    // Lit means hidden: the control reads as an alarm rather than as a state,
    // because a lane that is not drawing is the usual cause of a missing picture.
    LaneToggle {
        id: "vis",
        shown: every_lane,
        lit: |lane| !lane.enabled,
        glyph: Glyph::Icons {
            on: crate::icons::EYE_OFF,
            off: crate::icons::EYE,
        },
        rest: Rest::Hover,
        flagged: |lane| !lane.enabled,
        run: |state, lane| state.set_track_enabled(TrackId::from_raw(lane.id), !lane.enabled),
    },
    // Routing: which lane an edit lands on, and which lanes ride along with an
    // insert.
    LaneToggle {
        id: "target",
        shown: every_lane,
        lit: |lane| lane.targeted,
        glyph: Glyph::Icons {
            on: crate::icons::CROSSHAIR,
            off: crate::icons::CROSSHAIR,
        },
        rest: Rest::Hover,
        flagged: |lane| lane.targeted,
        run: |state, lane| state.toggle_track_targeted(TrackId::from_raw(lane.id)),
    },
    LaneToggle {
        id: "sync",
        shown: every_lane,
        lit: |lane| lane.sync_lock,
        glyph: Glyph::Icons {
            on: crate::icons::LINK,
            off: crate::icons::UNLINK,
        },
        rest: Rest::Hover,
        flagged: |lane| !lane.sync_lock,
        run: |state, lane| state.toggle_track_sync_lock(TrackId::from_raw(lane.id)),
    },
];
