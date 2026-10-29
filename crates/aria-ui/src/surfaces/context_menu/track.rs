//! The rows a lane header's menu draws.
//!
//! Split out of the parent by size. A lane height is a row here rather than a
//! branch in the panel, so a fourth height is a fourth row.

use aria_models::{TrackDisplay, TrackId, TrackKind};

use crate::app::AppState;
use crate::types::RenameTarget;

use super::{MenuItem, MenuRow, always};

/// The lane facts a track menu row reads.
#[derive(Debug, Clone, Copy)]
pub struct TrackMenu {
    pub track: u64,
    pub muted: bool,
    /// The lane carries sound, which is the only kind a duck source can be.
    pub audio: bool,
    /// The lane is the voice auto-duck listens to, which Duck Source's check
    /// mark shows.
    pub duck_source: bool,
    /// Where the lane sits in the drawn stack, and how tall the stack is, so the
    /// rows that move it are offered only where there is somewhere to move.
    pub row: usize,
    pub lanes: usize,
    /// How the lane is drawn: height, waveforms, filmstrips, collapsed.
    pub lane: TrackDisplay,
}

impl TrackMenu {
    /// Read the lane facts a menu would gather when it opens over `track`, or
    /// `None` when that lane is no longer projected.
    pub fn of(state: &AppState, track: u64) -> Option<Self> {
        let lanes = &state.store.sequence.tracks;
        let row = lanes.iter().position(|lane| lane.id == track)?;
        let lane = &lanes[row];
        Some(Self {
            track,
            muted: lane.muted,
            audio: lane.kind == crate::types::UiTrackKind::Audio,
            duck_source: lane.duck_source,
            row,
            lanes: lanes.len(),
            lane: state.lane_display(track),
        })
    }
}

/// One lane-height preset row in the track context menu. The row whose height
/// the lane holds draws the check mark.
struct LaneHeightRow {
    id: &'static str,
    label: &'static str,
    height: u32,
}

impl LaneHeightRow {
    /// Whether `lane` is drawn at this row's height.
    const fn holds(&self, lane: &TrackMenu) -> bool {
        lane.lane.height == self.height
    }
}

/// The lane heights the menu offers, derived from [`aria_models::TrackDisplay::PRESETS`].
const LANE_HEIGHTS: [LaneHeightRow; aria_models::TrackDisplay::PRESETS.len()] = [
    LaneHeightRow {
        id: "ctx-lane-mini",
        label: "Lane Height: Mini",
        height: aria_models::TrackDisplay::PRESETS[0].1,
    },
    LaneHeightRow {
        id: "ctx-lane-small",
        label: "Lane Height: Small",
        height: aria_models::TrackDisplay::PRESETS[1].1,
    },
    LaneHeightRow {
        id: "ctx-lane-medium",
        label: "Lane Height: Medium",
        height: aria_models::TrackDisplay::PRESETS[2].1,
    },
    LaneHeightRow {
        id: "ctx-lane-large",
        label: "Lane Height: Large",
        height: aria_models::TrackDisplay::PRESETS[3].1,
    },
];

