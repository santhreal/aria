//! `Project` ruler markers and template authoring.
//!
//! Declared from `project.rs` with `#[path]`. pyo3 allows one `#[pymethods]`
//! block per class, so that block stays in the parent as the Python surface and
//! these bodies live here; `use super::*` reaches the parent's imports.

use aria_models::{Marker as MarkerModel, MarkerColor};

use super::*;
use crate::marker::{Marker, parse_marker_color};
use crate::template::{self, Template};

pub(super) fn add_marker(
    mut slf: PyRefMut<'_, Project>,
    py: Python,
    t: f64,
    name: &str,
    color: Option<&str>,
    comment: &str,
) -> PyResult<Marker> {
    let rate = slf.rate();
    // An unnamed colour cycles the palette, so consecutive markers a script
    // drops in a loop stay distinguishable on the ruler without the script
    // choosing colours it does not care about.
    let color = match color {
        Some(name) => parse_marker_color(name)?,
        None => MarkerColor::cycle(slf.model().timeline().marker_count()),
    };
    let marker = MarkerModel::new(time_at(t, rate), name, color).with_comment(comment);
    let id = slf
        .model_mut()
        .timeline_mut()
        .add_marker(marker)
        .map_err(model_err)?;
    Ok(Marker::new(slf.into_pyobject(py)?.unbind(), id))
}

pub(super) fn markers(slf: PyRef<'_, Project>, py: Python) -> PyResult<Vec<Marker>> {
    let ids: Vec<_> = slf
        .model()
        .timeline()
        .markers()
        .iter()
        .map(|marker| marker.id)
        .collect();
    let project = slf.into_pyobject(py)?.unbind();
    Ok(ids
        .into_iter()
        .map(|id| Marker::new(project.clone_ref(py), id))
        .collect())
}

pub(super) fn save_template(
    project: &Project,
    path: &str,
    name: &str,
    author: Option<String>,
    description: Option<String>,
    tags: Option<Vec<String>>,
    bundle: bool,
) -> PyResult<Template> {
    let template = template::from_project(project.model(), name, author, description, tags);
    // A bundle carries a copy of every sample file, so it opens on a machine
    // that has none of them; a bare document references what is on this one.
    if bundle {
        template.save_bundle(path)?;
    } else {
        template.save(path)?;
    }
    Ok(template)
}
