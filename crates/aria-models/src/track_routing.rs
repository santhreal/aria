//! Where a lane's sound goes.
//!
//! Every audible lane used to sum straight to the master, so a project with
//! twelve music lanes had twelve faders and no way to ride them together, and a
//! reverb had to be spelled out clip by clip. A lane now names its output and
//! may copy itself to a bus at its own level:
//!
//! - [`TrackOutput`] is where the lane's own sum leaves it: the master, or a
//!   bus lane.
//! - [`AudioSend`] is a copy of the lane, at its own level, taken before or
//!   after the lane's fader, and added to a bus.
//! - A bus lane is a lane whose kind row says [`TrackKindSpec::is_bus`]: it
//!   holds no clips of its own and exists to sum what is routed into it.
//!
//! # One graph, resolved once
//!
//! [`Timeline::audio_buses`] is the only place the routing graph is read. It
//! returns the buses in *feed order* — every bus appears before the buses it
//! feeds — so a mixer folds them in one pass with no lookahead, and marks which
//! ones still reach the master so a caller can skip decoding what nothing will
//! hear. The mixer in `aria-render` and the mixer surface in `aria-ui` both
//! read that one answer instead of walking `output`/`sends` themselves.
//!
//! # Cycles
//!
//! A routing cycle has no mixdown, so it is refused where it would be created:
//! [`Timeline::set_track_output`] and [`Timeline::add_send`] walk the graph
//! first and return [`ModelError::RoutingCycle`]. `audio_buses` re-checks,
//! because a project file is a file: a hand-edited cycle is reported rather
//! than silently broken somewhere in the fold.
//!
//! # Sidechain keys
//!
//! An insert on a lane may read another lane's sound as a key: that lane's sum
//! after its own inserts and before its fader. The key lane is mixed first, so
//! a key is an edge of the graph like a send, and [`Timeline::sidechain_refusal`]
//! refuses a key that would close a cycle. A key naming a lane that is gone
//! reads silence and adds no edge, so undoing the removal brings it back.
//!
//! [`TrackKindSpec::is_bus`]: crate::track_catalog::TrackKindSpec::is_bus

use serde::{Deserialize, Serialize};

use crate::audio_effects::{AudioEffectInstance, AudioInsertTarget};
use crate::error::ModelError;
use crate::ids::TrackId;
use crate::param::Param;
use crate::timeline::Timeline;
use crate::track::TrackKind;

#[path = "track_automation.rs"]
mod track_automation;

pub use track_automation::{
    AutomationMode, MAX_TRACK_PAN, MIN_TRACK_PAN, check_track_pan, check_track_pan_envelope,
    check_track_volume_envelope,
};

/// Where a lane's own sum leaves it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TrackOutput {
    /// Straight to the programme mix.
    #[default]
    Master,
    /// Into a bus lane, which carries it on to its own output.
    Bus(TrackId),
}

impl TrackOutput {
    /// The bus this output feeds, if it is not the master.
    pub const fn bus(self) -> Option<TrackId> {
        match self {
            TrackOutput::Master => None,
            TrackOutput::Bus(id) => Some(id),
        }
    }
    /// Whether this is the default (straight to master), so a project that has
    /// never been routed writes nothing back.
    ///
    /// Takes a reference because that is the signature `skip_serializing_if`
    /// requires.
    #[allow(clippy::trivially_copy_pass_by_ref)]
    pub const fn is_master(&self) -> bool {
        matches!(self, TrackOutput::Master)
    }
}

/// A copy of one lane's sound, added to a bus at its own level.
///
/// `pre_fader` decides which copy is taken: before the lane's own fader (so
/// riding the lane down does not thin the send — how a reverb is fed), or after
/// it (so the send follows the lane — how a parallel compressor is fed).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AudioSend {
    /// The bus lane this send feeds.
    pub to: TrackId,
    /// Linear gain applied to the copy.
    pub level: f32,
    /// Take the copy before the lane's own fader.
    #[serde(default)]
    pub pre_fader: bool,
}

impl AudioSend {
    /// Largest send level accepted, as linear gain: `+12 dB`. A send is a
    /// balance against the lane, not a place to make up a missing fader.
    pub const MAX_LEVEL: f32 = 4.0;

