//! Transition commands: a timed blend at the cut between two abutting clips.
//!
//! A transition is keyed on the clip on the outgoing (left) side of the cut, so
//! every setter here addresses the selected clip's *right* junction. The engine
//! refuses a junction that does not abut, a lane that cannot carry one, and a
//! transition of the other kind, so the projected `transition_media` gates the
//! surface instead of a refused command: a cut between pictures takes a catalog
//! blend, a cut between sounds an [`AudioTransition`].

use aria_commands::{Command, EditCommand};
use aria_engine::EngineError;
use aria_models::{
    AudioTransition, Catalog, ClipId, TransitionAlignment, TransitionMedia, TransitionSpec,
    transition_catalog,
};

use super::AppState;
use super::decline::Decline;
use crate::types::UiSlots as _;

/// The smallest and largest transition window the UI will send, in ticks. The
/// model clamps to at least one tick; the upper bound keeps a drag from opening a
/// window longer than either side of the cut can plausibly cover.
pub const MIN_TRANSITION_TICKS: i64 = 2;
pub const MAX_TRANSITION_TICKS: i64 = 600;

/// The step one nudge of the duration control takes, in ticks.
pub const TRANSITION_STEP_TICKS: i64 = 6;

/// Every picture transition the model can render, shipped and registered, for
/// a catalog list.
pub fn transition_choices() -> Catalog<TransitionSpec> {
    transition_catalog()
}

/// The id Ctrl+T (picture) or Ctrl+Shift+T (audio) applies, or `None` when no
/// picture transition is registered.
pub fn default_transition(media: TransitionMedia) -> Option<String> {
    match media {
        TransitionMedia::Picture => transition_catalog().first().map(|spec| spec.id.clone()),
        TransitionMedia::Audio => Some(AudioTransition::DEFAULT.id().to_owned()),
    }
}

/// The window a wedge edge drag implies.
///
/// A centered wedge moves both sides around the cut, so pulling one edge outward
/// by `delta_ticks` changes the window by twice that; a start-aligned or
/// end-aligned wedge is anchored at the cut on one side, so the dragged edge
/// changes the window 1:1. A drag past the bounds stalls at them rather than
/// being refused.
pub fn transition_ticks_after_drag(
    current: i64,
    delta_ticks: i64,
    right_edge: bool,
    alignment: TransitionAlignment,
) -> i64 {
    // `-delta_ticks` panics on `i64::MIN`; a pointer delta never reaches it, but a
    // wedge drag has no business being the one arithmetic in the file that can.
    let outward = if right_edge {
        delta_ticks
    } else {
        delta_ticks.saturating_neg()
    };
    let step = match alignment {
        TransitionAlignment::Center => outward.saturating_mul(2),
        TransitionAlignment::Start | TransitionAlignment::End => outward,
    };
    current
        .saturating_add(step)
        .clamp(MIN_TRANSITION_TICKS, MAX_TRANSITION_TICKS)
}

impl AppState {
    /// Which kind of transition the clip's right junction takes, or `None`
    /// when it takes none.
    pub fn clip_transition_media(&self, clip: ClipId) -> Option<TransitionMedia> {
        self.ui_clip_ref(clip)?.transition_media
    }

    /// The catalog id of the transition at this clip's right junction.
    pub fn clip_transition_id(&self, clip: ClipId) -> Option<String> {
        self.ui_clip_ref(clip)?
            .transition
            .as_ref()
            .map(|transition| transition.id.to_string())
    }

    /// The catalog id of the transition at the selected clip's right junction.
    pub fn selected_transition_id(&self) -> Option<String> {
        self.clip_transition_id(self.selected_clip()?)
    }

    /// The window length of the transition at this clip's junction, in ticks.
    pub fn clip_transition_ticks(&self, clip: ClipId) -> Option<i64> {
        self.ui_clip_ref(clip)?
            .transition
            .as_ref()
            .map(|transition| transition.duration_ticks)
    }

