//! [`Marker`] — a named anchor on the timeline ruler.
//!
//! A handle like every other in this crate: `(Py<Project>, MarkerId)`, resolved
//! on each call, so a marker moved or renamed elsewhere is never read from a
//! stale copy and a deleted one raises instead of returning nonsense.

use aria_models::{MarkerColor, MarkerId};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::convert::{seconds, time_at};
use crate::errors::{AriaError, model_err};
use crate::project::Project;

/// The palette name a script writes, parsed against the enum itself so a colour
/// added to the model is nameable here without a second list to update.
pub fn parse_marker_color(name: &str) -> PyResult<MarkerColor> {
    let lowered = name.to_ascii_lowercase();
    MarkerColor::ALL
        .into_iter()
        .find(|color| color.name() == lowered)
        .ok_or_else(|| {
            let known: Vec<&str> = MarkerColor::ALL.iter().map(|c| c.name()).collect();
            PyValueError::new_err(format!(
                "unknown marker color {name:?}; one of {}",
                known.join(", ")
            ))
        })
}

/// A marker on the timeline ruler.
#[pyclass(unsendable)]
pub struct Marker {
    project: Py<Project>,
    id: MarkerId,
}

impl Marker {
    pub fn new(project: Py<Project>, id: MarkerId) -> Self {
        Self { project, id }
    }

    /// Read one field of the live marker, raising on a handle whose marker was
    /// deleted rather than answering from a copy taken when it was made.
    fn read<F, R>(&self, py: Python, f: F) -> PyResult<R>
    where
        F: FnOnce(&aria_models::Marker, aria_models::Rational) -> R,
    {
        let project = self.project.bind(py).borrow();
        let rate = project.rate();
        let marker = project
            .model()
            .timeline()
            .marker(self.id)
            .ok_or_else(|| AriaError::new_err("stale marker handle"))?;
        Ok(f(marker, rate))
    }

    /// Rewrite the marker, keeping every field the caller did not name. One
    /// model call owns move, rename, recolour and re-annotate, so the four
    /// setters cannot disagree about what the others do. The fields the caller
    /// left alone are re-supplied because `set_marker` re-sorts on the tick and
    /// therefore takes the whole record; only those are copied.
    fn edit(
        &self,
        py: Python,
        tick: Option<f64>,
        name: Option<String>,
        color: Option<MarkerColor>,
        comment: Option<String>,
    ) -> PyResult<()> {
        let mut project = self.project.bind(py).borrow_mut();
        let rate = project.rate();
        let (tick, name, color, comment) = {
            let marker = project
                .model()
                .timeline()
                .marker(self.id)
                .ok_or_else(|| AriaError::new_err("stale marker handle"))?;
            (
                tick.map_or(marker.tick, |seconds| time_at(seconds, rate)),
                name.unwrap_or_else(|| marker.name.to_string()),
                color.unwrap_or(marker.color),
                comment.unwrap_or_else(|| marker.comment.to_string()),
            )
        };
        project
            .model_mut()
            .timeline_mut()
            .set_marker(self.id, tick, name, color, comment)
            .map_err(model_err)
    }
}

#[pymethods]
impl Marker {
    /// Position on the timeline, in seconds.
    #[getter]
    fn time(&self, py: Python) -> PyResult<f64> {
        self.read(py, |marker, rate| seconds(marker.tick.value, rate))
    }

    #[setter]
    fn set_time(&self, py: Python, value: f64) -> PyResult<()> {
        self.edit(py, Some(value), None, None, None)
    }

    /// Short label drawn on the ruler.
    #[getter]
    fn name(&self, py: Python) -> PyResult<String> {
        self.read(py, |marker, _| marker.name.to_string())
    }

    #[setter]
    fn set_name(&self, py: Python, value: String) -> PyResult<()> {
        self.edit(py, None, Some(value), None, None)
    }

    /// Palette name (`"teal"`, `"red"`, …); see `aria.marker_colors()`.
    #[getter]
    fn color(&self, py: Python) -> PyResult<String> {
        self.read(py, |marker, _| marker.color.name().to_string())
    }

    #[setter]
    fn set_color(&self, py: Python, value: &str) -> PyResult<()> {
        let color = parse_marker_color(value)?;
        self.edit(py, None, None, Some(color), None)
    }

    /// The longer note kept with the marker; empty when it carries only a label.
    #[getter]
    fn comment(&self, py: Python) -> PyResult<String> {
        self.read(py, |marker, _| marker.comment.to_string())
    }

    #[setter]
    fn set_comment(&self, py: Python, value: String) -> PyResult<()> {
        self.edit(py, None, None, None, Some(value))
    }

    /// Remove the marker. The handle is stale afterwards and every accessor on
    /// it raises.
    fn delete(&self, py: Python) -> PyResult<()> {
        let mut project = self.project.bind(py).borrow_mut();
        project
            .model_mut()
            .timeline_mut()
            .remove_marker(self.id)
            .ok_or_else(|| AriaError::new_err("stale marker handle"))?;
        Ok(())
    }

    fn __repr__(&self, py: Python) -> PyResult<String> {
        let time = self.time(py)?;
        let name = self.name(py)?;
        let color = self.color(py)?;
        Ok(format!(
            "Marker(t={time:.3}s, name={name:?}, color={color:?})"
        ))
    }
}
