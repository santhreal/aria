//! [`Clip`] timeline placement handles.

use aria_models::{
    AnimationRef, AnimationSlot, ClipId, ClipParam, ClipTransform, CropRect, Easing, Generator,
    ParamValue, RationalTime, ShapeParam,
};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList, PyTuple};

use crate::content::apply_text_style;
use crate::convert::{
    clip_key_time, parse_color, parse_easing, parse_keyframe_pairs, seconds, span, speed_from_f64,
    speed_to_f64, ticks, time_at,
};
use crate::effect::Effect;
use crate::errors::{AriaError, model_err};
use crate::media::Media;
use crate::project::Project;
use crate::slot::Slot;
use crate::track::Track;

// The Python surface is the `#[pymethods]` block below: pyo3 permits exactly one
// per class, so it holds every signature and each body lives in the topic child
// it belongs to. Each child opens with `use super::*`.
#[path = "clip_place.rs"]
mod clip_place;
#[path = "clip_transform.rs"]
mod clip_transform;
#[path = "clip_anim.rs"]
mod clip_anim;
#[path = "clip_audio.rs"]
mod clip_audio;
#[path = "clip_look.rs"]
mod clip_look;
#[path = "clip_params.rs"]
mod clip_params;
#[path = "clip_retime.rs"]
mod clip_retime;

// The parameter helpers are shared by the children above, so the parent
// re-exports them into its own namespace for `use super::*` to pick up.
use clip_params::*;

/// A clip placement on a track.
#[pyclass(unsendable)]
pub struct Clip {
    project: Py<Project>,
    id: ClipId,
}

impl Clip {
    pub(crate) fn new(project: Py<Project>, id: ClipId) -> Self {
        Self { project, id }
    }

    pub(crate) fn id(&self) -> ClipId {
        self.id
    }

    pub(crate) fn with_project<F, R>(&self, py: Python, f: F) -> PyResult<R>
    where
        F: FnOnce(&mut Project) -> PyResult<R>,
    {
        let mut project = self.project.bind(py).borrow_mut();
        if project.model().clip(self.id).is_none() {
            return Err(AriaError::new_err("stale clip handle"));
        }
        f(&mut project)
    }
    pub(crate) fn with_project_ref<F, R>(&self, py: Python, f: F) -> PyResult<R>
    where
        F: FnOnce(&Project) -> PyResult<R>,
    {
        let project = self.project.bind(py).borrow();
        if project.model().clip(self.id).is_none() {
            return Err(AriaError::new_err("stale clip handle"));
        }
        f(&project)
    }


    pub(crate) fn require(project: &Project, id: ClipId) -> PyResult<&aria_models::Clip> {
        project
            .model()
            .clip(id)
            .ok_or_else(|| AriaError::new_err("stale clip handle"))
    }

    /// `(start, duration)` of the clip in timeline ticks.
    pub(crate) fn span_ticks(project: &Project, id: ClipId) -> PyResult<(i64, i64)> {
        let tl = Self::require(project, id)?.timeline;
        Ok((tl.start.value, tl.duration.value))
    }

    fn set_transform_constant(
        project: &mut Project,
        id: ClipId,
        transform: ClipTransform,
    ) -> PyResult<()> {
        project
            .model_mut()
            .set_transform(id, transform, None)
            .map_err(model_err)
    }

    fn current_transform(project: &Project, id: ClipId) -> PyResult<ClipTransform> {
        let clip = Self::require(project, id)?;
        Ok(clip.transform.sample(0))
    }
}

#[pymethods]
impl Clip {
    #[getter]
    fn start(&self, py: Python) -> PyResult<f64> {
        clip_place::start(self, py)
    }

    #[getter]
    fn end(&self, py: Python) -> PyResult<f64> {
        clip_place::end(self, py)
    }

    #[getter]
    fn duration(&self, py: Python) -> PyResult<f64> {
        clip_place::duration(self, py)
    }

    #[getter]
    fn track(&self, py: Python) -> PyResult<Track> {
        clip_place::track(self, py)
    }

    #[getter]
    fn media(&self, py: Python) -> PyResult<Option<Media>> {
        clip_place::media(self, py)
    }

    #[getter]
    fn source_start(&self, py: Python) -> PyResult<Option<f64>> {
        clip_place::source_start(self, py)
    }

    #[getter]
    fn source_duration(&self, py: Python) -> PyResult<Option<f64>> {
        clip_place::source_duration(self, py)
    }

    #[getter]
    fn speed(&self, py: Python) -> PyResult<f64> {
        clip_place::speed(self, py)
    }

    #[getter]
    fn reversed(&self, py: Python) -> PyResult<bool> {
        clip_place::reversed(self, py)
    }