    /// Refuse a level outside silence through `+12 dB`, naming what it was.
    ///
    /// The one owner of that bound: a fader, a send, and any surface that
    /// checks a level before offering it asks here, so a level accepted by a
    /// caller and refused by the setter cannot happen.
    pub fn check_level(what: &str, level: f32) -> Result<f32, ModelError> {
        if !level.is_finite() || level < 0.0 || level > Self::MAX_LEVEL {
            return Err(ModelError::InvalidParam(format!(
                "{what} {level} is outside 0..={}",
                Self::MAX_LEVEL
            )));
        }
        Ok(level)
    }

    /// A post-fader send at unity.
    pub fn unity(to: TrackId) -> Self {
        Self {
            to,
            level: 1.0,
            pre_fader: false,
        }
    }
}

/// One lane's routing, lifted off it so a removal can put it back.
///
/// Returned by [`Timeline::remove_track`] as part of what it removed: deleting
/// a bus changes the lanes that fed it, and an undo has to restore those too.
#[derive(Debug, Clone, PartialEq)]
pub struct LaneRoute {
    /// The lane the routing came off.
    pub track: TrackId,
    /// Where its own sum went.
    pub output: TrackOutput,
    /// The sends it held, including the ones that pointed elsewhere.
    pub sends: Vec<AudioSend>,
}

/// One node of the mix graph: a lane's sum, its fader, and where both go.
///
/// Borrowed from the timeline: the sends are read where they live rather than
/// copied out, so building the graph for a block of audio allocates one vector
/// of these and nothing else.
#[derive(Debug, Clone, Copy)]
pub struct AudioBus<'a> {
    /// The lane this node sums.
    pub track: TrackId,
    /// Whether the lane holds no clips and exists to sum what is routed in.
    pub is_bus: bool,
    /// The lane's fader, as linear gain.
    pub gain: f32,
    /// The lane's stereo pan (`-1.0..=1.0`).
    pub pan: f32,
    /// How this lane handles its volume and pan automation curves.
    pub automation: AutomationMode,
    /// The lane's sequence-tick volume automation curve.
    pub volume_envelope: &'a Param<f32>,
    /// The lane's sequence-tick pan automation curve.
    pub pan_envelope: &'a Param<f32>,
    /// Where the lane's own sum goes.
    pub output: TrackOutput,
    /// Copies of the lane taken for other buses.
    pub sends: &'a [AudioSend],
    /// The lane's insert chain, run over its sum before its fader and its
    /// sends.
    pub inserts: &'a [AudioEffectInstance],
    /// Whether the lane itself is heard: unmuted, and not dropped by another
    /// lane of its kind being soloed.
    pub audible: bool,
    /// Whether anything this lane produces still arrives at the master: through
    /// its output, through a send, or as the key of an insert on a lane that
    /// does or on the master. A lane that fails this is not worth decoding.
    pub reaches_master: bool,
}

impl Timeline {
    /// Whether this lane takes part in the mix at all.
    ///
    /// The answer is [`TrackKindSpec::in_mixer`], read from the lane's own kind
    /// row, so a new lane kind decides it by filling its row in.
    ///
    /// [`TrackKindSpec::in_mixer`]: crate::track_catalog::TrackKindSpec::in_mixer
    pub(crate) fn lane_mixes_audio(&self, track: TrackId) -> Result<bool, ModelError> {
        let track = self.track(track).ok_or(ModelError::UnknownTrack(track))?;
        Ok(track.kind.spec().in_mixer())
    }

    /// Point a lane's output at the master or at a bus lane.
    ///
    /// Refuses a lane that carries no sound, a destination that is not a bus,
    /// a destination outside this timeline, and any route that would close a
    /// cycle.
    pub fn set_track_output(
        &mut self,
        track: TrackId,
        output: TrackOutput,
    ) -> Result<(), ModelError> {
        if !self.lane_mixes_audio(track)? {
            return Err(ModelError::LaneCarriesNoSound(track));
        }
        if let Some(to) = output.bus() {
            self.route_refusal(track, to)?;
        }
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        lane.output = output;
        Ok(())
    }

