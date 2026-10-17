//! The grade's drawn controls: the colour wheel a press holds, and the Color
//! panel's open sections.
//!
//! One gesture on a Lift, Gamma or Gain wheel is one edit. A press hands over
//! the wheel's frame in window pixels with the pointer, and the pointer is read
//! against that frame, so a wheel reads the same wherever a panel draws it and
//! in two panels at once. Every move rolls back the group
//! the previous move applied and writes the offset under the pointer, so the
//! whole drag is one undo step and Escape restores the grade the press found.
//! From the press on the shell routes the pointer here: the drag keeps going
//! when the pointer leaves the wheel.
//!
//! A wheel moves the balance between its three channels and leaves their mean,
//! the level the press found, where it is: the master sets the level. The rim
//! moves a channel by the wheel's reach, a quarter for Lift and a half for
//! Gamma and Gain, so a gamma channel at unity stays inside `[0.5, 1.5]`.

use aria_engine::EngineError;
use aria_models::{ClipParam, GAIN_NEUTRAL, GAMMA_NEUTRAL, LIFT_NEUTRAL, LookParam};
use aria_plugin::{EditGroup, Origin};

use super::AppState;

/// Map a pointer to a wheel offset in the unit disc. `center` and `pointer`
/// are in the same pixels; a pointer outside the rim lands on the rim.
pub fn pointer_to_wheel_offset(center: [f32; 2], radius: f32, pointer: [f32; 2]) -> [f32; 2] {
    if radius <= 0.0 {
        return [0.0, 0.0];
    }
    let dx = pointer[0] - center[0];
    let dy = pointer[1] - center[1];
    let dist = (dx * dx + dy * dy).sqrt();
    if dist > radius {
        [dx / dist, dy / dist]
    } else {
        [dx / radius, dy / radius]
    }
}

/// Convert a unit-disc offset into RGB deltas that sum to zero.
///
/// Red sits at 0°, green at 120° and blue at 240°, so the deltas move the
/// balance between channels and leave the sum, which the master holds, alone.
pub fn wheel_offset_to_rgb_deltas(offset: [f32; 2]) -> [f32; 3] {
    let sqrt3_over_2 = 0.866_025_4;
    let dr = offset[0];
    let dg = -0.5 * offset[0] + sqrt3_over_2 * offset[1];
    let db = -0.5 * offset[0] - sqrt3_over_2 * offset[1];
    [dr, dg, db]
}

/// Convert RGB deltas back into a unit-disc offset, held to the rim.
pub fn rgb_deltas_to_wheel_offset(deltas: [f32; 3]) -> [f32; 2] {
    let sqrt3 = 1.732_050_8;
    let ux = deltas[0];
    let uy = (deltas[1] - deltas[2]) / sqrt3;
    let dist = (ux * ux + uy * uy).sqrt();
    if dist > 1.0 {
        [ux / dist, uy / dist]
    } else {
        [ux, uy]
    }
}

/// The three grading wheels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelControl {
    Lift,
    Gamma,
    Gain,
}

impl WheelControl {
    pub const ALL: [Self; 3] = [Self::Lift, Self::Gamma, Self::Gain];