    fn split(&self, py: Python, at: f64) -> PyResult<Clip> {
        clip_place::split(self, py, at)
    }

    #[pyo3(signature = (start = None, end = None))]
    fn trim(&self, py: Python, start: Option<f64>, end: Option<f64>) -> PyResult<()> {
        clip_place::trim(self, py, start, end)
    }

    #[pyo3(name = "move", signature = (start, track = None))]
    fn move_(&self, py: Python, start: f64, track: Option<&Track>) -> PyResult<()> {
        clip_place::move_(self, py, start, track)
    }

    fn delete(&self, py: Python) -> PyResult<()> {
        clip_place::delete(self, py)
    }

    fn ripple_delete(&self, py: Python) -> PyResult<()> {
        clip_place::ripple_delete(self, py)
    }

    #[getter]
    fn position(&self, py: Python) -> PyResult<(f32, f32)> {
        clip_transform::position(self, py)
    }

    #[setter]
    fn set_position(&self, py: Python, value: (f32, f32)) -> PyResult<()> {
        clip_transform::set_position(self, py, value)
    }

    #[getter]
    fn anchor(&self, py: Python) -> PyResult<(f32, f32)> {
        clip_transform::anchor(self, py)
    }

    #[setter]
    fn set_anchor(&self, py: Python, value: (f32, f32)) -> PyResult<()> {
        clip_transform::set_anchor(self, py, value)
    }

    #[getter]
    fn scale(&self, py: Python) -> PyResult<f32> {
        clip_transform::scale(self, py)
    }

    #[setter]
    fn set_scale(&self, py: Python, value: f32) -> PyResult<()> {
        clip_transform::set_scale(self, py, value)
    }

    #[getter]
    fn rotation(&self, py: Python) -> PyResult<f32> {
        clip_transform::rotation(self, py)
    }

    #[setter]
    fn set_rotation(&self, py: Python, value: f32) -> PyResult<()> {
        clip_transform::set_rotation(self, py, value)
    }

    #[getter]
    fn opacity(&self, py: Python) -> PyResult<f32> {
        clip_transform::opacity(self, py)
    }

    #[setter]
    fn set_opacity(&self, py: Python, value: f32) -> PyResult<()> {
        clip_transform::set_opacity(self, py, value)
    }

    #[getter]
    fn animation_in(&self, py: Python) -> PyResult<Option<String>> {
        clip_anim::animation_in(self, py)
    }

    #[getter]
    fn animation_out(&self, py: Python) -> PyResult<Option<String>> {
        clip_anim::animation_out(self, py)
    }

    #[getter]
    fn animation_combo(&self, py: Python) -> PyResult<Option<String>> {
        clip_anim::animation_combo(self, py)
    }

    #[pyo3(signature = (slot, animation_id=None))]
    fn set_animation(&self, py: Python, slot: &str, animation_id: Option<&str>) -> PyResult<()> {
        clip_anim::set_animation(self, py, slot, animation_id)
    }

    fn transform_at(&self, py: Python, t: f64) -> PyResult<((f32, f32), f32, f32, f32)> {
        clip_transform::transform_at(self, py, t)
    }

    #[pyo3(signature = (**kwargs))]
    fn animate(&self, py: Python, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
        clip_anim::animate(self, py, kwargs)
    }

    #[pyo3(signature = (name, *, at))]
    fn remove_keyframe(&self, py: Python, name: &str, at: f64) -> PyResult<()> {
        clip_anim::remove_keyframe(self, py, name, at)
    }

    #[pyo3(signature = (*names))]
    fn clear_animation(&self, py: Python, names: &Bound<'_, PyTuple>) -> PyResult<()> {
        clip_anim::clear_animation(self, py, names)
    }

    #[getter]
    fn volume(&self, py: Python) -> PyResult<f32> {
        clip_audio::volume(self, py)
    }

    #[setter]
    fn set_volume(&self, py: Python, value: f32) -> PyResult<()> {
        clip_audio::set_volume(self, py, value)
    }

    #[getter]
    fn fade_in(&self, py: Python) -> PyResult<f64> {
        clip_audio::fade_in(self, py)
    }

    #[setter]
    fn set_fade_in(&self, py: Python, value: f64) -> PyResult<()> {
        clip_audio::set_fade_in(self, py, value)
    }

    #[getter]
    fn fade_out(&self, py: Python) -> PyResult<f64> {
        clip_audio::fade_out(self, py)
    }

    #[setter]
    fn set_fade_out(&self, py: Python, value: f64) -> PyResult<()> {
        clip_audio::set_fade_out(self, py, value)
    }