    /// The window length of the transition at the selected junction, in ticks.
    pub fn selected_transition_ticks(&self) -> Option<i64> {
        self.clip_transition_ticks(self.selected_clip()?)
    }

    /// How the transition at this clip's junction sits relative to the cut.
    pub fn clip_transition_alignment(&self, clip: ClipId) -> Option<TransitionAlignment> {
        self.ui_clip_ref(clip)?
            .transition
            .as_ref()
            .map(|transition| transition.alignment)
    }

    /// How the transition at the selected junction sits relative to the cut.
    pub fn selected_transition_alignment(&self) -> Option<TransitionAlignment> {
        self.clip_transition_alignment(self.selected_clip()?)
    }

    /// Apply (or replace) a transition at this clip's right junction.
    ///
    /// Declines an unknown id, a junction that cannot carry one, and a
    /// transition of the other kind: a stale surface must not raise an error the
    /// operator did not cause, and it must not swallow the press either, so each
    /// case names itself in the status line.
    pub fn set_clip_transition(
        &mut self,
        clip: ClipId,
        transition_id: &str,
    ) -> Result<(), EngineError> {
        let Some(media) = self.clip_transition_media(clip) else {
            self.decline(Decline::NoAbuttingCut);
            return Ok(());
        };
        let picture = aria_models::transition_spec(transition_id).is_some();
        let sound = AudioTransition::from_id(transition_id).is_some();
        let refused = match media {
            TransitionMedia::Picture if sound => Some(Decline::PictureCut),
            TransitionMedia::Audio if picture => Some(Decline::SoundCut),
            TransitionMedia::Picture if !picture => Some(Decline::UnknownTransition),
            TransitionMedia::Audio if !sound => Some(Decline::UnknownTransition),
            _ => None,
        };
        if let Some(why) = refused {
            self.decline(why);
            return Ok(());
        }
        // The engine upserts, so a second id at the same junction replaces the
        // first rather than stacking.
        self.apply_command(Command::Edit(EditCommand::AddTransition {
            clip,
            transition_id: transition_id.to_string(),
        }))?;
        Ok(())
    }

