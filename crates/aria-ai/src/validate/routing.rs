//! Validation + lowering for the mixer: a lane's fader, where its sum goes,
//! and the copies of it other buses take.
//!
//! Every rule here is the model's own. The level bound comes from
//! [`AudioSend::check_level`], and whether a route may be made comes from
//! [`aria_models::Timeline::route_refusal`] — the same call the setter makes,
//! so a route this layer offers cannot be one the engine then refuses. What is
//! added is the sentence: which lanes route, which buses exist, and which send
//! index the model should have used.

use super::*;

use aria_models::{AudioSend, track_kind_specs};

use crate::wire::{AddSend, RemoveSend, SetSend, SetTrackGain, SetTrackOutput};

/// The lane kinds that appear in the mixer, named for a refusal.
///
/// Derived from the lane catalog's own `in_mixer` column, so a lane kind that
/// starts carrying sound is listed here by existing.
fn mixer_lane_ids() -> String {
    let ids: Vec<&str> = track_kind_specs()
        .iter()
        .filter(|spec| spec.in_mixer())
        .map(|spec| spec.id)
        .collect();
    ids.join(", ")
}

/// Refuse a lane with nothing to route: no sound of its own and not a bus.
fn require_mixer_lane(track: &aria_models::Track, raw: u64) -> Result<(), Rejection> {
    if track.kind.spec().in_mixer() {
        return Ok(());
    }
    Err(Rejection::new(format!(
        "track {raw} is a {} lane, which carries no sound; the lanes with a fader are: {}",
        track.kind.spec().id,
        mixer_lane_ids()
    )))
}

/// Check a linear level against the model's own bound.
fn routing_level(value: f64, what: &str) -> Result<f32, Rejection> {
    let level = value as f32;
    AudioSend::check_level(what, level)
        .map(|_| level)
        .map_err(|err| Rejection::new(format!("{err}: 0 silences, 1 is unity, 4 is +12 dB")))
}

/// The bus lanes a route may point at, for a refusal that offers an
/// alternative instead of only a complaint.
fn bus_ids(project: &Project) -> String {
    let buses: Vec<u64> = project
        .timeline()
        .tracks_ordered()
        .filter(|lane| lane.kind.spec().is_bus)
        .map(|lane| lane.id.raw())
        .collect();
    if buses.is_empty() {
        return "none yet — add_track with kind \"submix\" creates one".to_string();
    }
    list_ids(buses)
}

/// Resolve a routing destination: it has to exist, be a bus, and be
/// downstream of nothing that reaches the lane.
fn route_target(
    project: &Project,
    lane: &aria_models::Track,
    raw_track: u64,
    raw_to: u64,
) -> Result<TrackId, Rejection> {
    let dest = track_ref(project, raw_to)?;
    project
        .timeline()
        .route_refusal(lane.id, dest.id)
        .map_err(|err| match err {
            aria_models::ModelError::NotABus(_) => Rejection::new(format!(
                "track {raw_to} is a {} lane, not a submix bus; buses on this timeline: {}",
                dest.kind.spec().id,
                bus_ids(project)
            )),
            aria_models::ModelError::RoutingCycle(_) => Rejection::new(format!(
                "routing track {raw_track} into track {raw_to} would feed it back into \
                 itself, and a loop has no mixdown"
            )),
            other => Rejection::new(other.to_string()),
        })?;
    Ok(dest.id)
}

/// Resolve a send position on a lane, naming what the lane does hold.
fn send_index(lane: &aria_models::Track, index: u32, raw: u64) -> Result<usize, Rejection> {
    let at = index as usize;
    if at < lane.sends.len() {
        return Ok(at);
    }
    Err(Rejection::new(match lane.sends.len() {
        0 => format!("track {raw} has no send {index}; it has no sends — add_send makes one"),
        1 => format!("track {raw} has no send {index}; it has one send, at index 0"),
        held => format!(
            "track {raw} has no send {index}; it has {held} sends, at indices 0 to {}",
            held - 1
        ),
    }))
}

pub(super) fn set_track_gain(
    project: &Project,
    args: &SetTrackGain,
) -> Result<EditCommand, Rejection> {
    let lane = track_ref(project, args.track)?;
    require_mixer_lane(lane, args.track)?;
    Ok(EditCommand::SetTrackGain {
        track: lane.id,
        gain: routing_level(args.gain, "gain")?,
    })
}

pub(super) fn set_track_output(
    project: &Project,
    args: &SetTrackOutput,
) -> Result<EditCommand, Rejection> {
    let lane = track_ref(project, args.track)?;
    require_mixer_lane(lane, args.track)?;
    let to = match args.to {
        Some(raw_to) => Some(route_target(project, lane, args.track, raw_to)?),
        None => None,
    };
    Ok(EditCommand::SetTrackOutput { track: lane.id, to })
}

pub(super) fn add_send(project: &Project, args: &AddSend) -> Result<EditCommand, Rejection> {
    let lane = track_ref(project, args.track)?;
    require_mixer_lane(lane, args.track)?;
    let to = route_target(project, lane, args.track, args.to)?;
    // Same order the setter checks in: the route, then the level, then whether
    // the lane already has this send. A refusal here is the one the engine
    // would have given.
    let level = match args.level {
        Some(level) => routing_level(level, "level")?,
        None => AudioSend::unity(to).level,
    };
    if let Some(held) = lane.sends.iter().position(|send| send.to == to) {
        return Err(Rejection::new(format!(
            "track {} already sends to bus {}; set_send with index {held} changes that \
             send's level instead",
            args.track, args.to
        )));
    }
    Ok(EditCommand::AddSend {
        track: lane.id,
        to,
        level,
        pre_fader: args.pre_fader.unwrap_or(false),
    })
}

pub(super) fn set_send(project: &Project, args: &SetSend) -> Result<EditCommand, Rejection> {
    let lane = track_ref(project, args.track)?;
    let index = send_index(lane, args.index, args.track)?;
    if args.level.is_none() && args.pre_fader.is_none() {
        return Err(Rejection::new(format!(
            "set_send on track {} changes nothing: pass level, pre_fader, or both",
            args.track
        )));
    }
    let level = match args.level {
        Some(level) => Some(routing_level(level, "level")?),
        None => None,
    };
    Ok(EditCommand::SetSend {
        track: lane.id,
        index,
        level,
        pre_fader: args.pre_fader,
    })
}

pub(super) fn remove_send(project: &Project, args: &RemoveSend) -> Result<EditCommand, Rejection> {
    let lane = track_ref(project, args.track)?;
    let index = send_index(lane, args.index, args.track)?;
    Ok(EditCommand::RemoveSend {
        track: lane.id,
        index,
    })
}