    /// Add a send from `track` to the bus lane `to`, at `level` linear gain.
    ///
    /// Returns the index of the new send, which is how it is addressed
    /// afterwards. A second send to the same bus is refused: two copies of one
    /// lane on one bus is a level, not a route.
    pub fn add_send(
        &mut self,
        track: TrackId,
        to: TrackId,
        level: f32,
        pre_fader: bool,
    ) -> Result<usize, ModelError> {
        if !self.lane_mixes_audio(track)? {
            return Err(ModelError::LaneCarriesNoSound(track));
        }
        self.route_refusal(track, to)?;
        let level = check_send_level(level)?;
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        if lane.sends.iter().any(|send| send.to == to) {
            return Err(ModelError::DuplicateSend { track, to });
        }
        lane.sends.push(AudioSend {
            to,
            level,
            pre_fader,
        });
        Ok(lane.sends.len() - 1)
    }

    /// Set an existing send's level.
    pub fn set_send_level(
        &mut self,
        track: TrackId,
        index: usize,
        level: f32,
    ) -> Result<(), ModelError> {
        let level = check_send_level(level)?;
        let send = self.send_mut(track, index)?;
        send.level = level;
        Ok(())
    }

    /// Set whether an existing send is taken before the lane's fader.
    pub fn set_send_pre_fader(
        &mut self,
        track: TrackId,
        index: usize,
        pre_fader: bool,
    ) -> Result<(), ModelError> {
        let send = self.send_mut(track, index)?;
        send.pre_fader = pre_fader;
        Ok(())
    }

    /// Drop a send, returning what it was so an undo can put it back at the
    /// same index.
    pub fn remove_send(&mut self, track: TrackId, index: usize) -> Result<AudioSend, ModelError> {
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        if index >= lane.sends.len() {
            return Err(ModelError::UnknownSend { track, index });
        }
        Ok(lane.sends.remove(index))
    }

    /// Put a send back at `index`, for the inverse of [`Timeline::remove_send`].
    pub fn insert_send(
        &mut self,
        track: TrackId,
        index: usize,
        send: AudioSend,
    ) -> Result<(), ModelError> {
        self.route_refusal(track, send.to)?;
        check_send_level(send.level)?;
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        if index > lane.sends.len() {
            return Err(ModelError::UnknownSend { track, index });
        }
        if lane.sends.iter().any(|other| other.to == send.to) {
            return Err(ModelError::DuplicateSend { track, to: send.to });
        }
        lane.sends.insert(index, send);
        Ok(())
    }

    /// Set a lane's fader, as linear gain.
    ///
    /// Accepts silence through `+12 dB`; anything outside that, and anything
    /// that is not a number, is refused rather than clamped, because a fader
    /// that quietly disagrees with the number beside it is worse than an error.
    pub fn set_track_gain(&mut self, track: TrackId, gain: f32) -> Result<(), ModelError> {
        if !self.lane_mixes_audio(track)? {
            return Err(ModelError::LaneCarriesNoSound(track));
        }
        AudioSend::check_level("lane gain", gain)?;
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        lane.gain = gain;
        Ok(())
    }

    /// Write a lane's whole routing at once: its fader, its output and its
    /// sends, in the order given.
    ///
    /// What an undo applies. Every part goes through the same checks a single
    /// gesture does — a route it would have refused is refused here too — and a
    /// refusal leaves the lane exactly as it was rather than half written.
    pub fn set_lane_route(
        &mut self,
        track: TrackId,
        gain: f32,
        output: TrackOutput,
        sends: &[AudioSend],
    ) -> Result<(), ModelError> {
        let (held_gain, held) = {
            let lane = self
                .track_mut(track)
                .ok_or(ModelError::UnknownTrack(track))?;
            (
                lane.gain,
                LaneRoute {
                    track,
                    output: std::mem::replace(&mut lane.output, TrackOutput::Master),
                    sends: std::mem::take(&mut lane.sends),
                },
            )
        };
        let mut write = || -> Result<(), ModelError> {
            self.set_track_gain(track, gain)?;
            self.set_track_output(track, output)?;
            for send in sends {
                self.add_send(track, send.to, send.level, send.pre_fader)?;
            }
            Ok(())
        };
        match write() {
            Ok(()) => Ok(()),
            Err(err) => {
                if let Some(lane) = self.track_mut(track) {
                    lane.gain = held_gain;
                }
                self.restore_routes(vec![held]);
                Err(err)
            }
        }
    }

