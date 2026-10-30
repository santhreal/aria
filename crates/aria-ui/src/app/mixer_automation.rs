//! Mixer pan conversions, strip mute/solo/automation commands, and playback
//! automation recording (`Write`, `Touch`, `Latch`) grouped into one undo step
//! per playback pass.

use super::*;
use aria_engine::EngineError;
use aria_models::{AutomationMode, Easing, Param, TrackId};

/// Snap window around center pan (`0.0`), in pan units (`-1.0..=1.0`).
const CENTER_PAN_DETENT: f32 = 0.04;

/// Map stereo pan (`-1.0` left .. `+1.0` right) to `[0.0, 1.0]`.
pub fn pan_to_frac(pan: f32) -> f32 {
    ((pan.clamp(-1.0, 1.0) + 1.0) * 0.5).clamp(0.0, 1.0)
}

/// Map a `[0.0, 1.0]` travel fraction to stereo pan (`-1.0..=1.0`) with a
/// center detent at `0.0`.
pub fn frac_to_pan(frac: f32) -> f32 {
    if !frac.is_finite() {
        return 0.0;
    }
    let pan = (frac.clamp(0.0, 1.0) * 2.0 - 1.0).clamp(-1.0, 1.0);
    if pan.abs() <= CENTER_PAN_DETENT {
        0.0
    } else {
        pan
    }
}

/// Readout label for a stereo pan value (`"C"`, `"L50"`, `"R50"`).
pub fn pan_label(pan: f32) -> String {
    let pct = (pan.clamp(-1.0, 1.0) * 100.0).round() as i32;
    match pct.cmp(&0) {
        std::cmp::Ordering::Equal => "C".to_string(),
        std::cmp::Ordering::Less => format!("L{}", -pct),
        std::cmp::Ordering::Greater => format!("R{pct}"),
    }
}

/// Session state for one playback pass of mixer automation recording.
#[derive(Debug, Clone, Default)]
pub struct AutomationPassState {
    /// `playback_generation` of the pass whose first automation command has
    /// already been recorded on the undo stack.
    pub(crate) active_pass: Option<u64>,
    /// Sequence tick where the current drag started or last wrote.
    pub(crate) last_drag_tick: Option<i64>,
    /// Snapshot of the target's envelope taken when the fader or pan knob was
    /// grabbed, so `Touch` mode can return to the pre-touch curve on release.
    pub(crate) pre_touch_envelope: Option<Param<f32>>,
    /// Targets held after release in `Latch` or `Write` mode during the active
    /// playback pass: `(target, value, last_tick)`.
    pub(crate) latched: Vec<(FaderTarget, f32, i64)>,
}

impl AutomationPassState {
    pub fn clear(&mut self) {
        self.active_pass = None;
        self.last_drag_tick = None;
        self.pre_touch_envelope = None;
        self.latched.clear();
    }
}

impl FaderTarget {
    /// Whether this target controls stereo pan (`-1.0..=1.0`) rather than
    /// linear gain (`0.0..=4.0`).
    pub const fn is_pan(self) -> bool {
        matches!(self, Self::LanePan(_) | Self::MasterPan)
    }

    pub fn value_to_frac(self, value: f32) -> f32 {
        if self.is_pan() {
            pan_to_frac(value)
        } else {
            gain_to_frac(value)
        }
    }

    pub fn frac_to_value(self, frac: f32) -> f32 {
        if self.is_pan() {
            frac_to_pan(frac)
        } else {
            frac_to_gain(frac)
        }
    }
}

impl AppState {
    /// Reset the active playback automation pass (called when playback starts
    /// or stops so each playback pass groups into its own single undo entry).
    pub(crate) fn clear_mixer_automation_pass(&mut self) {
        self.mixer_automation.clear();
    }

