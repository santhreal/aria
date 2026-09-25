//! Python bindings for the Aria video engine (v2 API).

mod catalog;
mod clip;
mod content;
mod convert;
mod effect;
mod errors;
mod marker;
mod media;
mod project;
mod slot;
mod template;
mod track;

// Needs an interpreter to link against, which the shipped extension module does
// not have. The crate's default feature provides one, so `cargo test` runs with
// no flags; a wheel build turns it off (see `pyproject.toml`).
#[cfg(all(test, feature = "embedded-python"))]
#[path = "binding_tests.rs"]
mod scripted_editing;

#[cfg(all(test, feature = "embedded-python"))]
#[path = "binding_cost_tests.rs"]
mod binding_cost;

#[cfg(all(test, feature = "embedded-python"))]
#[path = "binding_library_tests.rs"]
mod scripted_library;

use content::{
    Arrow, Ellipse, Heart, Line, Polygon, Rect, ShapeStrokeSpec, Solid, Star, Sticker, Text,
    TextBackground, TextShadow, TextStroke,
};
use pyo3::prelude::*;

use catalog::register as register_catalog;
use clip::Clip;
use effect::Effect;
use errors::register as register_errors;
use marker::Marker;
use media::{Media, MediaSlice};
use project::Project;
use slot::Slot;
use template::Template;
use track::Track;

/// The `aria` Python module.
#[pymodule]
fn aria(m: &Bound<'_, PyModule>) -> PyResult<()> {
    register_errors(m)?;
    register_catalog(m)?;
    m.add_class::<Project>()?;
    m.add_class::<Media>()?;
    m.add_class::<MediaSlice>()?;
    m.add_class::<Track>()?;
    m.add_class::<Clip>()?;
    m.add_class::<Marker>()?;
    m.add_class::<Slot>()?;
    m.add_class::<Template>()?;
    m.add_class::<Effect>()?;
    m.add_class::<Text>()?;
    m.add_class::<TextStroke>()?;
    m.add_class::<TextBackground>()?;
    m.add_class::<TextShadow>()?;
    m.add_class::<Solid>()?;
    m.add_class::<Sticker>()?;
    m.add_class::<ShapeStrokeSpec>()?;
    m.add_class::<Rect>()?;
    m.add_class::<Ellipse>()?;
    m.add_class::<Polygon>()?;
    m.add_class::<Star>()?;
    m.add_class::<Line>()?;
    m.add_class::<Arrow>()?;
    m.add_class::<Heart>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}
