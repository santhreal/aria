//! Generated property rows: one read path and one write path for every
//! animatable property the model declares.
//!
//! Each inspector block used to carry its own knob table — `MASK_KNOBS`,
//! `CHROMA_KNOBS`, `STYLE_KNOBS` — pairing a reader closure with a writer
//! closure per property, so a property the model gained was a property the
//! panel silently lacked. Worse, those writers rebuilt the whole block as
//! constants: moving the mask feather slider deleted every mask keyframe,
//! because `SetClipMask` carries the entire mask and the UI had nothing else to
//! put in it.
//!
//! Rows come from the projected property list (see
//! [`crate::app::clip_keys::KeyParam`]) and are addressed by [`ClipParam`], so a
//! panel states which group it draws and nothing else. The lane's refusal is
//! already applied there, so a group cannot offer a property its lane rejects by
//! forgetting to ask. The write path is the keyframe-aware one the transform
//! already used: an animated property takes a keyframe under the playhead, a
//! constant property takes a constant, and neither drops the other properties in
//! its block.

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{ClipParam, Easing, ParamValue};

use super::AppState;

/// One control a panel draws for one animatable property.
///
/// A vector property becomes two rows, one per component, because that is what
/// a stepper can edit; the component travels with the row so the write path
/// knows which half of the value it is replacing.
#[derive(Debug, Clone, PartialEq)]
pub struct ParamRow {
    /// The catalog id, suffixed `-x` / `-y` on a component row. Stable enough
    /// for an element id and for a test to name a row. Shared text: both this
    /// and the label are built where the property list is projected, so a frame
    /// clones a refcount rather than the string.
    pub id: gpui::SharedString,
    pub label: gpui::SharedString,
    pub param: ClipParam,
    /// Which component of a vector property this row edits, `None` for a scalar.
    pub component: Option<u8>,
    pub value: f32,
    /// The bounds the model enforces, per side. A row bounded on both sides can
    /// size its own step; one that is open on a side cannot, and says so.
    pub range: aria_models::ParamRange,
    /// Whether the property carries keyframes, so an edit here moves a curve
    /// rather than a constant.
    pub animated: bool,
    /// Whether a keyframe sits exactly under the playhead, so the row's
    /// stopwatch is lit without a second reader asking the same question.
    pub at_playhead: bool,
}

impl ParamRow {
    /// The amount one nudge moves this row.
    ///
    /// A bounded property divides its span into twenty steps, which is a
    /// stepper's useful granularity whatever the units are. An open-ended
    /// property steps by one, because the row has no scale to derive from and a
    /// pixel-ish unit is what open-ended properties here actually are (blur,
    /// radius, padding, offset).
    pub fn step(&self) -> f32 {
        match self.range.span() {
            Some(span) if span > 0.0 => span / 20.0,
            _ => 1.0,
        }
    }

    /// The value as a row shows it: two decimals inside a bounded range, one
    /// where the property is open-ended and typically counted in pixels.
    pub fn display(&self) -> String {
        match self.range.span() {
            Some(_) => format!("{:.2}", self.value),
            None => format!("{:.1}", self.value),
        }
    }
}

impl AppState {
    /// The rows of one property group for the selected clip, in catalog order.
    ///
    /// Empty when nothing is selected, when the group owns nothing on this clip
    /// (an unmasked clip has no `mask` rows), or when the lane refuses the
    /// group's scope. Colour and rectangle properties yield no rows: a swatch
    /// and the crop handles own those, and the conformance test in
    /// `param_rows_tests.rs` holds that exemption to the two kinds.
    pub fn param_rows(&self, group: &str) -> Vec<ParamRow> {
        let Some(detail) = self.selected.as_ref() else {
            return Vec::new();
        };
        let clip = &detail.model;
        let local = self.read_tick(clip);
        let at = self.playhead.value;
        let mut rows = Vec::new();
        for row in detail.params.iter() {
            let spec = &row.spec;
            if spec.group != group {
                continue;
            }
            let Some(value) = spec.sample(clip, local) else {
                continue;
            };
            let animated = !row.marks.is_empty();
            let at_playhead = row.keyed_at(at);
            match value {
                ParamValue::Scalar(v) => rows.push(ParamRow {
                    id: row.id.clone(),
                    label: row.label.clone(),
                    param: spec.param,
                    component: None,
                    value: v,
                    range: spec.range,
                    animated,
                    at_playhead,
                }),
                ParamValue::Vec2(v) => {
                    for (index, axis) in row.axes().iter().enumerate() {
                        rows.push(ParamRow {
                            id: axis.id.clone(),
                            label: axis.label.clone(),
                            param: spec.param,
                            component: Some(index as u8),
                            value: v[index],
                            range: spec.range,
                            animated,
                            at_playhead,
                        });
                    }
                }
                // A colour needs a picker and a crop needs handles; neither is a
                // number a stepper can carry.
                ParamValue::Color(_) | ParamValue::Rect(_) => {}
            }
        }
        rows
    }

