//! The mixer: what every audio lane's place in the mix reads as, and the
//! gestures that change it.
//!
//! The model owns the graph ([`aria_models::Timeline::audio_buses`]); this file
//! is the view of it. One read builds every strip in feed order, so the panel
//! draws sources above the buses they feed without sorting anything itself, and
//! a lane that cannot reach the master is drawn as such rather than silently
//! missing from the mixdown.
//!
//! A fader is a draft while the pointer is down. The drag lives here, not in the
//! engine, so riding a fader from silence to unity costs one undo entry — the
//! same rule the speed ramp and the curve editor follow.

use super::audio_inserts;
use super::*;
use crate::types::{UiInsert, UiKeyChoice};
use aria_models::{AudioInsertTarget, AudioSend, AutomationMode, Param, TrackOutput};

#[path = "mixer_automation.rs"]
mod mixer_automation;
pub use mixer_automation::{AutomationPassState, frac_to_pan, pan_label, pan_to_frac};

/// The bottom of the fader's travel, in decibels. Below this a fader reads as
/// silence: a mixer that offered -80 dB would be offering arithmetic, not a
/// mix.
pub const FADER_FLOOR_DB: f32 = -60.0;

/// The top of the fader's travel, in decibels: whatever the model's own send
/// ceiling is, read from it, so the fader and the model refuse at exactly the
/// same place.
pub fn fader_ceiling_db() -> f32 {
    aria_analysis::db_from_linear(AudioSend::MAX_LEVEL).unwrap_or(0.0)
}

/// How close to unity a drag has to land before it snaps there, in decibels.
///
/// Unity is the value a fader returns to most often, and hitting it on a 120 px fader
/// otherwise takes a pixel. The detent is the only place the pointer is not
/// obeyed exactly, and it is announced by the readout snapping with it.
const UNITY_DETENT_DB: f32 = 0.75;

/// What a fader drag is moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaderTarget {
    /// A lane's own fader.
    Lane(TrackId),
    /// A lane's stereo pan knob (`-1.0..=1.0`).
    LanePan(TrackId),
    /// The level of one send on a lane, addressed by its index.
    Send { track: TrackId, index: usize },
    /// The master strip's fader.
    Master,
    /// The master strip's stereo pan knob (`-1.0..=1.0`).
    MasterPan,
}

/// A fader being ridden.
///
/// The grab point and the fader's travel are held with it, so the value moves
/// by how far the pointer has come rather than jumping to where it landed: a
/// press on a fader at unity does not first slam it to the bottom of the strip.
#[derive(Debug, Clone, Copy)]
pub struct FaderDrag {
    pub target: FaderTarget,
    pub gain: f32,
    /// Window y the fader was grabbed at.
    grabbed_at: f32,
    /// Where on the travel it was grabbed, `0.0` at the floor.
    grabbed_frac: f32,
    /// How many pixels the whole travel is.
    travel: f32,
}

/// One send, as the mixer draws it.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerSend {
    pub index: usize,
    pub to: u64,
    pub to_name: String,
    /// The stored level. A drag is drafted over it by
    /// [`AppState::fader_value`], so this is the value an undo returns to.
    pub level: f32,
    pub pre_fader: bool,
}

/// One channel strip.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerStrip {
    pub track: u64,
    pub name: String,
    pub is_bus: bool,
    /// The stored fader value; see [`MixerSend::level`].
    pub gain: f32,
    pub pan: f32,
    pub automation: AutomationMode,
    pub volume_envelope: Param<f32>,
    pub pan_envelope: Param<f32>,
    /// `None` is the master.
    pub output: Option<u64>,
    pub output_name: String,
    pub sends: Vec<MixerSend>,
    /// Whether the lane itself is heard (not muted, not soloed away).
    pub audible: bool,
    /// Whether anything this lane produces arrives at the master.
    pub reaches_master: bool,
    pub muted: bool,
    pub solo: bool,
    /// The buses this lane could be sent to, and where it already is.
    pub send_targets: Vec<SendTarget>,
    /// The outputs this lane could have, the master first.
    pub output_targets: Vec<OutputTarget>,
    /// The lane's insert chain, in signal order.
    pub inserts: Vec<UiInsert>,
    /// The lanes that may key an insert on this strip.
    pub keys: Vec<UiKeyChoice>,
}

/// Where a send on this strip could go, and what a click on it does.
#[derive(Debug, Clone, PartialEq)]
pub struct SendTarget {
    pub to: u64,
    pub name: String,
    /// The send's index on this lane, when one already exists.
    pub index: Option<usize>,
    pub pre_fader: bool,
}