    pub fn set_selected_transition(&mut self, transition_id: &str) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.set_clip_transition(clip, transition_id)
    }

    /// Remove the transition at this clip's right junction, or say there is
    /// none.
    pub fn clear_clip_transition(&mut self, clip: ClipId) -> Result<(), EngineError> {
        if self.clip_transition_ticks(clip).is_none() {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::RemoveTransition { clip }))?;
        Ok(())
    }

    pub fn clear_selected_transition(&mut self) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.clear_clip_transition(clip)
    }

    /// Set the transition window length, clamped to the UI bounds. Says so when
    /// the junction carries no transition; stays silent when the length would
    /// not change, because a drag that ends where it started is not a press that
    /// found nothing, and it costs no undo entry either.
    pub fn set_clip_transition_ticks(
        &mut self,
        clip: ClipId,
        ticks: i64,
    ) -> Result<(), EngineError> {
        let Some(current) = self.clip_transition_ticks(clip) else {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        };
        let adjacent_limit = self
            .engine
            .lock()
            .project()
            .transition_duration_limit(clip)
            .map_err(EngineError::Model)?;
        let upper = MAX_TRANSITION_TICKS.min(adjacent_limit);
        let duration = ticks.clamp(MIN_TRANSITION_TICKS.min(upper), upper);
        if duration == current {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetTransition { clip, duration }))?;
        Ok(())
    }

    pub fn set_selected_transition_ticks(&mut self, ticks: i64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.set_clip_transition_ticks(clip, ticks)
    }

    /// Widen or narrow the transition window by `delta` ticks.
    pub fn nudge_selected_transition_ticks(&mut self, delta: i64) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        let Some(current) = self.clip_transition_ticks(clip) else {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        };
        self.set_clip_transition_ticks(clip, current + delta)
    }

    /// Set how the transition window at `clip`'s right junction sits relative
    /// to the cut.
    pub fn set_clip_transition_alignment(
        &mut self,
        clip: ClipId,
        alignment: TransitionAlignment,
    ) -> Result<(), EngineError> {
        let Some(current) = self.clip_transition_alignment(clip) else {
            self.decline(Decline::NoTransitionAtCut);
            return Ok(());
        };
        if current == alignment {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetTransitionAlignment {
            clip,
            alignment,
        }))?;
        Ok(())
    }

    pub fn set_selected_transition_alignment(
        &mut self,
        alignment: TransitionAlignment,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip_or_decline() else {
            return Ok(());
        };
        self.set_clip_transition_alignment(clip, alignment)
    }

    fn modify_selected_transition(
        &mut self,
        edit: impl FnOnce(
            &mut aria_models::Transition,
            &TransitionSpec,
        ) -> Result<(), aria_models::ModelError>,
    ) -> Result<(), EngineError> {
        let Some(clip) = self.selected_clip() else {
            return Ok(());
        };
        let Some(mut transition) = self.engine.lock().project().transition_at(clip).cloned() else {
            return Ok(());
        };
        let Some(spec) = aria_models::transition_spec(&transition.transition_id) else {
            return Ok(());
        };
        let before = transition.clone();
        if edit(&mut transition, &spec).is_err() || transition == before {
            return Ok(());
        }
        self.apply_command(Command::Edit(EditCommand::SetTransitionState {
            clip,
            transition,
        }))?;
        Ok(())
    }

    /// Current value of a scalar or angle parameter on the selected clip's transition.
    pub fn selected_transition_param(&self, param: usize) -> Option<f32> {
        self.selected_ui_clip_ref()?
            .transition
            .as_ref()?
            .scalar(param)
    }

    /// Set a scalar or angle parameter on the selected clip's transition.
    pub fn set_selected_transition_param(
        &mut self,
        param: usize,
        value: f32,
    ) -> Result<(), EngineError> {
        if !value.is_finite() {
            return Ok(());
        }
        self.modify_selected_transition(|transition, spec| {
            let pspec = spec
                .params
                .get(param)
                .ok_or_else(|| aria_models::ModelError::InvalidParam("missing slot".into()))?;
            if !matches!(
                pspec.kind,
                aria_models::EffectParamKind::Scalar | aria_models::EffectParamKind::Angle
            ) {
                return Err(aria_models::ModelError::InvalidParam("not scalar".into()));
            }
            transition.set_param_constant(param, value.clamp(pspec.min, pspec.max))
        })
    }

    /// Step a scalar or angle parameter on the selected clip's transition by `by`.
    pub fn nudge_selected_transition_param(
        &mut self,
        param: usize,
        by: f32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_transition_param(param) else {
            return Ok(());
        };
        self.set_selected_transition_param(param, current + by)
    }

    /// Current value of an integer parameter on the selected clip's transition.
    pub fn selected_transition_int(&self, param: usize) -> Option<i32> {
        self.selected_ui_clip_ref()?.transition.as_ref()?.int(param)
    }

    /// Set an integer parameter on the selected clip's transition.
    pub fn set_selected_transition_int(
        &mut self,
        param: usize,
        value: i32,
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, spec| {
            let pspec = spec
                .params
                .get(param)
                .ok_or_else(|| aria_models::ModelError::InvalidParam("missing slot".into()))?;
            transition.set_int_param(param, i64::from(value).clamp(pspec.int_min, pspec.int_max))
        })
    }

    /// Step an integer parameter on the selected clip's transition by `by`.
    pub fn nudge_selected_transition_int(
        &mut self,
        param: usize,
        by: i32,
    ) -> Result<(), EngineError> {
        let Some(current) = self.selected_transition_int(param) else {
            return Ok(());
        };
        self.set_selected_transition_int(param, current.saturating_add(by))
    }

    /// Current value of a boolean parameter on the selected clip's transition.
    pub fn selected_transition_bool(&self, param: usize) -> Option<bool> {
        self.selected_ui_clip_ref()?
            .transition
            .as_ref()?
            .boolean(param)
    }

    /// Toggle a boolean parameter on the selected clip's transition.
    pub fn toggle_selected_transition_bool(&mut self, param: usize) -> Result<(), EngineError> {
        let Some(current) = self.selected_transition_bool(param) else {
            return Ok(());
        };
        self.modify_selected_transition(|transition, _spec| {
            transition.set_bool_param(param, !current)
        })
    }

    /// Current value of a choice parameter on the selected clip's transition.
    pub fn selected_transition_choice(&self, param: usize) -> Option<String> {
        self.selected_ui_clip_ref()?
            .transition
            .as_ref()?
            .choice(param)
            .map(str::to_owned)
    }

    /// Set a choice parameter on the selected clip's transition.
    pub fn set_selected_transition_choice(
        &mut self,
        param: usize,
        choice: &str,
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, _spec| {
            transition.set_choice_param(param, choice)
        })
    }

    /// Current value of an 8-bit RGBA colour parameter on the selected clip's transition.
    pub fn selected_transition_color(&self, param: usize) -> Option<[u8; 4]> {
        self.selected_ui_clip_ref()?
            .transition
            .as_ref()?
            .color(param)
    }

    /// Set an 8-bit RGBA colour parameter on the selected clip's transition.
    pub fn set_selected_transition_color(
        &mut self,
        param: usize,
        rgba: [u8; 4],
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, _spec| transition.set_color_param(param, rgba))
    }

    /// Current value of a 2D vector or size parameter on the selected clip's transition.
    pub fn selected_transition_vec2(&self, param: usize) -> Option<[f32; 2]> {
        self.selected_ui_clip_ref()?
            .transition
            .as_ref()?
            .vec2(param)
    }

    /// Set a 2D vector or size parameter on the selected clip's transition.
    pub fn set_selected_transition_vec2(
        &mut self,
        param: usize,
        value: [f32; 2],
    ) -> Result<(), EngineError> {
        if !value.iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        self.modify_selected_transition(|transition, spec| {
            let pspec = spec
                .params
                .get(param)
                .ok_or_else(|| aria_models::ModelError::InvalidParam("missing slot".into()))?;
            transition.set_vec2_param(
                param,
                [
                    value[0].clamp(pspec.min, pspec.max),
                    value[1].clamp(pspec.min, pspec.max),
                ],
            )
        })
    }

    /// Step one axis (`0` = X, `1` = Y) of a 2D vector or size parameter on the
    /// selected clip's transition.
    pub fn nudge_selected_transition_vec2(
        &mut self,
        param: usize,
        axis: usize,
        by: f32,
    ) -> Result<(), EngineError> {
        let Some(mut current) = self.selected_transition_vec2(param) else {
            return Ok(());
        };
        let Some(slot) = current.get_mut(axis) else {
            return Ok(());
        };
        *slot += by;
        self.set_selected_transition_vec2(param, current)
    }

    /// Current value of a 3D point parameter on the selected clip's transition.
    pub fn selected_transition_point3(&self, param: usize) -> Option<[f32; 3]> {
        self.selected_ui_clip_ref()?
            .transition
            .as_ref()?
            .point3(param)
    }

    /// Set a 3D point parameter on the selected clip's transition.
    pub fn set_selected_transition_point3(
        &mut self,
        param: usize,
        value: [f32; 3],
    ) -> Result<(), EngineError> {
        if !value.iter().all(|v| v.is_finite()) {
            return Ok(());
        }
        self.modify_selected_transition(|transition, spec| {
            let pspec = spec
                .params
                .get(param)
                .ok_or_else(|| aria_models::ModelError::InvalidParam("missing slot".into()))?;
            transition.set_vec3_param(
                param,
                [
                    value[0].clamp(pspec.min, pspec.max),
                    value[1].clamp(pspec.min, pspec.max),
                    value[2].clamp(pspec.min, pspec.max),
                ],
            )
        })
    }

    /// Step one axis (`0` = X, `1` = Y, `2` = Z) of a 3D point parameter on the
    /// selected clip's transition.
    pub fn nudge_selected_transition_point3(
        &mut self,
        param: usize,
        axis: usize,
        by: f32,
    ) -> Result<(), EngineError> {
        let Some(mut current) = self.selected_transition_point3(param) else {
            return Ok(());
        };
        let Some(slot) = current.get_mut(axis) else {
            return Ok(());
        };
        *slot += by;
        self.set_selected_transition_point3(param, current)
    }

    /// Set one axis of a 2D vector, size or 3D point parameter on the selected
    /// clip's transition, keeping the other axes.
    pub fn set_selected_transition_axis(
        &mut self,
        param: usize,
        axis: usize,
        value: f32,
    ) -> Result<(), EngineError> {
        if let Some(mut current) = self.selected_transition_vec2(param) {
            let Some(slot) = current.get_mut(axis) else {
                return Ok(());
            };
            *slot = value;
            return self.set_selected_transition_vec2(param, current);
        }
        let Some(mut current) = self.selected_transition_point3(param) else {
            return Ok(());
        };
        let Some(slot) = current.get_mut(axis) else {
            return Ok(());
        };
        *slot = value;
        self.set_selected_transition_point3(param, current)
    }

    /// Set a 1D curve parameter on the selected clip's transition.
    pub fn set_selected_transition_curve(
        &mut self,
        param: usize,
        points: Vec<aria_models::EffectCurvePoint>,
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, _spec| {
            transition.set_curve_param(param, points)
        })
    }

    /// Set a gradient parameter on the selected clip's transition.
    pub fn set_selected_transition_gradient(
        &mut self,
        param: usize,
        stops: Vec<aria_models::EffectGradientStop>,
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, _spec| {
            transition.set_gradient_param(param, stops)
        })
    }

    /// Set a text/font/file parameter on the selected clip's transition.
    pub fn set_selected_transition_text(
        &mut self,
        param: usize,
        text: &str,
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, _spec| transition.set_text_param(param, text))
    }

    /// Set a layer reference parameter on the selected clip's transition.
    pub fn set_selected_transition_layer(
        &mut self,
        param: usize,
        layer: Option<ClipId>,
    ) -> Result<(), EngineError> {
        self.modify_selected_transition(|transition, _spec| {
            transition.set_layer_param(param, layer)
        })
    }

    /// Apply a [`crate::surfaces::clip_pick::TransitionSlotEdit`] to parameter
    /// `param` on the selected clip's transition.
    pub fn edit_selected_transition_slot(
        &mut self,
        param: usize,
        edit: crate::surfaces::clip_pick::TransitionSlotEdit,
    ) -> Result<(), EngineError> {
        use crate::surfaces::clip_pick::TransitionSlotEdit;
        match edit {
            TransitionSlotEdit::NudgeScalar(by) => self.nudge_selected_transition_param(param, by),
            TransitionSlotEdit::NudgeInt(by) => self.nudge_selected_transition_int(param, by),
            TransitionSlotEdit::ToggleBool => self.toggle_selected_transition_bool(param),
            TransitionSlotEdit::Choice(choice) => {
                self.set_selected_transition_choice(param, &choice)
            }
            TransitionSlotEdit::Color(rgba) => self.set_selected_transition_color(param, rgba),
            TransitionSlotEdit::NudgeVec2 { axis, by } => {
                self.nudge_selected_transition_vec2(param, axis, by)
            }
            TransitionSlotEdit::NudgePoint3 { axis, by } => {
                self.nudge_selected_transition_point3(param, axis, by)
            }
            TransitionSlotEdit::Curve(points) => self.set_selected_transition_curve(param, points),
            TransitionSlotEdit::Gradient(stops) => {
                self.set_selected_transition_gradient(param, stops)
            }
            TransitionSlotEdit::Text(text) => self.set_selected_transition_text(param, &text),
            TransitionSlotEdit::Layer(layer) => self.set_selected_transition_layer(param, layer),
        }
    }
}