/// A track menu, in the order it draws.
pub const TRACK_ITEMS: &[MenuItem<TrackMenu>] = &[
    MenuItem::Row(MenuRow {
        id: "ctx-add-track",
        label: |_| "Add Track",
        when: always(),
        checked: None,
        run: |state, _| {
            // Named after the lanes that exist, so a stack of three offers V4
            // rather than a second V2.
            let next = state
                .store
                .sequence
                .tracks
                .iter()
                .filter(|lane| matches!(lane.kind, crate::types::UiTrackKind::Video))
                .count()
                + 1;
            state
                .add_track(TrackKind::Video, format!("V{next}"))
                .map(|_| ())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-rename-track",
        label: |_| "Rename Track",
        when: always(),
        checked: None,
        run: |state, lane| {
            state.close_context();
            state.begin_rename(RenameTarget::Track(lane.track));
            Ok(())
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-delete-track",
        label: |_| "Delete Track",
        when: always(),
        checked: None,
        run: |state, lane| state.remove_track(TrackId::from_raw(lane.track)),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-mute-track",
        // The row names where the click lands, not where the lane is.
        label: |lane| if lane.muted { "Unmute" } else { "Mute" },
        when: always(),
        checked: None,
        run: |state, lane| state.set_track_muted(TrackId::from_raw(lane.track), !lane.muted),
    }),
    // Offered where there is somewhere to move: the top lane has no row above it,
    // and a row that does nothing when clicked is worse than no row.
    MenuItem::Row(MenuRow {
        id: "ctx-track-up",
        label: |_| "Move Track Up",
        when: |lane| lane.row > 0,
        checked: None,
        run: |state, lane| state.move_track_up(TrackId::from_raw(lane.track)),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-track-down",
        label: |_| "Move Track Down",
        when: |lane| lane.row + 1 < lane.lanes,
        checked: None,
        run: |state, lane| state.move_track_down(TrackId::from_raw(lane.track)),
    }),
    // Auto-duck listens to one voice, and a voice is sound.
    MenuItem::Row(MenuRow {
        id: "ctx-track-duck",
        label: |_| "Duck Source",
        when: |lane| lane.audio,
        checked: Some(|lane| lane.duck_source),
        run: |state, lane| state.toggle_track_duck_source(TrackId::from_raw(lane.track)),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-track-open-gap",
        label: |_| "Open Gap at Playhead",
        when: always(),
        checked: None,
        run: |state, lane| {
            // One second of gap, so the amount reads the same at any frame rate.
            let delta = state.store.sequence.frame_rate.nominal_fps();
            state.shift_clips_from_playhead(TrackId::from_raw(lane.track), delta)
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-track-close-gap",
        label: |_| "Close Gap at Playhead",
        when: always(),
        checked: None,
        run: |state, lane| {
            let delta = state.store.sequence.frame_rate.nominal_fps();
            state.shift_clips_from_playhead(TrackId::from_raw(lane.track), -delta)
        },
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-lane-collapse",
        label: |lane| {
            if lane.lane.collapsed {
                "Expand Lane"
            } else {
                "Collapse Lane"
            }
        },
        when: always(),
        checked: None,
        run: |state, lane| state.set_lane_collapsed(lane.track, !lane.lane.collapsed),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-lane-waveform",
        label: |lane| {
            if lane.lane.waveform {
                "Hide Waveforms"
            } else {
                "Show Waveforms"
            }
        },
        when: always(),
        checked: None,
        run: |state, lane| state.set_lane_waveform(lane.track, !lane.lane.waveform),
    }),
    MenuItem::Row(MenuRow {
        id: "ctx-lane-filmstrip",
        label: |lane| {
            if lane.lane.filmstrip {
                "Hide Filmstrips"
            } else {
                "Show Filmstrips"
            }
        },
        when: always(),
        checked: None,
        run: |state, lane| state.set_lane_filmstrip(lane.track, !lane.lane.filmstrip),
    }),
    MenuItem::Row(MenuRow {
        id: LANE_HEIGHTS[0].id,
        label: |_| LANE_HEIGHTS[0].label,
        when: always(),
        checked: Some(|lane| LANE_HEIGHTS[0].holds(lane)),
        run: |state, lane| state.set_lane_height(lane.track, LANE_HEIGHTS[0].height as f32),
    }),
    MenuItem::Row(MenuRow {
        id: LANE_HEIGHTS[1].id,
        label: |_| LANE_HEIGHTS[1].label,
        when: always(),
        checked: Some(|lane| LANE_HEIGHTS[1].holds(lane)),
        run: |state, lane| state.set_lane_height(lane.track, LANE_HEIGHTS[1].height as f32),
    }),
    MenuItem::Row(MenuRow {
        id: LANE_HEIGHTS[2].id,
        label: |_| LANE_HEIGHTS[2].label,
        when: always(),
        checked: Some(|lane| LANE_HEIGHTS[2].holds(lane)),
        run: |state, lane| state.set_lane_height(lane.track, LANE_HEIGHTS[2].height as f32),
    }),
    MenuItem::Row(MenuRow {
        id: LANE_HEIGHTS[3].id,
        label: |_| LANE_HEIGHTS[3].label,
        when: always(),
        checked: Some(|lane| LANE_HEIGHTS[3].holds(lane)),
        run: |state, lane| state.set_lane_height(lane.track, LANE_HEIGHTS[3].height as f32),
    }),
];
