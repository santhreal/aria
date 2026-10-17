//! Animation menu verbs over the selected property rows and keyframes: Add
//! Keyframe, Time-Reverse Keyframes, Add Expression, and the Keyframe Velocity
//! dialog. Every edit is one undo step.

use std::collections::BTreeMap;

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    Clip, ClipId, ClipParam, Easing, Keyframe, MAX_TEMPORAL_INFLUENCE, MIN_TEMPORAL_INFLUENCE,
    Param, ParamValue, RationalTime, SpatialTangents, TemporalEase,
};
use gpui::SharedString;

use super::clip_keys::{KeyParam, key_params_of};
use super::property_rows::SelectedKeyframe;
use super::{AppState, DialogKind};

/// Which side of a keyframe a Keyframe Velocity field edits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VelocitySide {
    Incoming,
    Outgoing,
}

/// State of the Keyframe Velocity dialog (Animation > Keyframe Velocity, or a
/// double-click on a keyframe).
#[derive(Debug, Clone, PartialEq)]
pub struct KeyframeVelocityForm {
    /// The keyframes Apply writes: the selected ones, else the ones under the
    /// playhead on the selected clip. Shared, because the dialog's draw takes a
    /// copy of the form each frame.
    pub targets: std::sync::Arc<[SelectedKeyframe]>,
    pub incoming: TemporalEase,
    pub outgoing: TemporalEase,
    /// Both sides keep one speed: a speed typed on either side is written to
    /// both.
    pub continuous: bool,
    /// Sequence frames per second. The model stores speed per tick; the dialog
    /// shows it per second.
    pub ticks_per_second: f32,
    /// The unit a speed is read in, from the first target's property.
    pub unit: SharedString,
    pub error: Option<SharedString>,
}

impl Default for KeyframeVelocityForm {
    fn default() -> Self {
        Self {
            targets: std::sync::Arc::from([]),
            incoming: TemporalEase::EASY_EASE,
            outgoing: TemporalEase::EASY_EASE,
            continuous: false,
            ticks_per_second: 24.0,
            unit: SharedString::new_static("units/s"),
            error: None,
        }
    }
}

impl KeyframeVelocityForm {
    /// The side's speed in property units per second.
    pub fn speed_per_second(&self, side: VelocitySide) -> f32 {
        self.side(side).speed * self.ticks_per_second
    }

    /// The side's influence as a percentage of its segment.
    pub fn influence_percent(&self, side: VelocitySide) -> f32 {
        self.side(side).influence * 100.0
    }

    fn side(&self, side: VelocitySide) -> TemporalEase {
        match side {
            VelocitySide::Incoming => self.incoming,
            VelocitySide::Outgoing => self.outgoing,
        }
    }

    fn side_mut(&mut self, side: VelocitySide) -> &mut TemporalEase {
        match side {
            VelocitySide::Incoming => &mut self.incoming,
            VelocitySide::Outgoing => &mut self.outgoing,
        }
    }
}

/// The row of `param` on `model`, if the clip has the property.
fn row_of(model: &Clip, audio_lane: bool, param: ClipParam) -> Option<KeyParam> {
    key_params_of(model, audio_lane)
        .iter()
        .find(|row| row.param() == param)
        .cloned()
}

/// The stored keyframes of `row` on `model`, in stored ticks.
fn stored_keys(model: &Clip, row: &KeyParam) -> Vec<Keyframe<ParamValue>> {
    match row.spec.curve(model) {
        Some(Param::Keyframed { keyframes }) => keyframes,
        _ => Vec::new(),
    }
}

/// The shape of a segment played backwards: a Bezier's control polygon
/// mirrored through the segment's centre. A Hold stays a Hold; the jump moves
/// to the other end of the segment, which a keyframe cannot state.
fn reversed_segment(leaving: Easing) -> Easing {
    match leaving.control_points() {
        None => Easing::Hold,
        Some([x1, y1, x2, y2]) => {
            let points = [1.0 - x2, 1.0 - y2, 1.0 - x1, 1.0 - y1];
            if points == [0.0, 0.0, 1.0, 1.0] {
                Easing::Linear
            } else {
                Easing::Bezier { points }
            }
        }
    }
}

