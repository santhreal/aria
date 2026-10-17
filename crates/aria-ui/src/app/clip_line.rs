//! Dragging the line a clip's bar draws on the timeline: the clip volume on an
//! audio lane, the opacity elsewhere, or the property chosen in the clip's
//! badge menu.
//!
//! A press on a constant line moves the constant. A press on a keyframe dot
//! moves that keyframe, and a press on the line between two keyframes moves
//! both, so the segment keeps its slope. Before the first keyframe and after
//! the last the line is level with it, so a press there moves that keyframe.
//! The travel is measured in the scale the bar draws, so a gain moves in
//! decibels.
//!
//! A drag is one undo step: every move rolls back what the previous move wrote
//! and writes the whole travel again. Escape puts the line back.

use aria_commands::{Command, EditCommand};
use aria_models::{
    ClipId, ClipParam, Easing, ParamKind, ParamRange, ParamSpec, ParamValue, RationalTime,
};
use aria_plugin::{EditGroup, Origin};
use gpui::SharedString;

use super::AppState;
use crate::types::{LineScale, TrimFeedback, UiTrackKind};

/// Where a press on a clip's line landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinePress {
    /// The keyframe at this timeline tick.
    Key(i64),
    /// The line at this timeline tick.
    Line(i64),
}

/// A press held on a clip's line.
pub(crate) struct LineDrag {
    clip: ClipId,
    param: ClipParam,
    /// The scale the bar drew the line in when it was pressed. The travel is
    /// measured in it, and the bar keeps it until release, so a property
    /// whose scale is fitted to its values does not rescale under the pointer.
    scale: LineScale,
    range: ParamRange,
    /// Pixels between the bottom and the top of the scale.
    band: f32,
    held: Held,
    /// The property's row, which the readout prints the value in the units of.
    spec: ParamSpec,
    /// Shown in the drag's readout.
    label: SharedString,
    clip_name: SharedString,
    /// What the open group holds.
    group: Option<EditGroup>,
    /// The travel, in pixels, the open group holds.
    applied: f32,
}

/// What a press found under it.
enum Held {
    Constant(f32),
    /// Each keyframe the drag moves: its timeline tick, the value the press
    /// found, and its easing.
    Keys(Vec<(i64, f32, Easing)>),
}

impl LineDrag {
    /// The values the drag writes after `travel` pixels down the bar, with
    /// the keyframe tick each belongs to.
    fn values(&self, travel: f32) -> Vec<(Option<i64>, f32, Easing)> {
        let lift = -travel / self.band.max(1.0);
        let moved = |value: f32| {
            self.range
                .clamp(self.scale.value(self.scale.fraction(value) + lift))
        };
        match &self.held {
            Held::Constant(value) => vec![(None, moved(*value), Easing::Linear)],
            Held::Keys(keys) => keys
                .iter()
                .map(|&(tick, value, easing)| (Some(tick), moved(value), easing))
                .collect(),
        }
    }

    /// What the readout prints for `value` on `model`: what the property's
    /// row in Effect Controls and on the timeline prints.
    fn readout(&self, model: &aria_models::Clip, value: f32) -> SharedString {
        match crate::surfaces::param_units::of_param(&self.spec.group, self.param) {
            Some(units) => (units.read)(value).into(),
            None => {
                super::property_rows::format_param_value(
                    model,
                    &self.spec,
                    Some(ParamValue::Scalar(value)),
                )
                .0
            }
        }
    }
}

impl AppState {
    /// Whether a press holds a clip's line.
    pub fn clip_line_held(&self) -> bool {
        self.line_drag.is_some()
    }

