use aria_models::{AudioSend, ModelError, TrackId, TrackOutput};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// One routing gesture on one lane.
///
/// An enum rather than a patch struct because adding and dropping a send are
/// positional, not a field write: the caller names the gesture, and the action
/// below turns any of them into the same inverse.
#[derive(Debug, Clone, Copy)]
pub enum RouteEdit {
    /// Move the lane's fader, as linear gain.
    Gain(f32),
    /// Point the lane's output at a bus, or back at the master.
    Output(TrackOutput),
    /// Add a copy of the lane to a bus at its own level.
    AddSend {
        to: TrackId,
        level: f32,
        pre_fader: bool,
    },
    /// Change an existing send in place. `None` leaves a field as it is.
    SetSend {
        index: usize,
        level: Option<f32>,
        pre_fader: Option<bool>,
    },
    /// Drop a send from the lane.
    RemoveSend { index: usize },
}

/// The undoable unit for a lane's place in the mix (its fader, its output and
/// its sends).
///
/// Every routing gesture snapshots the lane's *whole* previous route, so the
/// inverse is one write through [`aria_models::Timeline::set_lane_route`] and
/// oscillates like `SetTrackFlagsAction`. A send therefore comes back at the
/// index and level it had, which a per-field inverse could not promise once a
/// later send had been dropped.
pub struct SetTrackRoutingAction {
    track: TrackId,
    gain: f32,
    output: TrackOutput,
    sends: Vec<AudioSend>,
}

pub fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    edit: RouteEdit,
) -> Result<Box<dyn EditAction>, EngineError> {
    let timeline = ctx.project.timeline_mut();
    let lane = timeline
        .track(track)
        .ok_or(ModelError::UnknownTrack(track))?;
    // Snapshot the pre-edit route into the inverse (a full restore).
    let inverse = Box::new(SetTrackRoutingAction {
        track,
        gain: lane.gain,
        output: lane.output,
        sends: lane.sends.to_vec(),
    });
    match edit {
        RouteEdit::Gain(gain) => timeline.set_track_gain(track, gain)?,
        RouteEdit::Output(to) => timeline.set_track_output(track, to)?,
        RouteEdit::AddSend {
            to,
            level,
            pre_fader,
        } => {
            timeline.add_send(track, to, level, pre_fader)?;
        }
        RouteEdit::SetSend {
            index,
            level,
            pre_fader,
        } => {
            if let Some(level) = level {
                timeline.set_send_level(track, index, level)?;
            }
            if let Some(pre_fader) = pre_fader {
                timeline.set_send_pre_fader(track, index, pre_fader)?;
            }
        }
        RouteEdit::RemoveSend { index } => {
            timeline.remove_send(track, index)?;
        }
    }
    Ok(inverse)
}

impl EditAction for SetTrackRoutingAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        let timeline = ctx.project.timeline_mut();
        let lane = timeline
            .track(self.track)
            .ok_or(ModelError::UnknownTrack(self.track))?;
        let inverse = Box::new(SetTrackRoutingAction {
            track: self.track,
            gain: lane.gain,
            output: lane.output,
            sends: lane.sends.to_vec(),
        });
        timeline.set_lane_route(self.track, self.gain, self.output, &self.sends)?;
        Ok(inverse)
    }
}
