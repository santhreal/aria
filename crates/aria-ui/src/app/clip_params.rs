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
use aria_models::{ClipParam, Easing, ParamValue, RationalTime};

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
    /// Whether a keyframe sits before the playhead, and whether one sits after
    /// it: where the navigator's arrows go.
    pub keys_around: (bool, bool),
    /// The property holds something other than its reset value at the
    /// playhead, on this clip or on another selected clip that shares it, so
    /// the reset button has something to do.
    pub off_default: bool,
    /// The source of the property's driver, which the row shows under the
    /// value. `value` is then the driver's result at the playhead.
    pub driver: Option<gpui::SharedString>,
    /// Why the driver failed at the playhead. `value` is then the keyframed
    /// value.
    pub fault: Option<gpui::SharedString>,
    /// Another selected clip that has the property reads differently on this
    /// row at the playhead, so the row prints a dash instead of `value`.
    pub mixed: bool,
}

/// One colour property, drawn as a row of swatches.
///
/// A colour is not a number a stepper can carry, so it gets its own row: the
/// value it reads at the playhead, and whether it is keyed, so the row's
/// stopwatch is the same affordance a stepper row draws.
#[derive(Debug, Clone, PartialEq)]
pub struct ColorRow {
    pub id: gpui::SharedString,
    pub label: gpui::SharedString,
    pub param: ClipParam,
    pub value: [u8; 4],
    pub animated: bool,
    pub at_playhead: bool,
    pub keys_around: (bool, bool),
    /// As on [`ParamRow::off_default`].
    pub off_default: bool,
    /// Another selected clip that has the colour reads a different one at the
    /// playhead, so the row names no colour.
    pub mixed: bool,
}

impl ParamRow {
    /// The amount one nudge moves this row; see [`param_step`].
    pub fn step(&self) -> f32 {
        param_step(&self.range)
    }

    /// The amount one unit of drag moves this row; see [`scrub_unit`].
    pub fn unit(&self) -> f32 {
        scrub_unit(&self.range)
    }

    /// Digits a row prints: two inside a bounded range, one where the property
    /// is open-ended and typically counted in pixels.
    pub fn decimals(&self) -> u8 {
        match self.range.span() {
            Some(_) => 2,
            None => 1,
        }
    }
}

/// Whether `spec` holds something other than its reset value at clip-local
/// `local`. `shown` is what the row reads there; a driven property reads its
/// driver's result, so its own value is sampled off the clip instead.
fn off_default(
    spec: &aria_models::ParamSpec,
    clip: &aria_models::Clip,
    driven: bool,
    local: i64,
    shown: ParamValue,
) -> bool {
    let Some(default) = spec.default_value(clip) else {
        return false;
    };
    let held = if driven {
        spec.sample(clip, local)
    } else {
        Some(shown)
    };
    held != Some(default)
}

/// The amount one unit of drag moves a property bounded by `range`.
///
/// A bounded property crosses its whole span in a hundred units, two hundred
/// pixels of drag at the scrub rate. An open-ended property moves by one, a
/// pixel-ish unit, which is what open-ended properties here are.
pub(crate) fn scrub_unit(range: &aria_models::ParamRange) -> f32 {
    match range.span() {
        Some(span) if span > 0.0 => span / 100.0,
        _ => 1.0,
    }
}

/// The amount one nudge moves a property bounded by `range`.
///
/// A bounded property divides its span into twenty steps, which is a stepper's
/// useful granularity whatever the units are. An open-ended property steps by
/// one, because the row has no scale to derive from and a pixel-ish unit is
/// what open-ended properties here are (blur, radius, padding, offset).
pub(crate) fn param_step(range: &aria_models::ParamRange) -> f32 {
    match range.span() {
        Some(span) if span > 0.0 => span / 20.0,
        _ => 1.0,
    }
}

