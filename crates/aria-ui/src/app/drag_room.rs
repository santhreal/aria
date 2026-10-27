//! How far a drag on a clip travels before the edit it sends is refused.
//!
//! A trim stops at the end of the take, at the clip beside it and at the
//! sequence start. A roll and a slide stop where a clip they change runs out
//! of take or down to one tick, and a slide with open lane on one side stops
//! at the next clip there. A rate stretch stops at the clip beside the edge
//! it drags and at one tick. The release sends the travel held inside this
//! room and the readout states it. Each take's reach is read off the model's
//! own trim check, so a drag held inside the room is not refused at release
//! for going too far.

use aria_models::{ClipId, Project, TrimReach};

use crate::app::AppState;
use crate::app::trim_gestures::{Neighbour, roll_room, slide_room};

/// What bounds a drag's travel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RoomKind {
    /// One edge trimmed inside its lane, the other held.
    Trim { left: bool },
    /// One edge trimmed while the rest of the lane moves out of its way.
    Ripple { left: bool },
    /// The cut at the clip's head or tail, between it and the clip it abuts.
    Roll { left: bool },
    /// The clip moved between its neighbors, which give and take the travel.
    Slide,
    /// The clip retimed by one edge, the other held.
    Stretch { left: bool },
}

/// The travel a drag takes, in ticks: from the least, at or below zero, to
/// the most, at or above it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Room {
    pub least: i64,
    pub most: i64,
}

impl Room {
    /// No bound: the model checks the edit at release.
    pub(crate) const OPEN: Self = Self {
        least: i64::MIN,
        most: i64::MAX,
    };

    /// A room that always holds a drag that stays where it started.
    fn new(least: i64, most: i64) -> Self {
        Self {
            least: least.min(0),
            most: most.max(0),
        }
    }

    /// `travel` held inside the room.
    pub(crate) fn hold(self, travel: i64) -> i64 {
        travel.max(self.least).min(self.most)
    }

    /// The room of the edge a trim with `reach` moves, as travel from where
    /// the edge is.
    fn of_edge(reach: Option<TrimReach>, left: bool, at: Neighbour) -> Self {
        let Some(reach) = reach else {
            return Self::OPEN;
        };
        if left {
            Self::new(
                reach
                    .start
                    .0
                    .map_or(i64::MIN, |earliest| earliest - at.start),
                reach.start.1 - at.start,
            )
        } else {
            Self::new(
                reach.end.0 - at.end,
                reach.end.1.map_or(i64::MAX, |latest| latest - at.end),
            )
        }
    }
}

/// The room of the drag in flight, read once per drag rather than once per
/// pointer move: reading it locks the engine, which a preview render holds
/// for the length of a frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct HeldRoom {
    kind: RoomKind,
    clip: ClipId,
    linked: bool,
    revision: u64,
    room: Room,
}

impl AppState {
    /// The room of a `kind` drag on `clip`: the held one while that drag is
    /// in flight on this revision, else read now off the session, for a
    /// release or a key that trims with no drag holding the room.
    pub(crate) fn drag_room(&self, kind: RoomKind, clip: ClipId) -> Room {
        match self.held_room {
            Some(held) if self.holds_room(held, kind, clip) => held.room,
            Some(_) | None => {
                let engine = self.engine.lock();
                self.read_room(engine.project(), kind, clip)
            }
        }
    }

    /// Hold the room of a `kind` drag on `clip` for the rest of the drag,
    /// read off `project`, the project the drag holds. `None` drops it, for a
    /// drag that has none.
    pub(crate) fn hold_room(&mut self, kind: Option<RoomKind>, clip: ClipId, project: &Project) {
        let Some(kind) = kind else {
            self.held_room = None;
            return;
        };
        if let Some(held) = self.held_room
            && self.holds_room(held, kind, clip)
        {
            return;
        }
        self.held_room = Some(HeldRoom {
            kind,
            clip,
            linked: self.linked_selection,
            revision: self.revision,
            room: self.read_room(project, kind, clip),
        });
    }