/// A keyframe's easing from the shape of the segment it leaves, its two
/// temporal handles, and its roving flag.
fn compose(
    segment: Easing,
    in_ease: Option<TemporalEase>,
    out_ease: Option<TemporalEase>,
    roving: bool,
) -> Easing {
    if in_ease.is_none() && out_ease.is_none() && !roving {
        return segment;
    }
    Easing::Temporal {
        in_ease,
        out_ease,
        roving,
        hold: segment.is_hold(),
        points: segment.control_points().unwrap_or([0.0, 0.0, 1.0, 1.0]),
    }
}

/// A handle with its speed negated: time running backwards turns a rise into
/// a fall.
fn backwards(ease: Option<TemporalEase>) -> Option<TemporalEase> {
    ease.map(|ease| TemporalEase::new(-ease.speed, ease.influence))
}

impl AppState {
    /// The property rows Add Keyframe and Add Expression act on: the rows
    /// selected in the timeline outline, whose clips still exist.
    pub fn selected_property_rows(&self) -> Vec<(ClipId, ClipParam)> {
        self.with_prop_rows(|s| s.graph_params.clone())
            .into_iter()
            .filter(|(clip, _)| self.clip_and_lane_kind(*clip).is_some())
            .collect()
    }

    /// Animation > Add Keyframe: a keyframe at the playhead on every selected
    /// property row, holding the value the property has there. A row whose
    /// clip is not under the playhead is passed over. Returns how many were
    /// written.
    pub fn add_keyframes_on_selected_properties(&mut self) -> Result<usize, EngineError> {
        let at = self.playhead;
        let mut writes = Vec::new();
        for (clip, param) in self.selected_property_rows() {
            let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
                continue;
            };
            let inside = model
                .end()
                .is_ok_and(|end| at.value >= model.start().value && at.value < end.value);
            if !inside {
                continue;
            }
            let Some(row) = row_of(&model, audio_lane, param) else {
                continue;
            };
            let local = model.param_tick(at.value);
            let Some(value) = row.spec.sample(&model, local) else {
                continue;
            };
            // A keyframe already there keeps its shape and its tangents.
            let existing = stored_keys(&model, &row)
                .into_iter()
                .find(|key| key.tick == local);
            writes.push(EditCommand::SetParamKeyframe {
                clip,
                param,
                at,
                value,
                easing: existing.as_ref().map_or(Easing::Linear, |key| key.easing),
                tangents: existing.and_then(|key| key.tangents),
            });
        }
        if writes.is_empty() {
            self.status_message =
                Some("Select a property row of a clip under the playhead first".into());
            return Ok(0);
        }
        let count = writes.len();
        self.one_step(|this| {
            for write in writes {
                this.apply_command(Command::Edit(write))?;
            }
            Ok(())
        })?;
        let noun = if count == 1 { "keyframe" } else { "keyframes" };
        self.status_message = Some(format!("Added {count} {noun} at the playhead"));
        Ok(count)
    }

    /// Animation > Keyframe Assistant > Time-Reverse Keyframes: on each
    /// property with two or more selected keyframes, the selected keyframes
    /// trade places across their span, so the animation plays backwards
    /// between the first and the last. Each keyframe takes its value,
    /// tangents (in and out swapped) and handles (in and out swapped, speed
    /// negated) to its mirrored time, and each segment keeps its shape played
    /// backwards. Returns how many keyframes moved.
    pub fn time_reverse_selected_keyframes(&mut self) -> Result<usize, EngineError> {
        let mut groups: BTreeMap<(ClipId, ClipParam), Vec<i64>> = BTreeMap::new();
        for key in self.selected_timeline_keyframes() {
            groups
                .entry((key.clip, key.param))
                .or_default()
                .push(key.tick);
        }
        let rate = self.playhead.rate;
        let mut removals = Vec::new();
        let mut writes = Vec::new();
        let mut selection = Vec::new();
        for ((clip, param), ticks) in groups {
            let Some((model, audio_lane)) = self.clip_and_lane_kind(clip) else {
                continue;
            };
            let Some(row) = row_of(&model, audio_lane, param) else {
                continue;
            };
            let locals: Vec<i64> = ticks.iter().map(|&tick| model.param_tick(tick)).collect();
            let mut keys: Vec<Keyframe<ParamValue>> = stored_keys(&model, &row)
                .into_iter()
                .filter(|key| locals.contains(&key.tick))
                .collect();
            if keys.len() < 2 {
                continue;
            }
            keys.sort_by_key(|key| key.tick);
            let last = keys.len() - 1;
            let span = keys[0].tick + keys[last].tick;
            let mirrored: Vec<i64> = keys.iter().map(|key| span - key.tick).collect();
            for key in &keys {
                if !mirrored.contains(&key.tick) {
                    removals.push(EditCommand::RemoveParamKeyframe {
                        clip,
                        param,
                        at: RationalTime::new(model.param_timeline_tick(key.tick), rate),
                    });
                }
            }
            for new in 0..=last {
                let old = last - new;
                let source = &keys[old];
                // The segment leaving the new keyframe is the one that arrived
                // at its source, run backwards. The last keyframe sits where the
                // old last one did and keeps the segment leaving the span.
                let segment = if old == 0 {
                    keys[last].easing
                } else {
                    reversed_segment(keys[old - 1].easing)
                };
                let easing = compose(
                    segment,
                    backwards(source.easing.out_ease()),
                    backwards(source.easing.in_ease()),
                    source.easing.is_roving(),
                );
                let tick = model.param_timeline_tick(span - source.tick);
                writes.push(EditCommand::SetParamKeyframe {
                    clip,
                    param,
                    at: RationalTime::new(tick, rate),
                    value: source.value,
                    easing,
                    tangents: source.tangents.map(|t| SpatialTangents {
                        out_t: t.in_t,
                        in_t: t.out_t,
                    }),
                });
                selection.push(SelectedKeyframe { clip, param, tick });
            }
        }
        if writes.is_empty() {
            self.status_message = Some("Select two or more keyframes on one property first".into());
            return Ok(0);
        }
        let count = writes.len();
        self.one_step(|this| {
            for edit in removals.into_iter().chain(writes) {
                this.apply_command(Command::Edit(edit))?;
            }
            Ok(())
        })?;
        self.with_prop_rows_mut(|s| {
            s.selected_keyframes.clear();
            s.selected_keyframes.extend(selection);
        });
        self.status_message = Some(format!("Time-reversed {count} keyframes"));
        Ok(count)
    }

    /// Animation > Add Expression: a driver on the selected property row of
    /// the selected clip, opened for editing. A property that already has one
    /// opens it instead of removing it.
    pub fn add_expression_to_selected_property(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            self.status_message = Some("Select a clip first".into());
            return Ok(());
        };
        let Some(param) = self
            .selected_property_rows()
            .into_iter()
            .find(|(owner, _)| *owner == clip)
            .map(|(_, param)| param)
        else {
            self.status_message = Some("Select a property row of the selected clip first".into());
            return Ok(());
        };
        if self.selected_driver(param).is_some() {
            self.open_driver_draft(param);
            return Ok(());
        }
        self.toggle_driver(param)
    }

    /// Open Keyframe Velocity on the selected keyframes, or the keyframes
    /// under the playhead on the selected clip, filled from the first one's
    /// handles as they play.
    pub fn open_keyframe_velocity(&mut self) {
        let targets = self.ease_target_keyframes();
        let Some(first) = targets.first().copied() else {
            self.status_message = Some("Select a keyframe first".into());
            return;
        };
        let rate = self.store.sequence.frame_rate;
        let ticks_per_second = (rate.num as f32 / rate.den.max(1) as f32).max(1.0);
        let (incoming, outgoing) = self
            .clip_and_lane_kind(first.clip)
            .and_then(|(model, audio_lane)| {
                let row = row_of(&model, audio_lane, first.param)?;
                row.spec
                    .temporal_ease_at(&model, model.param_tick(first.tick))
            })
            .unwrap_or((TemporalEase::EASY_EASE, TemporalEase::EASY_EASE));
        self.keyframe_velocity = KeyframeVelocityForm {
            targets: targets.into(),
            incoming,
            outgoing,
            continuous: (incoming.speed - outgoing.speed).abs() <= f32::EPSILON,
            ticks_per_second,
            unit: SharedString::new_static("units/s"),
            error: None,
        };
        self.open_dialog(DialogKind::KeyframeVelocity);
    }

    /// Set one side's speed, in property units per second. With Continuous on
    /// both sides take it.
    pub fn set_keyframe_velocity_speed(
        &mut self,
        side: VelocitySide,
        per_second: f32,
    ) -> Result<(), String> {
        if !per_second.is_finite() {
            return Err(self.velocity_error("Speed must be a number".into()));
        }
        let form = &mut self.keyframe_velocity;
        let speed = per_second / form.ticks_per_second;
        form.side_mut(side).speed = speed;
        if form.continuous {
            form.incoming.speed = speed;
            form.outgoing.speed = speed;
        }
        form.error = None;
        Ok(())
    }

    /// Set one side's influence, as a percentage of its segment.
    pub fn set_keyframe_velocity_influence(
        &mut self,
        side: VelocitySide,
        percent: f32,
    ) -> Result<(), String> {
        let low = MIN_TEMPORAL_INFLUENCE * 100.0;
        let high = MAX_TEMPORAL_INFLUENCE * 100.0;
        if !percent.is_finite() || !(low..=high).contains(&percent) {
            return Err(self.velocity_error(format!(
                "Influence must be between {low:.1}% and {high:.0}%"
            )));
        }
        let form = &mut self.keyframe_velocity;
        form.side_mut(side).influence = percent / 100.0;
        form.error = None;
        Ok(())
    }

    /// Continuous on or off. Turning it on gives the outgoing side the
    /// incoming speed.
    pub fn toggle_keyframe_velocity_continuous(&mut self) {
        let form = &mut self.keyframe_velocity;
        form.continuous ^= true;
        if form.continuous {
            form.outgoing.speed = form.incoming.speed;
        }
    }

    fn velocity_error(&mut self, reason: String) -> String {
        self.keyframe_velocity.error = Some(reason.clone().into());
        self.status_message = Some(reason.clone());
        reason
    }

    /// Apply: both handles on every target keyframe, as one undo step, and
    /// close the dialog. A Hold keyframe stays Hold.
    pub fn apply_keyframe_velocity(&mut self) {
        if let Some(err) = &self.keyframe_velocity.error {
            self.status_message = Some(err.to_string());
            return;
        }
        let form = self.keyframe_velocity.clone();
        let rate = self.playhead.rate;
        let result = self.one_step(|this| {
            for target in form.targets.iter() {
                let hold = this
                    .clip_and_lane_kind(target.clip)
                    .and_then(|(model, audio_lane)| {
                        row_of(&model, audio_lane, target.param)?
                            .spec
                            .easing_at(&model, model.param_tick(target.tick))
                    })
                    .is_some_and(Easing::is_hold);
                this.apply_command(Command::Edit(EditCommand::SetKeyframeEase {
                    clip: target.clip,
                    param: target.param,
                    at: RationalTime::new(target.tick, rate),
                    in_ease: Some(form.incoming),
                    out_ease: Some(form.outgoing),
                    hold,
                }))?;
            }
            Ok(())
        });
        match result {
            Ok(()) => {
                let count = form.targets.len();
                let noun = if count == 1 { "keyframe" } else { "keyframes" };
                self.status_message = Some(format!("Set the velocity of {count} {noun}"));
                self.open_dialog(DialogKind::None);
            }
            Err(err) => self.report_error(&err),
        }
    }
}
