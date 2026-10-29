use aria_models::{ModelError, RemovedTrack, Track, TrackId, TrackKind};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

pub(crate) struct RemoveTrackAction {
    pub track_id: TrackId,
}

pub(crate) struct InsertTrackAction {
    /// What the removal took out: the lane, and the routes other lanes had
    /// pointing at it. Both go back, so one undo restores the mix as well as
    /// the lane.
    pub removed: RemovedTrack,
    pub order_index: usize,
}

impl InsertTrackAction {
    /// Insert a lane that was never on the timeline: nothing was routed into it,
    /// so there is nothing to put back with it.
    pub(crate) fn fresh(track: Track, order_index: usize) -> Self {
        Self {
            removed: RemovedTrack {
                track,
                detached: Vec::new(),
            },
            order_index,
        }
    }
}

pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    kind: TrackKind,
    name: impl Into<String>,
    index: Option<usize>,
    pinned: bool,
) -> Result<(TrackId, Box<dyn EditAction>), EngineError> {
    let mut track = Track::new(kind, name);
    track.pinned = pinned;
    let timeline = ctx.project.timeline_mut();
    let id = match index {
        Some(order_index) => timeline.insert_track(track, order_index),
        None => timeline.add_track(track),
    };
    Ok((id, Box::new(RemoveTrackAction { track_id: id })))
}

impl EditAction for RemoveTrackAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let order_index = ctx
            .project
            .timeline()
            .order()
            .iter()
            .position(|&id| id == self.track_id)
            .ok_or(ModelError::UnknownTrack(self.track_id))?;
        let removed = ctx
            .project
            .timeline_mut()
            .remove_track(self.track_id)
            .ok_or(ModelError::UnknownTrack(self.track_id))?;
        Ok(Box::new(InsertTrackAction {
            removed,
            order_index,
        }))
    }
}

impl EditAction for InsertTrackAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let id = self.removed.track.id;
        ctx.project
            .timeline_mut()
            .restore_track(self.removed, self.order_index)?;
        Ok(Box::new(RemoveTrackAction { track_id: id }))
    }
}