    /// Every send a bus receives, as `(source lane, send)` pairs in lane order.
    ///
    /// What the mixer surface lists under a bus, and what a delete has to warn
    /// about: removing a bus lane silences the lanes feeding it.
    pub fn sends_into(&self, bus: TrackId) -> Vec<(TrackId, AudioSend)> {
        let mut found = Vec::new();
        for lane in self.tracks_ordered() {
            for send in &lane.sends {
                if send.to == bus {
                    found.push((lane.id, *send));
                }
            }
        }
        found
    }

    /// Every lane whose output feeds `bus`, in lane order.
    pub fn outputs_into(&self, bus: TrackId) -> Vec<TrackId> {
        self.tracks_ordered()
            .filter(|lane| lane.output.bus() == Some(bus))
            .map(|lane| lane.id)
            .collect()
    }

    /// Take every route that points at `bus` off the lanes holding it, and
    /// return what was taken so the same removal can be undone.
    ///
    /// Deleting a bus returns the lanes that fed it to the master rather than
    /// silencing them, and rather than leaving a route pointing at a lane that
    /// is gone. Called by [`Timeline::remove_track`], which is what hands the
    /// snapshot to the caller.
    pub(crate) fn detach_routes_to(&mut self, bus: TrackId) -> Vec<LaneRoute> {
        let holders: Vec<TrackId> = self
            .tracks_ordered()
            .filter(|lane| {
                lane.output.bus() == Some(bus) || lane.sends.iter().any(|send| send.to == bus)
            })
            .map(|lane| lane.id)
            .collect();
        let mut detached = Vec::with_capacity(holders.len());
        for id in holders {
            let Some(lane) = self.track_mut(id) else {
                continue;
            };
            let sends = std::mem::take(&mut lane.sends);
            let output = lane.output;
            lane.sends
                .extend(sends.iter().filter(|send| send.to != bus).copied());
            if output.bus() == Some(bus) {
                lane.output = TrackOutput::Master;
            }
            detached.push(LaneRoute {
                track: id,
                output,
                sends,
            });
        }
        detached
    }

    /// Put routes back exactly as [`Timeline::detach_routes_to`] found them.
    ///
    /// A lane that is gone is skipped: the snapshot describes what a removal
    /// took, and a lane removed since is not this call's business.
    pub(crate) fn restore_routes(&mut self, routes: Vec<LaneRoute>) {
        for route in routes {
            let Some(lane) = self.track_mut(route.track) else {
                continue;
            };
            lane.output = route.output;
            lane.sends = route.sends;
        }
    }

    /// The mix graph, in feed order: every bus appears before the buses it
    /// feeds.
    ///
    /// The one reader of `output` and `sends`. A mixer folds this front to back
    /// and needs no lookahead; `reaches_master` is filled in by walking it back
    /// to front, so a lane behind a muted bus is known to be inaudible before
    /// anything is decoded.
    pub fn audio_buses(&self) -> Result<Vec<AudioBus<'_>>, ModelError> {
        let mut nodes: Vec<AudioBus<'_>> = Vec::new();
        for lane in self.tracks_ordered() {
            if !lane.kind.spec().in_mixer() {
                continue;
            }
            nodes.push(AudioBus {
                track: lane.id,
                is_bus: lane.kind.spec().is_bus,
                gain: lane.gain,
                pan: lane.pan,
                automation: lane.automation,
                volume_envelope: &lane.volume_envelope,
                pan_envelope: &lane.pan_envelope,
                output: lane.output,
                sends: lane.sends.as_slice(),
                inserts: lane.audio_effects.as_slice(),
                audible: self.track_is_audible(lane),
                reaches_master: false,
            });
        }