/// Where a strip's output could point.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputTarget {
    /// `None` is the master.
    pub to: Option<u64>,
    pub name: String,
    /// Whether the lane points here now.
    pub current: bool,
}

/// The mix, built once per change from the same engine read that builds the
/// store.
///
/// A strip used to be read by locking the session while the panel drew, which
/// parked the frame behind whatever the preview was decoding. Every value the
/// mixer draws is here instead, including each strip's destination lists, so the
/// panel takes no lock and a drawn strip cannot disagree with the timeline
/// beside it. `Err` when the routing has a cycle: a hand-edited file can hold
/// one, and the mixer says so rather than guessing a fold order.
#[derive(Debug, Clone, PartialEq)]
pub struct MixerFacts {
    /// Behind an `Arc` so a panel can take the whole mix for the length of a
    /// frame with one refcount bump, rather than copying every strip's strings
    /// per repaint to get out of the borrow that drawing needs.
    pub strips: Result<std::sync::Arc<[MixerStrip]>, String>,
    /// The master's insert chain, and the lanes that may key it.
    pub master: std::sync::Arc<MasterChain>,
}

/// The master strip's inserts.
#[derive(Debug, Clone, PartialEq)]
pub struct MasterChain {
    pub gain: f32,
    pub pan: f32,
    pub muted: bool,
    pub automation: AutomationMode,
    pub volume_envelope: Param<f32>,
    pub pan_envelope: Param<f32>,
    pub inserts: Vec<UiInsert>,
    pub keys: Vec<UiKeyChoice>,
}