    /// Press on `clip`'s line at `at`. `band` is the height, in pixels, the
    /// line travels on the bar. Returns whether the line can be dragged: the
    /// property it draws is one number.
    pub fn press_clip_line(&mut self, clip: ClipId, at: LinePress, band: f32) -> bool {
        self.cancel_clip_line();
        let Some(ui) = self.ui_clip_ref(clip) else {
            return false;
        };
        let Some(line) = ui.line.as_ref().filter(|line| line.draggable) else {
            return false;
        };
        let (param, scale, clip_name) = (line.param, line.scale, ui.name.clone());
        let audio = self.store.sequence.tracks.iter().any(|lane| {
            lane.kind == UiTrackKind::Audio && lane.clips.iter().any(|c| c.id == clip.raw())
        });
        let engine = self.engine.lock();
        let Some(model) = engine.project().clip(clip) else {
            return false;
        };
        let Some(spec) = model
            .line_row(param, audio)
            .filter(|spec| spec.kind == ParamKind::Scalar)
        else {
            return false;
        };
        let scalar = |local: i64| match spec.sample(model, local) {
            Some(ParamValue::Scalar(value)) => Some(value),
            _ => None,
        };
        let keys: Vec<(i64, f32, Easing)> = spec
            .keyframes(model)
            .into_iter()
            .filter_map(|(local, easing)| {
                Some((model.param_timeline_tick(local), scalar(local)?, easing))
            })
            .collect();
        let held = if keys.is_empty() {
            let Some(value) = scalar(model.param_tick(model.timeline.start.value)) else {
                return false;
            };
            Held::Constant(value)
        } else {
            let moved: Vec<(i64, f32, Easing)> = match at {
                LinePress::Key(tick) => keys.iter().filter(|key| key.0 == tick).copied().collect(),
                LinePress::Line(tick) => {
                    let after = keys.partition_point(|key| key.0 <= tick);
                    match after {
                        0 => vec![keys[0]],
                        n if n == keys.len() => vec![keys[n - 1]],
                        n => vec![keys[n - 1], keys[n]],
                    }
                }
            };
            if moved.is_empty() {
                return false;
            }
            Held::Keys(moved)
        };
        let range = spec.range;
        let label = SharedString::from(spec.label.to_string());
        drop(engine);
        self.line_drag = Some(LineDrag {
            clip,
            param,
            scale,
            range,
            band,
            held,
            spec,
            label,
            clip_name,
            group: None,
            applied: 0.0,
        });
        true
    }

    /// The pointer is `travel` pixels below where the line was pressed.
    /// Returns whether the line moved.
    pub fn drag_clip_line(&mut self, travel: f32) -> bool {
        let Some(drag) = self.line_drag.as_mut() else {
            return false;
        };
        if travel.to_bits() == drag.applied.to_bits() {
            return false;
        }
        let (clip, param, scale) = (drag.clip, drag.param, drag.scale);
        let writes = drag.values(travel);
        let previous = drag.group.take();
        if let Some(group) = previous {
            group.rollback();
            self.sync_from_engine();
        }
        let rate = self.store.sequence.frame_rate;
        let wrote = self.host.group(&Origin::Editor).and_then(|group| {
            for &(tick, value, easing) in &writes {
                let value = ParamValue::Scalar(value);
                let command = match tick {
                    None => EditCommand::SetParamConstant { clip, param, value },
                    Some(tick) => EditCommand::SetParamKeyframe {
                        clip,
                        param,
                        at: RationalTime::new(tick, rate),
                        value,
                        easing,
                        tangents: None,
                    },
                };
                self.apply_command(Command::Edit(command))?;
            }
            Ok(group)
        });
        let wrote = match wrote {
            Ok(group) => Some(group),
            Err(err) => {
                self.sync_from_engine();
                self.report_error(&err);
                None
            }
        };
        self.pin_line_scale(clip, scale);
        let Some(drag) = self.line_drag.as_mut() else {
            return true;
        };
        drag.applied = if wrote.is_some() { travel } else { 0.0 };
        drag.group = wrote;
        let value = drag
            .values(drag.applied)
            .first()
            .map_or(0.0, |write| write.1);
        let engine = self.engine.lock();
        let delta_label = engine
            .project()
            .clip(clip)
            .map_or_else(SharedString::default, |model| drag.readout(model, value));
        drop(engine);
        self.trim_feedback = Some(TrimFeedback {
            tool_label: drag.label.clone(),
            clip_name: drag.clip_name.clone(),
            delta_ticks: 0,
            delta_label,
            detail_label: SharedString::new_static("Esc puts it back"),
        });
        true
    }

    /// Let go of the line. A drag keeps what it wrote as one undo step.
    /// Returns whether a press was held.
    pub fn release_clip_line(&mut self) -> bool {
        let Some(drag) = self.line_drag.take() else {
            return false;
        };
        self.trim_feedback = None;
        if let Some(group) = drag.group {
            group.commit();
        }
        // The bar fits its scale to the values it holds again.
        self.projected = None;
        self.sync_from_engine();
        true
    }

    /// Escape during a drag on a line: put it back where the press found it.
    /// Returns whether a press was held.
    pub fn cancel_clip_line(&mut self) -> bool {
        let Some(drag) = self.line_drag.take() else {
            return false;
        };
        self.trim_feedback = None;
        if let Some(group) = drag.group {
            group.rollback();
        }
        self.projected = None;
        self.sync_from_engine();
        true
    }

    /// Keep the scale `clip`'s bar drew its line in when it was pressed.
    fn pin_line_scale(&mut self, clip: ClipId, scale: LineScale) {
        let id = clip.raw();
        if let Some(line) = self
            .store_mut()
            .sequence
            .tracks
            .iter_mut()
            .flat_map(|lane| lane.clips.iter_mut())
            .find(|ui| ui.id == id)
            .and_then(|ui| ui.line.as_mut())
        {
            line.scale = scale;
        }
    }
}

#[cfg(test)]
#[path = "clip_line_tests.rs"]
mod tests;