    pub fn label(self) -> &'static str {
        match self {
            Self::Lift => "Lift",
            Self::Gamma => "Gamma",
            Self::Gain => "Gain",
        }
    }

    /// The value each channel and the master rest at.
    pub fn neutral_master(self) -> f32 {
        match self {
            Self::Lift => LIFT_NEUTRAL,
            Self::Gamma => GAMMA_NEUTRAL,
            Self::Gain => GAIN_NEUTRAL,
        }
    }

    /// How far the rim moves a channel from the channels' mean.
    pub fn reach(self) -> f32 {
        match self {
            Self::Lift => 0.25,
            Self::Gamma | Self::Gain => 0.5,
        }
    }

    pub fn rgb_params(self) -> [LookParam; 3] {
        match self {
            Self::Lift => [
                LookParam::AdjustLiftR,
                LookParam::AdjustLiftG,
                LookParam::AdjustLiftB,
            ],
            Self::Gamma => [
                LookParam::AdjustGammaR,
                LookParam::AdjustGammaG,
                LookParam::AdjustGammaB,
            ],
            Self::Gain => [
                LookParam::AdjustGainR,
                LookParam::AdjustGainG,
                LookParam::AdjustGainB,
            ],
        }
    }

    pub fn master_param(self) -> LookParam {
        match self {
            Self::Lift => LookParam::AdjustLiftMaster,
            Self::Gamma => LookParam::AdjustGammaMaster,
            Self::Gain => LookParam::AdjustGainMaster,
        }
    }

    /// Put the three channels and the master back at neutral, as one undo
    /// step, on every selected clip that has the wheel. A wheel already at
    /// neutral leaves no step.
    pub fn reset(self, state: &mut AppState) -> Result<(), EngineError> {
        let peers = state.peers_sharing(self.look(self.master_param()));
        // A refused write drops the group, which rolls it back.
        let wrote = state.host.group(&Origin::Editor).and_then(|group| {
            state.edit_each(peers, |state| self.neutralize(state))?;
            Ok(group)
        });
        let result = match wrote {
            Ok(group) => {
                group.commit_as(self.reset_label());
                Ok(())
            }
            Err(err) => Err(err),
        };
        // The commit changes the History list, and a rollback the project.
        state.sync_from_engine();
        result
    }

    /// Put the selected clip's channels and master at neutral.
    fn neutralize(self, state: &mut AppState) -> Result<(), EngineError> {
        let neutral = self.neutral_master();
        for param in self.rgb_params().into_iter().chain([self.master_param()]) {
            if self.channel(state, param) != neutral {
                state.set_param_scalar(self.look(param), None, neutral)?;
            }
        }
        Ok(())
    }

    /// The address of one of this wheel's channels.
    fn look(self, param: LookParam) -> ClipParam {
        ClipParam::Look { param }
    }

    /// The History line a drag on this wheel reads as.
    pub fn drag_label(self) -> &'static str {
        match self {
            Self::Lift => "Lift wheel",
            Self::Gamma => "Gamma wheel",
            Self::Gain => "Gain wheel",
        }
    }

    /// The History line a reset of this wheel reads as.
    pub fn reset_label(self) -> &'static str {
        match self {
            Self::Lift => "Reset Lift",
            Self::Gamma => "Reset Gamma",
            Self::Gain => "Reset Gain",
        }
    }

    /// The id a drawn wheel's selector ends in.
    pub fn id(self) -> &'static str {
        match self {
            Self::Lift => "lift",
            Self::Gamma => "gamma",
            Self::Gain => "gain",
        }
    }

    /// One channel as the playhead reads it.
    fn channel(self, state: &AppState, param: LookParam) -> f32 {
        state
            .param_scalar(ClipParam::Look { param }, None)
            .unwrap_or(self.neutral_master())
    }

    /// The mean of the three channels: the part of them a wheel leaves alone.
    pub fn channel_mean(self, state: &AppState) -> f32 {
        let [r, g, b] = self.rgb_params().map(|param| self.channel(state, param));
        (r + g + b) / 3.0
    }

    /// Write the channels for a wheel offset in the unit disc, around `mean`.
    pub fn apply_offset(
        self,
        state: &mut AppState,
        offset: [f32; 2],
        mean: f32,
    ) -> Result<(), EngineError> {
        let deltas = wheel_offset_to_rgb_deltas(offset);
        for (param, delta) in self.rgb_params().into_iter().zip(deltas) {
            let value = mean + self.reach() * delta;
            // A channel already there takes no command, so a press that moves
            // nothing leaves no undo step.
            if self.channel(state, param) == value {
                continue;
            }
            state.set_param_scalar(ClipParam::Look { param }, None, value)?;
        }
        Ok(())
    }

    /// The wheel offset the channels read as now.
    pub fn current_offset(self, state: &AppState) -> [f32; 2] {
        let channels = self.rgb_params().map(|param| self.channel(state, param));
        let mean = (channels[0] + channels[1] + channels[2]) / 3.0;
        rgb_deltas_to_wheel_offset(channels.map(|value| (value - mean) / self.reach()))
    }

    pub fn current_master(self, state: &AppState) -> f32 {
        self.channel(state, self.master_param())
    }
}

/// A wheel under a held press.
struct Held {
    control: WheelControl,
    /// The channels' mean when the press landed, which the drag holds.
    mean: f32,
    /// The wheel's frame in window pixels: x, y, width, height.
    frame: [f32; 4],
    /// The offset the open group holds.
    applied: Option<[f32; 2]>,
    group: Option<EditGroup>,
}

/// The wheel a press holds, until it is released, and the Color panel
/// sections drawn open.
pub struct GradeControls {
    held: Option<Held>,
    /// One bit per Color panel section, set while it is open.
    open: u32,
}

