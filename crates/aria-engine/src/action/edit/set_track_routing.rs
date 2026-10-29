use aria_models::{AudioSend, AutomationMode, Param, TrackId, TrackOutput};

use crate::action::{ApplyContext, EditAction};
use crate::error::EngineError;

/// One routing gesture on one lane.
///
/// An enum rather than a patch struct because adding and dropping a send are
/// positional, not a field write: the caller names the gesture, and the action
/// below turns any of them into the same inverse.
#[derive(Debug, Clone)]
pub(crate) enum RouteEdit {
    /// Move the lane's fader, as linear gain.
    Gain(f32),
    /// Move the lane's stereo pan (`-1.0..=1.0`).
    Pan(f32),
    /// Set the lane's automation mode and volume/pan envelopes.
    Automation {
        mode: AutomationMode,
        volume: Param<f32>,
        pan: Param<f32>,
    },
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
    RemoveSend {
        index: usize,
    },
}

/// The undoable unit for a lane's place in the mix (its fader, its output and
/// its sends).
///
/// Every routing gesture snapshots the lane's *whole* previous route, so the
/// inverse is one write through [`aria_models::Timeline::set_lane_route`] and
/// oscillates like `SetTrackFlagsAction`. A send therefore comes back at the
/// index and level it had, which a per-field inverse could not promise once a
/// later send had been dropped.
pub(crate) struct SetTrackRoutingAction {
    track: TrackId,
    gain: f32,
    pan: f32,
    automation: AutomationMode,
    volume: Param<f32>,
    pan_envelope: Param<f32>,
    output: TrackOutput,
    sends: Vec<AudioSend>,
}

pub(crate) fn execute(
    ctx: &mut ApplyContext<'_>,
    track: TrackId,
    edit: RouteEdit,
) -> Result<Box<dyn EditAction>, EngineError> {
    let timeline = ctx.project.timeline_mut();
    let lane = timeline.require_track(track)?;
    // Snapshot the pre-edit route into the inverse (a full restore).
    let inverse = Box::new(SetTrackRoutingAction {
        track,
        gain: lane.gain,
        pan: lane.pan,
        automation: lane.automation,
        volume: lane.volume_envelope.clone(),
        pan_envelope: lane.pan_envelope.clone(),
        output: lane.output,
        sends: lane.sends.to_vec(),
    });
    match edit {
        RouteEdit::Gain(gain) => timeline.set_track_gain(track, gain)?,
        RouteEdit::Pan(pan) => timeline.set_track_pan(track, pan)?,
        RouteEdit::Automation { mode, volume, pan } => {
            timeline.set_track_automation(track, mode, volume, pan)?;
        }
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
        let lane = timeline.require_track(self.track)?;
        let inverse = Box::new(SetTrackRoutingAction {
            track: self.track,
            gain: lane.gain,
            pan: lane.pan,
            automation: lane.automation,
            volume: lane.volume_envelope.clone(),
            pan_envelope: lane.pan_envelope.clone(),
            output: lane.output,
            sends: lane.sends.to_vec(),
        });
        timeline.set_lane_route(self.track, self.gain, self.output, &self.sends)?;
        timeline.set_track_pan(self.track, self.pan)?;
        timeline.set_track_automation(
            self.track,
            self.automation,
            self.volume,
            self.pan_envelope,
        )?;
        Ok(inverse)
    }
}
pub(crate) struct SetMasterMixAction {
    gain: f32,
    pan: f32,
    muted: bool,
    automation: AutomationMode,
    volume: Param<f32>,
    pan_envelope: Param<f32>,
}

pub(crate) fn execute_master(
    ctx: &mut ApplyContext<'_>,
    gain: f32,
    pan: f32,
    muted: bool,
    automation: AutomationMode,
    volume: Param<f32>,
    pan_envelope: Param<f32>,
) -> Result<Box<dyn EditAction>, EngineError> {
    let timeline = ctx.project.timeline_mut();
    let inverse = Box::new(SetMasterMixAction {
        gain: timeline.master_gain(),
        pan: timeline.master_pan(),
        muted: timeline.master_muted(),
        automation: timeline.master_automation(),
        volume: timeline.master_volume_envelope().clone(),
        pan_envelope: timeline.master_pan_envelope().clone(),
    });
    timeline.set_master_mix(gain, pan, muted, automation, volume, pan_envelope)?;
    Ok(inverse)
}

impl EditAction for SetMasterMixAction {
    fn apply(
        self: Box<Self>,
        ctx: &mut ApplyContext<'_>,
    ) -> Result<Box<dyn EditAction>, EngineError> {
        execute_master(
            ctx,
            self.gain,
            self.pan,
            self.muted,
            self.automation,
            self.volume,
            self.pan_envelope,
        )
    }
}