impl Default for MasterChain {
    fn default() -> Self {
        Self {
            gain: 1.0,
            pan: 0.0,
            muted: false,
            automation: AutomationMode::Off,
            volume_envelope: Param::Constant(1.0),
            pan_envelope: Param::Constant(0.0),
            inserts: Vec::new(),
            keys: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct StripBasis {
    id: TrackId,
    kind: aria_models::TrackKind,
    name: String,
    muted: bool,
    solo: bool,
    gain: u32,
    pan: u32,
    automation: AutomationMode,
    volume_envelope: Param<f32>,
    pan_envelope: Param<f32>,
    output: TrackOutput,
    sends: Vec<AudioSend>,
    audio_effects: Vec<aria_models::AudioEffectInstance>,
}

impl StripBasis {
    fn of(track: &aria_models::Track) -> Self {
        Self {
            id: track.id,
            kind: track.kind,
            name: track.name.clone(),
            muted: track.muted,
            solo: track.solo,
            gain: track.gain.to_bits(),
            pan: track.pan.to_bits(),
            automation: track.automation,
            volume_envelope: track.volume_envelope.clone(),
            pan_envelope: track.pan_envelope.clone(),
            output: track.output,
            sends: track.sends.clone(),
            audio_effects: track.audio_effects.clone(),
        }
    }

    fn matches(&self, track: &aria_models::Track) -> bool {
        self.id == track.id
            && self.kind == track.kind
            && self.name == track.name
            && self.muted == track.muted
            && self.solo == track.solo
            && self.gain == track.gain.to_bits()
            && self.pan == track.pan.to_bits()
            && self.automation == track.automation
            && self.volume_envelope == track.volume_envelope
            && self.pan_envelope == track.pan_envelope
            && self.output == track.output
            && self.sends == track.sends
            && self.audio_effects == track.audio_effects
    }
}

/// Sequence and audio effect catalog state covered by [`MixerFacts`].
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MixerBasis {
    catalog_revision: u64,
    master_gain: u32,
    master_pan: u32,
    master_muted: bool,
    master_automation: AutomationMode,
    master_volume_envelope: Param<f32>,
    master_pan_envelope: Param<f32>,
    master_audio_effects: Vec<aria_models::AudioEffectInstance>,
    tracks: Vec<StripBasis>,
}

impl MixerBasis {
    pub(crate) fn of(timeline: &aria_models::Timeline) -> Self {
        Self {
            catalog_revision: aria_models::audio_effect_catalog().revision(),
            master_gain: timeline.master_gain().to_bits(),
            master_pan: timeline.master_pan().to_bits(),
            master_muted: timeline.master_muted(),
            master_automation: timeline.master_automation(),
            master_volume_envelope: timeline.master_volume_envelope().clone(),
            master_pan_envelope: timeline.master_pan_envelope().clone(),
            master_audio_effects: timeline.master_audio_effects().to_vec(),
            tracks: timeline.tracks_ordered().map(StripBasis::of).collect(),
        }
    }

    pub(crate) fn matches(&self, timeline: &aria_models::Timeline) -> bool {
        self.catalog_revision == aria_models::audio_effect_catalog().revision()
            && self.master_gain == timeline.master_gain().to_bits()
            && self.master_pan == timeline.master_pan().to_bits()
            && self.master_muted == timeline.master_muted()
            && self.master_automation == timeline.master_automation()
            && self.master_volume_envelope == *timeline.master_volume_envelope()
            && self.master_pan_envelope == *timeline.master_pan_envelope()
            && self.master_audio_effects.as_slice() == timeline.master_audio_effects()
            && self.tracks.len() == timeline.track_count()
            && self
                .tracks
                .iter()
                .zip(timeline.tracks_ordered())
                .all(|(basis, track)| basis.matches(track))
    }
}

impl MixerFacts {
    pub fn of(engine: &aria_engine::Engine) -> Self {
        Self::of_timeline(engine.project().timeline())
    }

    pub(crate) fn of_timeline(timeline: &aria_models::Timeline) -> Self {
        let master = std::sync::Arc::new(MasterChain {
            gain: timeline.master_gain(),
            pan: timeline.master_pan(),
            muted: timeline.master_muted(),
            automation: timeline.master_automation(),
            volume_envelope: timeline.master_volume_envelope().clone(),
            pan_envelope: timeline.master_pan_envelope().clone(),
            inserts: audio_inserts::rows(timeline.master_audio_effects()),
            keys: audio_inserts::key_choices(timeline, AudioInsertTarget::Master),
        });
        let buses = match timeline.audio_buses() {
            Ok(buses) => buses,
            Err(err) => {
                return Self {
                    strips: Err(format!("this project's audio routing has no mix: {err}")),
                    master,
                };
            }
        };
        let name_of = |id: TrackId| {
            timeline
                .track(id)
                .map_or_else(|| "(missing lane)".to_string(), |t| t.name.to_string())
        };
        let strips = buses
            .iter()
            .map(|bus| {
                let track = timeline.track(bus.track);
                let sends: &[AudioSend] = track.map_or(&[], |lane| lane.sends.as_slice());
                let chain = track.map_or(&[][..], |lane| lane.audio_effects.as_slice());
                MixerStrip {
                    track: bus.track.raw(),
                    name: name_of(bus.track),
                    is_bus: bus.is_bus,
                    gain: bus.gain,
                    pan: bus.pan,
                    automation: bus.automation,
                    volume_envelope: bus.volume_envelope.clone(),
                    pan_envelope: bus.pan_envelope.clone(),
                    output: bus.output.bus().map(TrackId::raw),
                    output_name: bus
                        .output
                        .bus()
                        .map_or_else(|| "Master".to_string(), name_of),
                    sends: bus
                        .sends
                        .iter()
                        .enumerate()
                        .map(|(index, send)| MixerSend {
                            index,
                            to: send.to.raw(),
                            to_name: name_of(send.to),
                            level: send.level,
                            pre_fader: send.pre_fader,
                        })
                        .collect(),
                    audible: bus.audible,
                    reaches_master: bus.reaches_master,
                    muted: track.is_some_and(|t| t.muted),
                    solo: track.is_some_and(|t| t.solo),
                    // Built from the mixer's own lanes rather than a list the
                    // panel keeps, so a bus added to the project shows up as a
                    // target with no edit here. A lane never lists itself.
                    send_targets: timeline
                        .tracks_ordered()
                        .filter(|other| other.kind.is_bus() && other.id != bus.track)
                        .map(|other| {
                            let held = sends.iter().position(|send| send.to == other.id);
                            SendTarget {
                                to: other.id.raw(),
                                name: other.name.to_string(),
                                index: held,
                                pre_fader: held.is_some_and(|at| sends[at].pre_fader),
                            }
                        })
                        .collect(),
                    output_targets: std::iter::once(OutputTarget {
                        to: None,
                        name: "Master".to_string(),
                        current: bus.output == TrackOutput::Master,
                    })
                    .chain(
                        timeline
                            .tracks_ordered()
                            .filter(|other| other.kind.is_bus() && other.id != bus.track)
                            .map(|other| OutputTarget {
                                to: Some(other.id.raw()),
                                name: other.name.to_string(),
                                current: bus.output == TrackOutput::Bus(other.id),
                            }),
                    )
                    .collect(),
                    inserts: audio_inserts::rows(chain),
                    keys: audio_inserts::key_choices(timeline, AudioInsertTarget::Track(bus.track)),
                }
            })
            .collect();
        Self {
            strips: Ok(strips),
            master,
        }
    }
}

/// Linear gain as decibels, floored at the bottom of the fader's travel so
/// silence has a position rather than an infinity.
///
/// The conversion itself belongs to `aria_analysis`; this only decides what the
/// bottom of a fader means.
pub fn gain_to_db(gain: f32) -> f32 {
    aria_analysis::db_from_linear(gain).map_or(FADER_FLOOR_DB, |db| db.max(FADER_FLOOR_DB))
}

/// Decibels as linear gain. The floor is exactly silence, so a fader pulled to
/// the bottom writes `0.0` and not a level that is merely quiet.
pub fn db_to_gain(db: f32) -> f32 {
    if db <= FADER_FLOOR_DB {
        return 0.0;
    }
    aria_analysis::linear_from_db(db.min(fader_ceiling_db()))
}

/// Where a gain sits on the fader's travel, `0.0` at the floor and `1.0` at the
/// ceiling.
pub fn gain_to_frac(gain: f32) -> f32 {
    let db = gain_to_db(gain);
    ((db - FADER_FLOOR_DB) / (fader_ceiling_db() - FADER_FLOOR_DB)).clamp(0.0, 1.0)
}

/// The gain a position on the fader means, with the unity detent applied.
pub fn frac_to_gain(frac: f32) -> f32 {
    if !frac.is_finite() {
        return 1.0;
    }
    let db = FADER_FLOOR_DB + frac.clamp(0.0, 1.0) * (fader_ceiling_db() - FADER_FLOOR_DB);
    if db.abs() <= UNITY_DETENT_DB {
        return 1.0;
    }
    db_to_gain(db)
}

/// What a fader reads beside it. Silence is named, not printed as -60.0.
pub fn gain_label(gain: f32) -> String {
    if gain <= 0.0 {
        return "-inf".to_string();
    }
    let db = gain_to_db(gain);
    if db.abs() < 0.05 {
        return "0.0 dB".to_string();
    }
    format!("{db:+.1} dB")
}

impl AppState {
    /// The master's insert chain and the lanes that may key it, as projected.
    pub fn master_chain(&self) -> std::sync::Arc<MasterChain> {
        std::sync::Arc::clone(&self.facts.mixer.master)
    }

    /// Every strip, in the order the mix folds them: a lane before the bus it
    /// feeds. `Err` when the project's routing has a cycle — a hand-edited file
    /// can hold one, and the mixer says so rather than drawing a graph it would
    /// have to guess the fold order of.
    ///
    /// Read from the projection, so a frame drawing the mixer takes no lock.
    pub fn mixer_strips(&self) -> Result<std::sync::Arc<[MixerStrip]>, &str> {
        match &self.facts.mixer.strips {
            Ok(strips) => Ok(std::sync::Arc::clone(strips)),
            Err(why) => Err(why.as_str()),
        }
    }

    /// The projected strip for `track`, when it is in the mix.
    fn strip_of(&self, track: TrackId) -> Option<&MixerStrip> {
        self.facts
            .mixer
            .strips
            .as_ref()
            .ok()?
            .iter()
            .find(|strip| strip.track == track.raw())
    }

    /// The outputs a lane could have: the master, then every bus that would not
    /// feed the lane back into itself.
    pub fn output_targets(&self, track: TrackId) -> &[OutputTarget] {
        self.strip_of(track)
            .map_or(&[][..], |strip| strip.output_targets.as_slice())
    }

    /// The value to draw for `target`: the dragged one while a drag is on it,
    /// the stored one otherwise. Every strip reads its numbers through here, so
    /// the picture cannot disagree with the pointer.
    pub fn fader_value(&self, target: FaderTarget, stored: f32) -> f32 {
        match self.fader_drag {
            Some(drag) if drag.target == target => drag.gain,
            _ => self.effective_target_value(target, stored),
        }
    }

    /// The fader being ridden, if any.
    pub fn fader_dragging(&self) -> Option<FaderDrag> {
        self.fader_drag
    }

    /// Grab a fader at its current value, at window coordinate `grabbed_at`, on
    /// a strip whose travel is `travel` pixels.
    pub fn begin_fader_drag(&mut self, target: FaderTarget, grabbed_at: f32, travel: f32) {
        let stored = self
            .stored_gain(target)
            .unwrap_or(if target.is_pan() { 0.0 } else { 1.0 });
        let gain = self.effective_target_value(target, stored);
        if self.playing && self.target_automation_mode(target).writes_envelope() {
            self.mixer_automation.pre_touch_envelope = self.target_envelope(target);
            self.mixer_automation.last_drag_tick = Some(self.playhead.value);
            self.mixer_automation
                .latched
                .retain(|(latched_target, _, _)| *latched_target != target);
        }
        self.fader_drag = Some(FaderDrag {
            target,
            gain,
            grabbed_at,
            grabbed_frac: target.value_to_frac(gain),
            travel: if travel > 0.0 { travel } else { 1.0 },
        });
    }

    fn stored_gain(&self, target: FaderTarget) -> Option<f32> {
        match target {
            FaderTarget::Lane(track) => self.strip_of(track).map(|strip| strip.gain),
            FaderTarget::LanePan(track) => self.strip_of(track).map(|strip| strip.pan),
            FaderTarget::Send { track, index } => self
                .strip_of(track)
                .and_then(|strip| strip.sends.get(index))
                .map(|send| send.level),
            FaderTarget::Master => Some(self.facts.mixer.master.gain),
            FaderTarget::MasterPan => Some(self.facts.mixer.master.pan),
        }
    }

    /// Move the dragged fader or pan knob to a position on its travel.
    pub fn drag_fader_to(&mut self, frac: f32) -> bool {
        let Some(mut drag) = self.fader_drag else {
            return false;
        };
        let gain = drag.target.frac_to_value(frac);
        if gain == drag.gain {
            return false;
        }
        drag.gain = gain;
        self.fader_drag = Some(drag);
        if self.playing && self.target_automation_mode(drag.target).writes_envelope() {
            let tick = self.playhead.value;
            let from = self.mixer_automation.last_drag_tick.unwrap_or(tick);
            let _ = self.write_automation_span(drag.target, gain, from, tick, None);
            self.mixer_automation.last_drag_tick = Some(tick);
        }
        true
    }

    /// Ride the grabbed fader to window y `at`. Up is louder, and the distance
    /// is measured from the grab, so the fader tracks the hand rather than the
    /// cursor's absolute position on the strip.
    pub fn drag_fader_to_pointer(&mut self, at: f32) -> bool {
        let Some(drag) = self.fader_drag else {
            return false;
        };
        if !at.is_finite() {
            return false;
        }
        let delta = if drag.target.is_pan() {
            at - drag.grabbed_at
        } else {
            drag.grabbed_at - at
        };
        let frac = drag.grabbed_frac + delta / drag.travel;
        self.drag_fader_to(frac)
    }

    /// Abandon the drag; the stored value is what stays.
    pub fn cancel_fader_drag(&mut self) {
        self.fader_drag = None;
    }

    /// Point a lane at a bus, or back at the master.
    pub fn set_track_output(
        &mut self,
        track: TrackId,
        to: Option<TrackId>,
    ) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::SetTrackOutput { track, to }))?;
        Ok(())
    }