impl Default for GradeControls {
    /// Basic Correction open, every other section closed.
    fn default() -> Self {
        Self {
            held: None,
            open: section_bit(0),
        }
    }
}

impl std::fmt::Debug for GradeControls {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let held = self.held.as_ref();
        f.debug_struct("GradeControls")
            .field("control", &held.map(|held| held.control))
            .field("frame", &held.map(|held| held.frame))
            .field("applied", &held.and_then(|held| held.applied))
            .field("open", &format_args!("{:#b}", self.open))
            .finish_non_exhaustive()
    }
}

/// Sections the Color panel keeps open state for: one bit each.
pub(crate) const COLOR_SECTIONS: usize = u32::BITS as usize;

/// The bit section `index` is kept in; none past [`COLOR_SECTIONS`].
fn section_bit(index: usize) -> u32 {
    u32::try_from(index)
        .ok()
        .and_then(|index| 1_u32.checked_shl(index))
        .unwrap_or(0)
}

impl AppState {
    /// Whether the Color panel draws section `index` open.
    pub fn color_section_open(&self, index: usize) -> bool {
        self.grade_controls.open & section_bit(index) != 0
    }

    /// Open Color panel section `index` when it is closed, close it when open.
    pub fn toggle_color_section(&mut self, index: usize) {
        self.grade_controls.open ^= section_bit(index);
    }

    /// The wheel a press holds.
    pub fn wheel_held(&self) -> Option<WheelControl> {
        self.grade_controls.held.as_ref().map(|held| held.control)
    }

    /// Press on a wheel drawn in `frame` (x, y, width, height in window
    /// pixels) at `at`, in the same pixels. The offset under the pointer is
    /// written at once. Returns whether the grade changed.
    pub fn press_wheel(&mut self, control: WheelControl, frame: [f32; 4], at: [f32; 2]) -> bool {
        self.cancel_wheel_drag();
        let mean = control.channel_mean(self);
        self.grade_controls.held = Some(Held {
            control,
            mean,
            frame,
            applied: None,
            group: None,
        });
        self.drag_wheel(at)
    }

    /// The pointer moved to `at` with a wheel held. Returns whether the grade
    /// changed.
    pub fn drag_wheel(&mut self, at: [f32; 2]) -> bool {
        let Some(held) = self.grade_controls.held.as_mut() else {
            return false;
        };
        let [x, y, width, height] = held.frame;
        let offset = pointer_to_wheel_offset(
            [x + width * 0.5, y + height * 0.5],
            width.min(height) * 0.5,
            at,
        );
        if held.applied == Some(offset) {
            return false;
        }
        let (control, mean) = (held.control, held.mean);
        let previous = held.group.take();
        if let Some(group) = previous {
            group.rollback();
            self.sync_from_engine();
        }
        // The other selected clips that grade take the channels the drag
        // lands on the first one, so one wheel moves them all alike.
        let peers = self.peers_sharing(control.look(control.master_param()));
        let landed = control
            .rgb_params()
            .map(|param| (control.look(param), None::<u8>));
        let wrote = self.host.group(&Origin::Editor).and_then(|group| {
            self.land_each(peers, &landed, |state| {
                control.apply_offset(state, offset, mean)
            })?;
            Ok(group)
        });
        match wrote {
            Ok(group) => {
                if let Some(held) = self.grade_controls.held.as_mut() {
                    held.applied = Some(offset);
                    held.group = Some(group);
                }
            }
            Err(err) => {
                if let Some(held) = self.grade_controls.held.as_mut() {
                    held.applied = None;
                }
                self.sync_from_engine();
                self.report_error(&err);
            }
        }
        true
    }

    /// Let go of the wheel: what the drag wrote stays as one undo step.
    /// Returns whether a wheel was held.
    pub fn release_wheel(&mut self) -> bool {
        let Some(held) = self.grade_controls.held.take() else {
            return false;
        };
        if let Some(group) = held.group {
            group.commit_as(held.control.drag_label());
            // The commit changes the History list and not the project.
            self.sync_from_engine();
        }
        true
    }

    /// Escape during a drag: put the grade back where the press found it.
    /// Returns whether a wheel was held.
    pub fn cancel_wheel_drag(&mut self) -> bool {
        let Some(held) = self.grade_controls.held.take() else {
            return false;
        };
        if let Some(group) = held.group {
            group.rollback();
            self.sync_from_engine();
        }
        true
    }
}
