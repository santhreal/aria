//! [`Template`] — a project with replaceable slots, and filling one.
//!
//! A template is a finished edit whose media is a placeholder: the timing,
//! transforms, effects and transitions are the author's, the pictures are the
//! caller's. `fill` returns an ordinary [`Project`], so everything else in this
//! module works on the result unchanged.

use std::path::Path;

use aria_core::count::{counted, noun};
use aria_models::{Generator, MediaSource, Pick, Template as Model, TemplateMeta, template_bundle};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyTuple;

use crate::convert::{seconds, time_at};
use crate::errors::{AriaError, model_err};
use crate::media::Media;
use crate::project::Project;
use crate::slot::Slot;

/// A template document (`.ariat`) or an installed bundle (`.ariab`).
#[pyclass(unsendable)]
pub struct Template {
    model: Model,
}

impl Template {
    pub fn from_model(model: Model) -> Self {
        Self { model }
    }

    pub fn model(&self) -> &Model {
        &self.model
    }

    /// Turn one Python pick into a model [`Pick`]. A pick is a `Media` handle,
    /// or `(media, start_seconds)` to take the slot's footage from further in.
    /// The media is copied out of its own project's pool, so a template may be
    /// filled from assets imported anywhere.
    fn pick(py: Python, item: &Bound<'_, PyAny>) -> PyResult<Pick> {
        let (media, start): (Py<Media>, Option<f64>) = if item.is_instance_of::<PyTuple>() {
            let tuple = item.cast::<PyTuple>()?;
            if tuple.len() != 2 {
                return Err(PyValueError::new_err(
                    "a pick tuple is (media, start_seconds)",
                ));
            }
            (
                tuple.get_item(0)?.extract()?,
                Some(tuple.get_item(1)?.extract()?),
            )
        } else {
            (item.extract()?, None)
        };
        let media = media.bind(py).borrow();
        let project = media.project().bind(py).borrow();
        let source: MediaSource = Media::require(&project, media.id())?.clone();
        Ok(match start {
            Some(start) => {
                let rate = source.frame_rate;
                Pick::at(source, time_at(start, rate))
            }
            None => Pick::new(source),
        })
    }
}

#[pymethods]
impl Template {
    /// Load a `.ariat` document. Its sample media is referenced by the path it
    /// had when it was authored; a template moved between machines is loaded
    /// from a bundle instead (see [`Template::install`]).
    #[staticmethod]
    fn load(path: &str) -> PyResult<Self> {
        let model = Model::load_from_file(Path::new(path)).map_err(model_err)?;
        Ok(Self { model })
    }

    /// Extract a `.ariab` bundle into `dest_dir` and load the template it
    /// carries, with its sample media rewritten to the extracted copies.
    #[staticmethod]
    fn install(path: &str, dest_dir: &str) -> PyResult<Self> {
        let model =
            template_bundle::install(Path::new(path), Path::new(dest_dir)).map_err(model_err)?;
        Ok(Self { model })
    }

    #[getter]
    fn name(&self) -> String {
        self.model.meta().name.to_string()
    }

    #[getter]
    fn author(&self) -> Option<String> {
        self.model.meta().author.as_ref().map(|a| a.to_string())
    }

    #[getter]
    fn description(&self) -> Option<String> {
        self.model
            .meta()
            .description
            .as_ref()
            .map(|d| d.to_string())
    }

    #[getter]
    fn tags(&self) -> Vec<String> {
        self.model
            .meta()
            .tags
            .iter()
            .map(|tag| tag.to_string())
            .collect()
    }

    /// Timeline length in seconds. Filling a slot does not change it: a
    /// template's timing is the author's.
    #[getter]
    fn duration(&self) -> f64 {
        let rate = self.model.project().timeline().frame_rate;
        seconds(self.model.duration().value, rate)
    }

    #[getter]
    fn slot_count(&self) -> usize {
        self.model.slot_count()
    }

    /// The replaceable positions, in fill order, each with the length it locks.
    #[getter]
    fn slots(&self) -> Vec<Slot> {
        let rate = self.model.project().timeline().frame_rate;
        self.model
            .slots()
            .iter()
            .filter_map(|clip| {
                let marker = clip.replaceable.as_ref()?;
                let duration = seconds(clip.timeline.duration.value, rate);
                Some(Slot::from_model(marker, Some(duration)))
            })
            .collect()
    }

    /// The editable text lines, in timeline order — what a script overwrites
    /// after filling to put its own words on the author's titles.
    #[getter]
    fn texts(&self) -> Vec<String> {
        self.model
            .editable_texts()
            .iter()
            .filter_map(|clip| match &clip.content {
                aria_models::ClipSource::Generated(Generator::Text { content, .. }) => {
                    Some(content.to_string())
                }
                _ => None,
            })
            .collect()
    }

    /// Whether the template carries a swappable music clip.
    #[getter]
    fn has_music(&self) -> bool {
        self.model.music().is_some()
    }

    /// Fill the slots in order and return a render-ready project.
    ///
    /// Each pick is a `Media` handle or `(media, start_seconds)`. Supplying a
    /// different number of picks than the template has slots is an error rather
    /// than a partial fill, so a script never renders a placeholder by accident.
    fn fill(&self, py: Python, picks: &Bound<'_, PyAny>) -> PyResult<Project> {
        let mut model_picks = Vec::new();
        for item in picks.try_iter()? {
            model_picks.push(Self::pick(py, &item?)?);
        }
        if model_picks.len() != self.model.slot_count() {
            return Err(AriaError::new_err(format!(
                "{} {} {}, got {}",
                counted(self.model.slot_count(), "slot", "slots"),
                noun(self.model.slot_count(), "needs", "need"),
                counted(self.model.slot_count(), "pick", "picks"),
                model_picks.len()
            )));
        }
        let filled = self.model.apply(&model_picks).map_err(model_err)?;
        Ok(Project::from_model(filled))
    }

    /// Write the template as a `.ariat` document.
    pub fn save(&self, path: &str) -> PyResult<()> {
        self.model
            .save_to_file(Path::new(path))
            .map_err(|e| AriaError::new_err(e.to_string()))
    }

    /// Write a distributable `.ariab` bundle: the document plus a copy of every
    /// sample media file, so it opens on a machine that has neither.
    pub fn save_bundle(&self, path: &str) -> PyResult<()> {
        template_bundle::write(&self.model, Path::new(path)).map_err(model_err)
    }

    fn __repr__(&self) -> String {
        format!(
            "Template(name={:?}, slots={}, duration={:.3}s)",
            self.model.meta().name,
            self.model.slot_count(),
            self.duration()
        )
    }
}

/// Build a template from a project snapshot. A template *is* a snapshot: the
/// script keeps editing its project afterwards without touching what was saved.
pub fn from_project(
    project: &aria_models::Project,
    name: &str,
    author: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
) -> Template {
    let mut meta = TemplateMeta::new(name);
    meta.author = author;
    meta.description = description;
    if let Some(tags) = tags {
        meta = meta.with_tags(tags);
    }
    Template::from_model(Model::from_project(project.clone(), meta))
}
