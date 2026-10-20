//! The controls pressed in the pool sidebar: Make Subclip,
//! the multicam button, and a group's angle chips.
//!
//! Three commands claimed these buttons and nothing could press them. Two of the
//! three also swallowed their refusal: the multicam button discarded the result,
//! so a group the model refused looked exactly like one it built.
//!
//! What a multicam group is built from lives here rather than in the panel,
//! because it is the row's own rule and the census has to read it: the pool's
//! selected items, in the order they were picked, and every item in the pool
//! when nothing is selected. Grouping a whole card's worth of angles right after
//! importing them is what that second case is for.
//!
//! The pool's other rows stay out. A card click is
//! [`pool_pick`](crate::surfaces::pool_pick), a double click opens the monitor,
//! and a bin drag is [`pool_drop`](crate::surfaces::pool_drop).

use aria_engine::EngineError;
use aria_models::{MediaId, MulticamAngle, MulticamGroupId};

use crate::app::AppState;

/// One control the pool sidebar draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    /// Make a pool entry from the source monitor's marked range.
    Subclip,
    /// Build a camera group from the pool.
    MulticamGroup,
    /// Cut a group to one of its angles at the playhead.
    MulticamAngle {
        group: MulticamGroupId,
        angle: usize,
    },
    /// Put a group on a video lane at the playhead.
    MulticamPlace { group: MulticamGroupId },
    /// Move one angle on its group's clock by a number of frames.
    MulticamNudge {
        group: MulticamGroupId,
        angle: usize,
        frames: i64,
    },
    /// Place every camera of a group against its first, measured from sound.
    MulticamSync { group: MulticamGroupId },
}

impl Pick {
    /// The pressed control, for a census and for a failure message.
    pub fn what(&self) -> &'static str {
        match self {
            Self::Subclip => "make a subclip of the marked source",
            Self::MulticamGroup => "build a multicam group from the pool",
            Self::MulticamAngle { .. } => "cut a multicam group to an angle",
            Self::MulticamPlace { .. } => "place a multicam group on the timeline",
            Self::MulticamNudge { .. } => "nudge a multicam angle on the group's clock",
            Self::MulticamSync { .. } => "sync a multicam group's angles by sound",
        }
    }
}

/// The angles the button builds a group from.
///
/// One angle per selected item, in the order they were picked, and the whole
/// pool when nothing is selected: grouping every camera file that was just
/// imported is what the button is for.
pub fn angles_from_pool(state: &AppState) -> Vec<MulticamAngle> {
    let selected = state.selected_pool_media_ids();
    let ids: Vec<MediaId> = if selected.is_empty() {
        state
            .store
            .media
            .iter()
            .map(|item| MediaId::from_raw(item.id))
            .collect()
    } else {
        selected
    };
    ids.into_iter()
        .map(|id| MulticamAngle::new(id, 0))
        .collect()
}

/// The name the next group gets: the count it will be, so two presses do not
/// draw two rows with the same label.
fn next_group_name(state: &AppState) -> String {
    format!("Multicam {}", state.multicam_groups().len() + 1)
}

/// Report in the status line why the button did nothing.
fn say_the_pool_is_empty(state: &mut AppState) {
    state.status_message = Some("Import media to build a multicam group".into());
}

/// Press one control on the open project.
pub fn apply(state: &mut AppState, pick: Pick) -> Result<(), EngineError> {
    match pick {
        Pick::Subclip => state.create_subclip_from_source(None).map(|_| ()),
        Pick::MulticamGroup => {
            let angles = angles_from_pool(state);
            if angles.is_empty() {
                say_the_pool_is_empty(state);
                return Ok(());
            }
            let name = next_group_name(state);
            state.create_multicam_group(name, angles).map(|_| ())
        }
        Pick::MulticamAngle { group, angle } => state.switch_multicam_angle(group, angle),
        Pick::MulticamPlace { group } => state.place_multicam_group(group),
        Pick::MulticamNudge {
            group,
            angle,
            frames,
        } => state.nudge_multicam_angle_offset(group, angle, frames),
        Pick::MulticamSync { group } => state.sync_multicam_group_by_audio(group),
    }
}