    #[pyo3(signature = (factor, reverse = false))]
    fn set_speed(&self, py: Python, factor: f64, reverse: bool) -> PyResult<()> {
        clip_audio::set_speed(self, py, factor, reverse)
    }

    #[pyo3(signature = (x = 0.0, y = 0.0, w = 1.0, h = 1.0, flip_h = false, flip_v = false))]
    #[allow(clippy::too_many_arguments)]
    fn crop(
        &self,
        py: Python,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        flip_h: bool,
        flip_v: bool,
    ) -> PyResult<()> {
        clip_transform::crop(self, py, x, y, w, h, flip_h, flip_v)
    }

    #[pyo3(signature = (effect_id, **params))]
    fn add_effect(
        &self,
        py: Python,
        effect_id: &str,
        params: Option<&Bound<'_, PyDict>>,
    ) -> PyResult<Effect> {
        clip_look::add_effect(self, py, effect_id, params)
    }

    #[getter]
    fn effects(&self, py: Python) -> PyResult<Vec<Effect>> {
        clip_look::effects(self, py)
    }

    fn remove_effect(&self, py: Python, effect: &Effect) -> PyResult<()> {
        clip_look::remove_effect(self, py, effect)
    }

    #[pyo3(signature = (transition_id, duration = None))]
    fn transition(&self, py: Python, transition_id: &str, duration: Option<f64>) -> PyResult<()> {
        clip_look::transition(self, py, transition_id, duration)
    }

    fn remove_transition(&self, py: Python) -> PyResult<()> {
        clip_look::remove_transition(self, py)
    }

    #[getter]
    fn text(&self, py: Python) -> PyResult<String> {
        clip_look::text(self, py)
    }

    #[setter]
    fn set_text(&self, py: Python, content: String) -> PyResult<()> {
        clip_look::set_text(self, py, content)
    }

    #[pyo3(signature = (**kwargs))]
    fn set_style(&self, py: Python, kwargs: Option<&Bound<'_, PyDict>>) -> PyResult<()> {
        clip_look::set_style(self, py, kwargs)
    }

    /// The clip's speed ramp as `[(position, speed), …]`, or `None` when it
    /// plays at one rate. Positions run 0.0…1.0 across the clip.
    #[getter]
    fn speed_curve(&self, py: Python) -> PyResult<Option<Vec<(f64, f32)>>> {
        clip_retime::speed_curve(self, py)
    }

    /// The built-in preset the ramp matches, or `None` for a flat or
    /// hand-written one. See `aria.speed_presets()`.
    #[getter]
    fn speed_preset(&self, py: Python) -> PyResult<Option<String>> {
        clip_retime::speed_preset(self, py)
    }

    /// Set the ramp: a preset name, `[(position, speed[, easing]), …]`, or
    /// `None` to play at one rate. The clip's timeline length follows the ramp.
    #[pyo3(signature = (curve = None))]
    fn set_speed_curve(&self, py: Python, curve: Option<&Bound<'_, PyAny>>) -> PyResult<()> {
        clip_retime::set_speed_curve(self, py, curve)
    }

    /// Play a different file, keeping placement, transform, effects and ramp.
    #[pyo3(signature = (media, start = None))]
    fn set_media(&self, py: Python, media: &Media, start: Option<f64>) -> PyResult<()> {
        clip_retime::set_media(self, py, media, start)
    }

    /// The template slot this clip is, or `None`.
    #[getter]
    fn replaceable(&self, py: Python) -> PyResult<Option<Slot>> {
        clip_retime::replaceable(self, py)
    }

    #[setter]
    fn set_replaceable(&self, py: Python, slot: Option<Slot>) -> PyResult<()> {
        clip_retime::set_replaceable(self, py, slot)
    }

    /// Whether a template may rewrite this text clip's words.
    #[getter]
    fn text_editable(&self, py: Python) -> PyResult<bool> {
        clip_retime::text_editable(self, py)
    }

    #[setter]
    fn set_text_editable(&self, py: Python, editable: bool) -> PyResult<()> {
        clip_retime::set_text_editable(self, py, editable)
    }

    /// Link this clip with `others` so they move, trim and delete together.
    fn link_with(&self, py: Python, others: &Bound<'_, PyAny>) -> PyResult<()> {
        clip_retime::link_with(self, py, others)
    }

    /// Dissolve this clip's link group.
    fn unlink(&self, py: Python) -> PyResult<()> {
        clip_retime::unlink(self, py)
    }

    /// The other clips linked to this one.
    #[getter]
    fn linked(&self, py: Python) -> PyResult<Vec<Clip>> {
        clip_retime::linked(self, py)
    }

    fn __repr__(&self, py: Python) -> PyResult<String> {
        clip_look::repr(self, py)
    }
}