    /// Toggle mute on a track strip (`Some(track)`) or the master strip (`None`).
    pub fn toggle_strip_mute(&mut self, track: Option<TrackId>) -> Result<(), EngineError> {
        let command = match track {
            Some(track) => {
                let muted = !self.strip_of(track).is_some_and(|s| s.muted);
                EditCommand::SetTrackMuted { track, muted }
            }
            None => {
                let m = &self.facts.mixer.master;
                EditCommand::SetMasterMix {
                    gain: m.gain,
                    pan: m.pan,
                    muted: !m.muted,
                    automation: m.automation,
                    volume: m.volume_envelope.clone(),
                    pan_envelope: m.pan_envelope.clone(),
                }
            }
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }

    /// Toggle solo-in-place on a track strip.
    pub fn toggle_strip_solo(&mut self, track: TrackId) -> Result<(), EngineError> {
        let solo = !self.strip_of(track).is_some_and(|s| s.solo);
        self.apply_command(Command::Edit(EditCommand::SetTrackSolo { track, solo }))?;
        Ok(())
    }

    /// Set the automation mode on a track strip (`Some(track)`) or the master
    /// strip (`None`).
    pub fn set_strip_automation_mode(
        &mut self,
        track: Option<TrackId>,
        mode: AutomationMode,
    ) -> Result<(), EngineError> {
        let command = match track {
            Some(track) => {
                let Some(strip) = self.strip_of(track) else {
                    return Ok(());
                };
                EditCommand::SetTrackAutomation {
                    track,
                    mode,
                    volume: strip.volume_envelope.clone(),
                    pan: strip.pan_envelope.clone(),
                }
            }
            None => {
                let m = &self.facts.mixer.master;
                EditCommand::SetMasterMix {
                    gain: m.gain,
                    pan: m.pan,
                    muted: m.muted,
                    automation: mode,
                    volume: m.volume_envelope.clone(),
                    pan_envelope: m.pan_envelope.clone(),
                }
            }
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }

    /// Automation mode of the strip owning `target`, if `target` is a strip
    /// fader or pan knob. Sends have no automation mode of their own.
    pub(super) fn target_automation_mode(&self, target: FaderTarget) -> AutomationMode {
        match target {
            FaderTarget::Lane(track) | FaderTarget::LanePan(track) => self
                .strip_of(track)
                .map_or(AutomationMode::Off, |s| s.automation),
            FaderTarget::Master | FaderTarget::MasterPan => self.facts.mixer.master.automation,
            FaderTarget::Send { .. } => AutomationMode::Off,
        }
    }

    /// Current envelope for `target`, when `target` is a strip fader or pan.
    pub(super) fn target_envelope(&self, target: FaderTarget) -> Option<Param<f32>> {
        match target {
            FaderTarget::Lane(track) => self.strip_of(track).map(|s| s.volume_envelope.clone()),
            FaderTarget::LanePan(track) => self.strip_of(track).map(|s| s.pan_envelope.clone()),
            FaderTarget::Master => Some(self.facts.mixer.master.volume_envelope.clone()),
            FaderTarget::MasterPan => Some(self.facts.mixer.master.pan_envelope.clone()),
            FaderTarget::Send { .. } => None,
        }
    }

    /// Effective value of `target` at the current playhead when no drag is
    /// active: samples the automation curve when the strip's mode reads
    /// envelopes and the curve has keyframes; otherwise returns `stored`.
    pub(super) fn effective_target_value(&self, target: FaderTarget, stored: f32) -> f32 {
        let mode = self.target_automation_mode(target);
        if !mode.reads_envelope() {
            return stored;
        }
        let Some(env) = self.target_envelope(target) else {
            return stored;
        };
        if env.is_animated() {
            env.sample(self.playhead.value)
        } else {
            stored
        }
    }

    /// Extend any held or latched automation writes when the playhead moves
    /// during playback.
    pub(crate) fn on_playback_tick_automation(&mut self, tick: i64) {
        if !self.playing {
            return;
        }
        if let Some(drag) = self.fader_drag {
            let mode = self.target_automation_mode(drag.target);
            if mode.writes_envelope() {
                let from = self.mixer_automation.last_drag_tick.unwrap_or(tick);
                let _ = self.write_automation_span(drag.target, drag.gain, from, tick, None);
                self.mixer_automation.last_drag_tick = Some(tick);
            }
        }
        let latched = self.mixer_automation.latched.clone();
        for (idx, (target, value, last_tick)) in latched.into_iter().enumerate() {
            let mode = self.target_automation_mode(target);
            if matches!(mode, AutomationMode::Latch | AutomationMode::Write) {
                let _ = self.write_automation_span(target, value, last_tick, tick, None);
                if let Some(slot) = self.mixer_automation.latched.get_mut(idx) {
                    slot.2 = tick;
                }
            }
        }
    }

    /// Record keyframes on `target`'s envelope across `[from_tick, to_tick]`
    /// and coalesce all writes within the current playback pass into a single
    /// undo step.
    pub(super) fn write_automation_span(
        &mut self,
        target: FaderTarget,
        value: f32,
        from_tick: i64,
        to_tick: i64,
        on_release_touch: Option<&Param<f32>>,
    ) -> Result<(), EngineError> {
        let t_min = from_tick.min(to_tick).max(0);
        let t_max = from_tick.max(to_tick).max(0);
        let command = match target {
            FaderTarget::Lane(track) | FaderTarget::LanePan(track) => {
                let Some(strip) = self.strip_of(track) else {
                    return Ok(());
                };
                let mode = strip.automation;
                let mut vol = strip.volume_envelope.clone();
                let mut pan = strip.pan_envelope.clone();
                if target.is_pan() {
                    update_envelope(
                        &mut pan,
                        strip.pan,
                        value,
                        t_min,
                        t_max,
                        mode,
                        on_release_touch,
                    );
                } else {
                    update_envelope(
                        &mut vol,
                        strip.gain,
                        value,
                        t_min,
                        t_max,
                        mode,
                        on_release_touch,
                    );
                }
                EditCommand::SetTrackAutomation {
                    track,
                    mode,
                    volume: vol,
                    pan,
                }
            }
            FaderTarget::Master | FaderTarget::MasterPan => {
                let m = (*self.facts.mixer.master).clone();
                let mut vol = m.volume_envelope;
                let mut pan = m.pan_envelope;
                if target.is_pan() {
                    update_envelope(
                        &mut pan,
                        m.pan,
                        value,
                        t_min,
                        t_max,
                        m.automation,
                        on_release_touch,
                    );
                } else {
                    update_envelope(
                        &mut vol,
                        m.gain,
                        value,
                        t_min,
                        t_max,
                        m.automation,
                        on_release_touch,
                    );
                }
                EditCommand::SetMasterMix {
                    gain: m.gain,
                    pan: m.pan,
                    muted: m.muted,
                    automation: m.automation,
                    volume: vol,
                    pan_envelope: pan,
                }
            }
            FaderTarget::Send { .. } => return Ok(()),
        };
        self.apply_automation_edit(command)
    }

    fn apply_automation_edit(&mut self, command: EditCommand) -> Result<(), EngineError> {
        let pass_id = self.playback_generation;
        let coalesce = self.playing && self.mixer_automation.active_pass == Some(pass_id);
        self.apply_command(Command::Edit(command))?;
        if coalesce {
            self.engine.lock().coalesce_last_two_undo_steps();
            self.sync_from_engine();
        } else if self.playing {
            self.mixer_automation.active_pass = Some(pass_id);
        }
        Ok(())
    }

    /// Release the fader or pan knob, writing one command (or recording
    /// automation during playback).
    pub fn commit_fader_drag(&mut self) -> Result<(), EngineError> {
        let Some(drag) = self.fader_drag.take() else {
            return Ok(());
        };
        let mode = self.target_automation_mode(drag.target);
        if self.playing && mode.writes_envelope() {
            let tick = self.playhead.value;
            let from = self.mixer_automation.last_drag_tick.unwrap_or(tick);
            let pre_touch = self.mixer_automation.pre_touch_envelope.take();
            if mode == AutomationMode::Touch {
                self.write_automation_span(drag.target, drag.gain, from, tick, pre_touch.as_ref())?;
            } else {
                self.write_automation_span(drag.target, drag.gain, from, tick, None)?;
                self.mixer_automation
                    .latched
                    .retain(|(t, _, _)| *t != drag.target);
                self.mixer_automation
                    .latched
                    .push((drag.target, drag.gain, tick));
            }
            self.mixer_automation.last_drag_tick = None;
            return Ok(());
        }
        if self.stored_gain(drag.target) == Some(drag.gain) {
            return Ok(());
        }
        let command = match drag.target {
            FaderTarget::Lane(track) => EditCommand::SetTrackGain {
                track,
                gain: drag.gain,
            },
            FaderTarget::LanePan(track) => EditCommand::SetTrackPan {
                track,
                pan: drag.gain,
            },
            FaderTarget::Send { track, index } => EditCommand::SetSend {
                track,
                index,
                level: Some(drag.gain),
                pre_fader: None,
            },
            FaderTarget::Master => {
                let m = &self.facts.mixer.master;
                EditCommand::SetMasterMix {
                    gain: drag.gain,
                    pan: m.pan,
                    muted: m.muted,
                    automation: m.automation,
                    volume: m.volume_envelope.clone(),
                    pan_envelope: m.pan_envelope.clone(),
                }
            }
            FaderTarget::MasterPan => {
                let m = &self.facts.mixer.master;
                EditCommand::SetMasterMix {
                    gain: m.gain,
                    pan: drag.gain,
                    muted: m.muted,
                    automation: m.automation,
                    volume: m.volume_envelope.clone(),
                    pan_envelope: m.pan_envelope.clone(),
                }
            }
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }
}

fn update_envelope(
    env: &mut Param<f32>,
    base_value: f32,
    value: f32,
    t_min: i64,
    t_max: i64,
    mode: AutomationMode,
    on_release_touch: Option<&Param<f32>>,
) {
    if !env.is_animated()
        && matches!(mode, AutomationMode::Touch | AutomationMode::Latch)
        && t_min > 0
    {
        let pre = on_release_touch.map_or(base_value, |p| p.sample(t_min - 1));
        env.set_keyframe(0, pre, Easing::Linear);
        env.set_keyframe(t_min - 1, pre, Easing::Linear);
    }
    let interior: Vec<i64> = env
        .keyframes()
        .iter()
        .map(|kf| kf.tick)
        .filter(|&t| t > t_min && t < t_max)
        .collect();
    for tick in interior {
        env.remove_keyframe(tick);
    }
    env.set_keyframe(t_min, value, Easing::Linear);
    if t_max > t_min {
        env.set_keyframe(t_max, value, Easing::Linear);
    }
    if let Some(pre_touch) = on_release_touch {
        let trailing: Vec<i64> = env
            .keyframes()
            .iter()
            .map(|kf| kf.tick)
            .filter(|&t| t > t_max)
            .collect();
        for tick in trailing {
            env.remove_keyframe(tick);
        }
        let resume_tick = t_max.saturating_add(1);
        let resume_val = if pre_touch.is_animated() {
            pre_touch.sample(resume_tick)
        } else {
            base_value
        };
        env.set_keyframe(resume_tick, resume_val, Easing::Linear);
        for kf in pre_touch.keyframes() {
            if kf.tick > resume_tick {
                env.set_keyframe(kf.tick, kf.value, kf.easing);
            }
        }
    } else if matches!(mode, AutomationMode::Latch | AutomationMode::Write) {
        let trailing: Vec<i64> = env
            .keyframes()
            .iter()
            .map(|kf| kf.tick)
            .filter(|&t| t > t_max)
            .collect();
        for tick in trailing {
            env.remove_keyframe(tick);
        }
    }
}