    /// The current value of one scalar or one vector component, as
    /// [`AppState::param_rows`] reports it.
    pub fn param_scalar(&self, param: ClipParam, component: Option<u8>) -> Option<f32> {
        let clip = self.selected_model()?;
        let spec = &self.key_param(param)?.spec;
        let local = self.read_tick(clip);
        match (spec.sample(clip, local)?, component) {
            (ParamValue::Scalar(v), None) => Some(v),
            (ParamValue::Vec2(v), Some(index)) => v.get(index as usize).copied(),
            _ => None,
        }
    }

    /// Set one scalar property, or one component of a vector property, to
    /// `value`.
    ///
    /// Clamped to the range the model declares, so a stepper cannot ask for a
    /// value the command would refuse. Writes a keyframe under the playhead when
    /// the property is animated and the playhead sits inside the clip, and a
    /// constant otherwise: an edit must not silently flatten a curve, and a
    /// property with no curve must not sprout one.
    ///
    /// Inert with nothing selected, on a property this clip does not have, and
    /// on a component that does not match the property's kind.
    pub fn set_param_scalar(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        value: f32,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(write) = self.param_write(param, component, value) else {
            return Ok(());
        };
        let command = match write.at {
            Some(at) => EditCommand::SetParamKeyframe {
                clip,
                param,
                at,
                value: write.value,
                easing: write.easing,
                tangents: None,
            },
            None => EditCommand::SetParamConstant {
                clip,
                param,
                value: write.value,
            },
        };
        self.apply_command(Command::Edit(command))?;
        Ok(())
    }

    /// Move one property by `delta`, from whatever it reads now. The step is the
    /// caller's; the bound is the model's.
    pub fn nudge_param(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        delta: f32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.param_scalar(param, component) else {
            return Ok(());
        };
        self.set_param_scalar(param, component, current + delta)
    }

    /// The tick a row reads at: the playhead, held inside the clip so a row
    /// still shows the value at the nearest edge rather than nothing, then
    /// addressed in whatever time the clip's parameters are keyed in.
    fn read_tick(&self, clip: &aria_models::Clip) -> i64 {
        clip.param_tick(self.playhead.value)
    }

    /// Resolve an edit into the payload and where it lands, or `None` when the
    /// property is not reachable or the component does not fit its kind.
    ///
    /// Reads the selected clip from the projection, so a stepper drag resolves
    /// its payload without waiting on whatever holds the session.
    fn param_write(
        &self,
        param: ClipParam,
        component: Option<u8>,
        value: f32,
    ) -> Option<ParamWrite> {
        let clip = self.selected_model()?;
        let row = self.key_param(param)?;
        let spec = &row.spec;
        let start = clip.timeline.start.value;
        let end = clip.timeline.end_tick();
        let local = self.read_tick(clip);
        // A non-finite value is dropped rather than clamped: it is a broken
        // input, not a value at a bound.
        if !value.is_finite() {
            return None;
        }
        let clamped = spec.range.clamp(value);
        let payload = match (spec.sample(clip, local)?, component) {
            (ParamValue::Scalar(_), None) => ParamValue::Scalar(clamped),
            (ParamValue::Vec2(mut v), Some(index)) => {
                *v.get_mut(index as usize)? = clamped;
                ParamValue::Vec2(v)
            }
            _ => return None,
        };
        let inside = self.playhead.value >= start && self.playhead.value < end;
        let animated = !row.marks.is_empty();
        Some(ParamWrite {
            value: payload,
            at: (animated && inside).then_some(self.playhead),
            easing: spec.easing_at(clip, local).unwrap_or(Easing::Linear),
        })
    }
}

/// A resolved edit: what to write, where, and with which curve leaving it.
struct ParamWrite {
    value: ParamValue,
    /// The playhead when the edit is a keyframe, `None` when it is a constant.
    at: Option<aria_models::RationalTime>,
    easing: Easing,
}