    /// One click on a (lane, bus) pair: off becomes a post-fader send, post
    /// becomes pre, and pre goes back to off.
    ///
    /// Three states on one control rather than a menu: the cycle is the whole
    /// vocabulary of a send apart from its level, which is the drag.
    pub fn cycle_send(&mut self, track: TrackId, to: TrackId) -> Result<(), EngineError> {
        let held = {
            let engine = self.engine.lock();
            let timeline = engine.project().timeline();
            timeline.track(track).and_then(|lane| {
                lane.sends
                    .iter()
                    .position(|send| send.to == to)
                    .map(|at| (at, lane.sends[at].pre_fader))
            })
        };
        let command = match held {
            None => EditCommand::AddSend {
                track,
                to,
                level: AudioSend::unity(to).level,
                pre_fader: false,
            },
            Some((index, false)) => EditCommand::SetSend {
                track,
                index,
                level: None,
                pre_fader: Some(true),
            },
            Some((index, true)) => EditCommand::RemoveSend { track, index },
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }

    /// Drop one send from a lane.
    pub fn remove_send(&mut self, track: TrackId, index: usize) -> Result<(), EngineError> {
        self.apply_command(Command::Edit(EditCommand::RemoveSend { track, index }))?;
        Ok(())
    }

    /// Add a submix bus, named after how many the project already has.
    pub fn add_submix_lane(&mut self) -> Result<(), EngineError> {
        let next = {
            let engine = self.engine.lock();
            engine
                .project()
                .timeline()
                .tracks_ordered()
                .filter(|track| track.kind.is_bus())
                .count()
                + 1
        };
        self.add_track(aria_models::TrackKind::Submix, format!("Bus {next}"))?;
        Ok(())
    }
}