impl AppState {
    /// The rows of one property group for the selected clip, in catalog order.
    ///
    /// Empty when nothing is selected, when the group owns nothing on this clip
    /// (an unmasked clip has no `mask-0` rows), or when the lane refuses the
    /// group's scope. Colour and rectangle properties yield no rows: a swatch
    /// and the crop handles own those, and the conformance test in
    /// `param_rows_tests.rs` holds that exemption to the two kinds.
    pub fn param_rows(&self, group: &str) -> Vec<ParamRow> {
        self.param_rows_where(|spec| spec.group == group)
    }

    /// The rows of the effect at `index` in the selected clip's chain, in slot
    /// order: one per keyframeable parameter the effect shows, one per axis of
    /// a vector. Each reads the value at the playhead, so a keyed parameter
    /// shows its curve where the playhead sits rather than where the clip
    /// starts.
    pub fn effect_param_rows(&self, index: usize) -> Vec<ParamRow> {
        self.param_rows_where(|spec| {
            matches!(spec.param, ClipParam::Effect { effect, .. } if effect as usize == index)
        })
    }

    /// The rows of every property `keep` accepts, in catalog order.
    fn param_rows_where(&self, keep: impl Fn(&aria_models::ParamSpec) -> bool) -> Vec<ParamRow> {
        let Some(detail) = self.selected.as_ref() else {
            return Vec::new();
        };
        let clip = &detail.model;
        // A driven property reads its driver's result; the driven clip holds
        // it as a constant, so any tick samples it.
        let shown = detail.driven.as_ref().unwrap_or(clip);
        let local = self.read_tick(clip);
        let at = self.playhead.value;
        let shared = self.edits_shared();
        let mut rows = Vec::new();
        for row in detail.params.iter() {
            let spec = &row.spec;
            if !keep(spec) {
                continue;
            }
            let Some(value) = spec.sample(shown, local) else {
                continue;
            };
            let animated = !row.marks.is_empty();
            let at_playhead = row.keyed_at(at);
            let keys_around = row.keys_around(at);
            let off_default = off_default(spec, clip, detail.driven.is_some(), local, value)
                || (shared && detail.peer_off_default(spec.param, at));
            let mixed = |component| shared && detail.mixed(spec.param, component, at, value);
            let view = detail.drivers.iter().find(|view| view.param == spec.param);
            let driver = view.map(|view| view.source.clone());
            let fault = view.and_then(|view| view.fault.clone());
            match value {
                ParamValue::Scalar(v) => rows.push(ParamRow {
                    id: row.id.clone(),
                    label: row.label.clone(),
                    param: spec.param,
                    component: None,
                    value: self.text_run_scalar(spec.param, v),
                    range: spec.range,
                    animated,
                    at_playhead,
                    keys_around,
                    off_default,
                    mixed: mixed(None),
                    driver,
                    fault,
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
                            keys_around,
                            off_default,
                            mixed: mixed(Some(index as u8)),
                            driver: driver.clone(),
                            fault: fault.clone(),
                        });
                    }
                }
                ParamValue::Vec3(v) => {
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
                            keys_around,
                            off_default,
                            mixed: mixed(Some(index as u8)),
                            driver: driver.clone(),
                            fault: fault.clone(),
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

    /// The colour properties of one group for the selected clip, in catalog
    /// order. The swatch twin of [`AppState::param_rows`]: empty when nothing
    /// is selected or the group owns no colour on this clip, so a title's
    /// stroke colour arrives with its stroke.
    pub fn param_colors(&self, group: &str) -> Vec<ColorRow> {
        self.param_colors_where(|spec| spec.group == group)
    }

    /// The colour parameters of the effect at `index`, in slot order.
    pub fn effect_param_colors(&self, index: usize) -> Vec<ColorRow> {
        self.param_colors_where(|spec| {
            matches!(spec.param, ClipParam::Effect { effect, .. } if effect as usize == index)
        })
    }

    /// The colour rows of every property `keep` accepts, in catalog order.
    fn param_colors_where(&self, keep: impl Fn(&aria_models::ParamSpec) -> bool) -> Vec<ColorRow> {
        let Some(detail) = self.selected.as_ref() else {
            return Vec::new();
        };
        let clip = &detail.model;
        let shown = detail.driven.as_ref().unwrap_or(clip);
        let local = self.read_tick(clip);
        let at = self.playhead.value;
        let shared = self.edits_shared();
        detail
            .params
            .iter()
            .filter(|row| keep(&row.spec))
            .filter_map(|row| match row.spec.sample(shown, local)? {
                sampled @ ParamValue::Color(value) => Some(ColorRow {
                    id: row.id.clone(),
                    label: row.label.clone(),
                    param: row.spec.param,
                    value: self.text_run_color(row.spec.param, value),
                    animated: !row.marks.is_empty(),
                    at_playhead: row.keyed_at(at),
                    keys_around: row.keys_around(at),
                    off_default: off_default(
                        &row.spec,
                        clip,
                        detail.driven.is_some(),
                        local,
                        sampled,
                    ) || (shared && detail.peer_off_default(row.spec.param, at)),
                    mixed: shared && detail.mixed(row.spec.param, None, at, sampled),
                }),
                _ => None,
            })
            .collect()
    }

    /// Set one colour property to `rgba`, through the same keyframe-aware
    /// write a stepper row uses: a keyframe where the row reads when the colour
    /// is animated, a constant otherwise.
    ///
    /// Inert with nothing selected, on a property this clip does not have, on
    /// a property that is not a colour, and on the colour already in force.
    pub fn set_param_color(&mut self, param: ClipParam, rgba: [u8; 4]) -> Result<(), EngineError> {
        // A title's fill over characters selected in the Viewer is theirs.
        if self.style_text_run_color(param, rgba)? {
            return Ok(());
        }
        let write = self.param_landing(param, |current, _| match current {
            ParamValue::Color(now) if now != rgba => Some(ParamValue::Color(rgba)),
            _ => None,
        });
        self.send_param(param, write)
    }

    /// The current value of one scalar or one vector component, as
    /// [`AppState::param_rows`] reports it.
    pub fn param_scalar(&self, param: ClipParam, component: Option<u8>) -> Option<f32> {
        match (self.param_value(param)?, component) {
            (ParamValue::Scalar(v), None) => Some(self.text_run_scalar(param, v)),
            (ParamValue::Vec2(v), Some(index)) => v.get(index as usize).copied(),
            (ParamValue::Vec3(v), Some(index)) => v.get(index as usize).copied(),
            _ => None,
        }
    }

    /// What one property of the selected clip holds at the playhead, held
    /// inside the clip. `None` with nothing selected or on a property the clip
    /// cannot animate.
    pub fn param_value(&self, param: ClipParam) -> Option<ParamValue> {
        let clip = self.selected_model()?;
        self.key_param(param)?
            .spec
            .sample(clip, self.read_tick(clip))
    }

    /// Set one property to `value` where a row edit lands: a keyframe under the
    /// playhead when the property is animated, a constant otherwise. Inert when
    /// the property already holds `value` there, and on a value of the wrong
    /// kind, which the model refuses.
    pub fn set_param_value(
        &mut self,
        param: ClipParam,
        value: ParamValue,
    ) -> Result<(), EngineError> {
        let write = self.param_landing(param, |current, _| (current != value).then_some(value));
        self.send_param(param, write)
    }

    /// Return one property to the value its block starts at, where a row edit
    /// lands: a keyframe under the playhead when the property is animated, so
    /// the rest of its curve stays, and a constant otherwise.
    ///
    /// Inert with nothing selected, on a property the clip does not offer or
    /// that has no reset value, and when the property already holds its reset
    /// value there.
    pub fn reset_param(&mut self, param: ClipParam) -> Result<(), EngineError> {
        let Some(value) = self.param_default(param) else {
            return Ok(());
        };
        self.set_param_value(param, value)
    }

    /// The value [`AppState::reset_param`] returns `param` to on the selected
    /// clip.
    pub fn param_default(&self, param: ClipParam) -> Option<ParamValue> {
        let clip = self.selected_model()?;
        self.key_param(param)?.spec.default_value(clip)
    }

    /// Whether `param` reads anything other than its reset value at the
    /// playhead: the reset button has something to do.
    pub fn param_off_default(&self, param: ClipParam) -> bool {
        match (self.param_value(param), self.param_default(param)) {
            (Some(value), Some(default)) => value != default,
            _ => false,
        }
    }

    /// Set one scalar property, or one component of a vector property, to
    /// `value`.
    ///
    /// Clamped to the range the model declares, so a stepper cannot ask for a
    /// value the command would refuse. Writes a keyframe when the property is
    /// animated and a constant otherwise: an edit must not silently flatten a
    /// curve, and a property with no curve must not sprout one. The keyframe
    /// lands under the playhead, or on the clip's nearest frame when the
    /// playhead is off the clip, which is the frame the row reads.
    ///
    /// Inert with nothing selected, on a property this clip does not have, and
    /// on a component that does not match the property's kind.
    pub fn set_param_scalar(
        &mut self,
        param: ClipParam,
        component: Option<u8>,
        value: f32,
    ) -> Result<(), EngineError> {
        // A title's size or tracking over characters selected in the Viewer
        // is theirs.
        if component.is_none() && self.style_text_run_scalar(param, value)? {
            return Ok(());
        }
        let write = self.param_write(param, component, value);
        self.send_param(param, write)
    }

    /// Send a resolved edit to the selected clip. Inert without a selection or
    /// without an edit.
    fn send_param(
        &mut self,
        param: ClipParam,
        write: Option<ParamWrite>,
    ) -> Result<(), EngineError> {
        let (Some(clip), Some(write)) = (self.selected_clip(), write) else {
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

    /// Resolve a scalar edit into the payload and where it lands, or `None`
    /// when the property is not reachable or the component does not fit its
    /// kind.
    fn param_write(
        &self,
        param: ClipParam,
        component: Option<u8>,
        value: f32,
    ) -> Option<ParamWrite> {
        // A non-finite value is dropped rather than clamped: it is a broken
        // input, not a value at a bound.
        if !value.is_finite() {
            return None;
        }
        self.param_landing(param, |current, range| {
            let clamped = range.clamp(value);
            match (current, component) {
                (ParamValue::Scalar(_), None) => Some(ParamValue::Scalar(clamped)),
                (ParamValue::Vec2(mut v), Some(index)) => {
                    *v.get_mut(index as usize)? = clamped;
                    Some(ParamValue::Vec2(v))
                }
                (ParamValue::Vec3(mut v), Some(index)) => {
                    *v.get_mut(index as usize)? = clamped;
                    Some(ParamValue::Vec3(v))
                }
                _ => None,
            }
        })
    }

    /// Resolve an edit into the payload and where it lands. `payload` builds
    /// the value from what the property reads now and the range the model
    /// enforces, or returns `None` when the edit does not fit the property.
    ///
    /// Reads the selected clip from the projection, so a stepper drag resolves
    /// its payload without waiting on whatever holds the session.
    fn param_landing(
        &self,
        param: ClipParam,
        payload: impl FnOnce(ParamValue, aria_models::ParamRange) -> Option<ParamValue>,
    ) -> Option<ParamWrite> {
        let clip = self.selected_model()?;
        let row = self.key_param(param)?;
        let spec = &row.spec;
        let start = clip.timeline.start.value;
        let last = (clip.timeline.end_tick() - 1).max(start);
        let local = self.read_tick(clip);
        let payload = payload(spec.sample(clip, local)?, spec.range)?;
        // The row reads the clip's nearest frame when the playhead is off it,
        // so an edit there keys that frame rather than flattening the curve.
        let at = self.playhead.value.clamp(start, last);
        let animated = !row.marks.is_empty();
        Some(ParamWrite {
            value: payload,
            at: animated.then(|| RationalTime::new(at, self.playhead.rate)),
            easing: spec.easing_at(clip, local).unwrap_or(Easing::Linear),
        })
    }
}

/// A resolved edit: what to write, where, and with which curve leaving it.
struct ParamWrite {
    value: ParamValue,
    /// The frame the keyframe lands on, `None` when the edit is a constant.
    at: Option<RationalTime>,
    easing: Easing,
}
