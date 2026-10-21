//! The master bus: its insert chain, gain, pan, mute and automation.

use super::Timeline;
use crate::audio_effects::AudioEffectInstance;
use crate::param::Param;
use crate::track_routing::AutomationMode;

impl Timeline {
    /// The master insert chain, in signal order.
    pub fn master_audio_effects(&self) -> &[AudioEffectInstance] {
        &self.master_audio_effects
    }

    pub(crate) fn master_audio_effects_mut(&mut self) -> &mut Vec<AudioEffectInstance> {
        &mut self.master_audio_effects
    }

    /// Master strip static fader, as linear gain.
    pub fn master_gain(&self) -> f32 {
        self.master_gain
    }

    /// Master strip static stereo pan (`-1.0..=1.0`).
    pub fn master_pan(&self) -> f32 {
        self.master_pan
    }

    /// Whether the master strip is muted.
    pub fn master_muted(&self) -> bool {
        self.master_muted
    }

    /// Master strip automation mode.
    pub fn master_automation(&self) -> AutomationMode {
        self.master_automation
    }

    /// Master strip sequence-tick volume automation curve.
    pub fn master_volume_envelope(&self) -> &Param<f32> {
        &self.master_volume_envelope
    }

    /// Master strip sequence-tick pan automation curve.
    pub fn master_pan_envelope(&self) -> &Param<f32> {
        &self.master_pan_envelope
    }

    /// Effective master linear gain at sequence `tick` (`0.0` when muted).
    pub fn master_gain_at(&self, tick: i64) -> f32 {
        if self.master_muted {
            return 0.0;
        }
        if self.master_automation.reads_envelope() && self.master_volume_envelope.is_animated() {
            self.master_volume_envelope.sample(tick)
        } else {
            self.master_gain
        }
    }

    /// Effective master stereo pan (`-1.0..=1.0`) at sequence `tick`.
    pub fn master_pan_at(&self, tick: i64) -> f32 {
        if self.master_automation.reads_envelope() && self.master_pan_envelope.is_animated() {
            self.master_pan_envelope.sample(tick)
        } else {
            self.master_pan
        }
    }

    pub(crate) fn set_master_mix_unchecked(
        &mut self,
        gain: f32,
        pan: f32,
        muted: bool,
        automation: AutomationMode,
        volume: Param<f32>,
        pan_envelope: Param<f32>,
    ) {
        self.master_gain = gain;
        self.master_pan = pan;
        self.master_muted = muted;
        self.master_automation = automation;
        self.master_volume_envelope = volume;
        self.master_pan_envelope = pan_envelope;
    }
}