        // Every route has to name a bus that is here. The setters refuse
        // anything else, so this only fires on a project file that was edited
        // by hand — and it fires by name instead of quietly dropping the lane's
        // sound somewhere in the fold.
        for node in &nodes {
            for target in node.targets() {
                match self.track(target) {
                    None => return Err(ModelError::UnknownTrack(target)),
                    Some(lane) if !lane.kind.spec().is_bus => {
                        return Err(ModelError::NotABus(target));
                    }
                    Some(_) => {}
                }
            }
        }

        let ordered = feed_order(&nodes)?;
        let mut buses: Vec<AudioBus<'_>> = ordered.into_iter().map(|i| nodes[i]).collect();

        // Back to front: a bus is known before anything that feeds it, and a
        // lane before any lane it keys, so one reverse pass answers "does this
        // still arrive at the master".
        let master_keys = |id: TrackId| {
            self.master_audio_effects()
                .iter()
                .any(|fx| !fx.bypass && fx.sidechain == Some(id))
        };
        let any_bus_solo = self.has_solo(TrackKind::Submix);
        for i in (0..buses.len()).rev() {
            let reaches =
                |target: Option<TrackId>, buses: &[AudioBus<'_>], from_bus_or_solo: bool| {
                    match target {
                        None => !any_bus_solo || from_bus_or_solo,
                        Some(id) => buses
                            .iter()
                            .skip(i + 1)
                            .find(|bus| bus.track == id)
                            .is_some_and(|bus| bus.reaches_master),
                    }
                };
            let id = buses[i].track;
            let from_bus_or_solo = buses[i].is_bus || self.track(id).is_some_and(|lane| lane.solo);
            let out = reaches(buses[i].output.bus(), &buses, from_bus_or_solo);
            let sent = buses[i]
                .sends
                .iter()
                .any(|send| reaches(Some(send.to), &buses, from_bus_or_solo));
            let keyed = master_keys(id)
                || buses[i + 1..].iter().any(|bus| {
                    bus.reaches_master
                        && bus
                            .inserts
                            .iter()
                            .any(|fx| !fx.bypass && fx.sidechain == Some(id))
                });
            buses[i].reaches_master = buses[i].audible && (out || sent || keyed);
        }
        Ok(buses)
    }

    fn send_mut(&mut self, track: TrackId, index: usize) -> Result<&mut AudioSend, ModelError> {
        let lane = self
            .track_mut(track)
            .ok_or(ModelError::UnknownTrack(track))?;
        lane.sends
            .get_mut(index)
            .ok_or(ModelError::UnknownSend { track, index })
    }

    /// Why routing `track` into `to` would be refused, if it would be.
    ///
    /// Every gate a route passes, in the order a setter applies them: the
    /// destination exists, is a bus, is not the lane itself, and is not
    /// downstream of it. Public because a caller that offers a route — the
    /// mixer's menu, the assistant's tool validation — has to answer the same
    /// question before the setter does, and a second copy of the walk is how
    /// the two come to disagree.
    pub fn route_refusal(&self, track: TrackId, to: TrackId) -> Result<(), ModelError> {
        self.check_bus_destination(track, to)?;
        self.check_no_cycle(track, to)
    }

    /// Why keying an insert at `holder` from lane `key` would be refused, if it
    /// would be: the key lane exists and carries sound, a clip insert takes no
    /// key, and a lane is not keyed by itself or by a lane it feeds. The master
    /// may be keyed by any lane that carries sound.
    pub fn sidechain_refusal(
        &self,
        key: TrackId,
        holder: AudioInsertTarget,
    ) -> Result<(), ModelError> {
        if !self.lane_mixes_audio(key)? {
            return Err(ModelError::LaneCarriesNoSound(key));
        }
        match holder {
            AudioInsertTarget::Master => Ok(()),
            AudioInsertTarget::Track(lane) if lane == key => Err(ModelError::RoutingCycle(key)),
            AudioInsertTarget::Track(lane) => self.check_no_cycle(key, lane),
            AudioInsertTarget::Clip(_) => Err(ModelError::InvalidParam(
                "a clip insert takes no sidechain key; key an insert on its lane".into(),
            )),
        }
    }

    /// A destination has to exist here, be a bus lane, and not be the lane
    /// itself.
    fn check_bus_destination(&self, track: TrackId, to: TrackId) -> Result<(), ModelError> {
        if to == track {
            return Err(ModelError::RoutingCycle(track));
        }
        let dest = self.track(to).ok_or(ModelError::UnknownTrack(to))?;
        if !dest.kind.spec().is_bus {
            return Err(ModelError::NotABus(to));
        }
        Ok(())
    }

    /// Whether routing `track` into `to` would close a cycle: walk out of `to`
    /// along outputs, sends and sidechain keys and see whether `track` is
    /// reached.
    fn check_no_cycle(&self, track: TrackId, to: TrackId) -> Result<(), ModelError> {
        let mut seen: Vec<TrackId> = Vec::new();
        let mut frontier = vec![to];
        while let Some(id) = frontier.pop() {
            if id == track {
                return Err(ModelError::RoutingCycle(track));
            }
            if seen.contains(&id) {
                continue;
            }
            seen.push(id);
            let Some(lane) = self.track(id) else {
                continue;
            };
            if let Some(next) = lane.output.bus() {
                frontier.push(next);
            }
            frontier.extend(lane.sends.iter().map(|send| send.to));
            frontier.extend(
                self.tracks_ordered()
                    .filter(|holder| {
                        holder
                            .audio_effects
                            .iter()
                            .any(|fx| fx.sidechain == Some(id))
                    })
                    .map(|holder| holder.id),
            );
        }
        Ok(())
    }
}

