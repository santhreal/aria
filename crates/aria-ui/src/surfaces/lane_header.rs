//! The toggles a lane header draws.

use aria_engine::EngineError;
use aria_models::TrackId;

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
    /// Whether the button draws lit, given the lane it sits on.
    pub lit: fn(LaneFlags) -> bool,
    /// What the button draws in each state.
    pub glyph: Glyph,
    /// What pressing it asks the session for.
    pub run: fn(&mut AppState, LaneFlags) -> Result<(), EngineError>,
}

/// The lane facts a toggle reads and acts on.
///
/// Copied out of the projection while the header draws and carried into the
/// press, so a toggle acts on the state the operator saw. Small and `Copy` on
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
        }
    }
}

/// Every toggle a lane header offers, in the order it draws them.
pub const LANE_TOGGLES: &[LaneToggle] = &[
    LaneToggle {
        id: "mute",
        lit: |lane| lane.muted,
        glyph: Glyph::Icons {
            on: crate::icons::VOLUME_X,
            off: crate::icons::VOLUME,
        },
        run: |state, lane| state.set_track_muted(TrackId::from_raw(lane.id), !lane.muted),
    },
    LaneToggle {
        id: "solo",
        lit: |lane| lane.solo,
        glyph: Glyph::Icons {
            on: crate::icons::HEADPHONES,
            off: crate::icons::HEADPHONES,
        },
        run: |state, lane| state.toggle_track_solo(TrackId::from_raw(lane.id)),
    },
    LaneToggle {
        id: "collapse",
        lit: |lane| lane.collapsed,
        glyph: Glyph::Chevron,
        run: |state, lane| state.toggle_lane_collapsed(lane.id),
    },
    LaneToggle {
        id: "lock",
        lit: |lane| lane.locked,
        glyph: Glyph::Icons {
            on: crate::icons::LOCK,
            off: crate::icons::LOCK_OPEN,
        },
        run: |state, lane| state.set_track_locked(TrackId::from_raw(lane.id), !lane.locked),
    },
    // Lit means hidden: the control reads as an alarm rather than as a state,
    // because a lane that is not drawing is what an operator is looking for.
    LaneToggle {
        id: "vis",
        lit: |lane| !lane.enabled,
        glyph: Glyph::Icons {
            on: crate::icons::EYE_OFF,
            off: crate::icons::EYE,
        },
        run: |state, lane| state.set_track_enabled(TrackId::from_raw(lane.id), !lane.enabled),
    },
    // Routing: which lane an edit lands on, and which lanes ride along with an
    // insert.
    LaneToggle {
        id: "target",
        lit: |lane| lane.targeted,
        glyph: Glyph::Icons {
            on: crate::icons::CROSSHAIR,
            off: crate::icons::CROSSHAIR,
        },
        run: |state, lane| state.toggle_track_targeted(TrackId::from_raw(lane.id)),
    },
    LaneToggle {
        id: "sync",
        lit: |lane| lane.sync_lock,
        glyph: Glyph::Icons {
            on: crate::icons::LINK,
            off: crate::icons::UNLINK,
        },
        run: |state, lane| state.toggle_track_sync_lock(TrackId::from_raw(lane.id)),
    },
];