    fn holds_room(&self, held: HeldRoom, kind: RoomKind, clip: ClipId) -> bool {
        held.kind == kind
            && held.clip == clip
            && held.linked == self.linked_selection
            && held.revision == self.revision
    }

    fn read_room(&self, project: &Project, kind: RoomKind, clip: ClipId) -> Room {
        let linked = self.linked_selection;
        let Some(at) = project.clip(clip).map(|held| Neighbour {
            clip,
            start: held.timeline.start.value,
            end: held.timeline.end_tick(),
        }) else {
            return Room::OPEN;
        };
        // With `lane` false only the take bounds the edge: the edit moves
        // whatever else is in its way.
        let reach = |id: ClipId, lane: bool| project.trim_reach(id, lane, linked).ok();
        match kind {
            RoomKind::Trim { left } => Room::of_edge(reach(clip, true), left, at),
            RoomKind::Ripple { left } => Room::of_edge(reach(clip, false), left, at),
            RoomKind::Roll { left } => {
                let (before, after) = self.abutting(clip);
                let pair = if left {
                    before.map(|prev| (prev, at))
                } else {
                    after.map(|next| (at, next))
                };
                // No cut to roll: the drag does nothing.
                let Some((cut_left, cut_right)) = pair else {
                    return Room::new(0, 0);
                };
                // The clip the cut moves into keeps a tick, and the one it
                // moves away from grows as far as its take.
                let grows_left = Room::of_edge(reach(cut_left.clip, false), false, cut_left);
                let grows_right = Room::of_edge(reach(cut_right.clip, false), true, cut_right);
                Room::new(
                    roll_room(cut_left, cut_right, -i64::MAX).max(grows_right.least),
                    roll_room(cut_left, cut_right, i64::MAX).min(grows_left.most),
                )
            }
            RoomKind::Slide => {
                let (before, after) = self.abutting(clip);
                let (behind, ahead) = self.open_lane(at);
                let mut least = slide_room(at.start, at.end, -i64::MAX, before, after);
                let mut most = slide_room(at.start, at.end, i64::MAX, before, after);
                // The neighbor the clip leaves grows as far as its take; with
                // none, the clip stops at the next clip or the sequence start.
                match before {
                    Some(prev) => {
                        most = most.min(Room::of_edge(reach(prev.clip, false), false, prev).most);
                    }
                    None => least = least.max(-behind),
                }
                match after {
                    Some(next) => {
                        least = least.max(Room::of_edge(reach(next.clip, false), true, next).least);
                    }
                    None => most = most.min(ahead),
                }
                Room::new(least, most)
            }
            RoomKind::Stretch { left } => {
                let (behind, ahead) = self.open_lane(at);
                let length = at.end - at.start;
                if left {
                    Room::new(-behind, length - 1)
                } else {
                    Room::new(1 - length, ahead)
                }
            }
        }
    }

    /// The open lane on each side of `at`, as the store draws it: back to
    /// the clip before it or the sequence start, and on to the clip after
    /// it, `i64::MAX` with none.
    fn open_lane(&self, at: Neighbour) -> (i64, i64) {
        let raw = at.clip.raw();
        let Some(lane) = self
            .store
            .sequence
            .tracks
            .iter()
            .find(|lane| lane.clips.iter().any(|item| item.id == raw))
        else {
            return (at.start.max(0), i64::MAX);
        };
        let others = || lane.clips.iter().filter(|item| item.id != raw);
        let behind = others()
            .map(|item| item.timeline_end_ticks)
            .filter(|&end| end <= at.start)
            .max()
            .unwrap_or(0);
        let ahead = others()
            .map(|item| item.timeline_start_ticks)
            .filter(|&start| start >= at.end)
            .min();
        (
            at.start - behind,
            ahead.map_or(i64::MAX, |start| start - at.end),
        )
    }
}