fn check_send_level(level: f32) -> Result<f32, ModelError> {
    AudioSend::check_level("send level", level)
}

/// Indices of `nodes` in feed order, or the lane a cycle runs through.
///
/// Kahn's algorithm over the graph "lane → everything it feeds, and every lane
/// whose inserts it keys". A project file can hold a cycle the setters would
/// have refused, so this is the second place the check lives, and the one that
/// keeps a mixdown from being defined by whichever edge happened to be folded
/// first.
///
/// The ready set is a queue rather than a stack, so lanes that feed nothing in
/// common keep the order they sit in on the timeline. A mixer draws its strips
/// in this order, and a stack would draw them upside down.
fn feed_order(nodes: &[AudioBus<'_>]) -> Result<Vec<usize>, ModelError> {
    let index_of = |id: TrackId| nodes.iter().position(|node| node.track == id);
    // `(from, to)`: `from` is mixed before `to`.
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        edges.extend(node.targets().filter_map(index_of).map(|to| (i, to)));
        edges.extend(node.keys().filter_map(index_of).map(|from| (from, i)));
    }
    let mut fed_by = vec![0usize; nodes.len()];
    for &(_, to) in &edges {
        fed_by[to] += 1;
    }
    let mut ready: std::collections::VecDeque<usize> =
        (0..nodes.len()).filter(|i| fed_by[*i] == 0).collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(i) = ready.pop_front() {
        order.push(i);
        for &(from, to) in &edges {
            if from == i {
                fed_by[to] -= 1;
                if fed_by[to] == 0 {
                    ready.push_back(to);
                }
            }
        }
    }
    if order.len() != nodes.len() {
        // Whatever Kahn could not place is inside the cycle. There is at least
        // one such lane here, so the first is reported; the fallback keeps this
        // total rather than trusting that.
        let stuck = (0..nodes.len())
            .find(|i| !order.contains(i))
            .map_or(nodes[0].track, |i| nodes[i].track);
        return Err(ModelError::RoutingCycle(stuck));
    }
    Ok(order)
}

impl AudioBus<'_> {
    /// Every bus this lane feeds: its output, and each send's destination.
    fn targets(&self) -> impl Iterator<Item = TrackId> + '_ {
        self.output
            .bus()
            .into_iter()
            .chain(self.sends.iter().map(|send| send.to))
    }

    /// Every lane keying an insert on this lane, in insert order.
    pub fn keys(&self) -> impl Iterator<Item = TrackId> + '_ {
        self.inserts.iter().filter_map(|fx| fx.sidechain)
    }
}

#[cfg(test)]
#[path = "track_routing_tests.rs"]
mod submix;
