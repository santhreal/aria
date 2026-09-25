//! [`Slot`] — a replaceable position in a template, and the marker that makes a
//! clip into one.
//!
//! One type for both directions: reading `clip.replaceable` or `template.slots`
//! gives a `Slot`, and assigning one back marks a clip. A script that lists a
//! template's slots can therefore rebuild them on another timeline without a
//! second vocabulary for the same three fields.

use aria_models::{Replaceable, SlotMedia};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// The media restriction names, derived from the enum so a variant added to the
/// model cannot be silently unnameable here: the match below stops compiling.
pub fn slot_media_name(accepts: SlotMedia) -> &'static str {
    match accepts {
        SlotMedia::Any => "any",
        SlotMedia::VideoOnly => "video",
        SlotMedia::ImageOnly => "image",
        SlotMedia::AudioOnly => "audio",
    }
}

/// Every restriction a script may write, in the order the model declares them.
pub const SLOT_MEDIA: [SlotMedia; 4] = [
    SlotMedia::Any,
    SlotMedia::VideoOnly,
    SlotMedia::ImageOnly,
    SlotMedia::AudioOnly,
];

pub fn parse_slot_media(name: &str) -> PyResult<SlotMedia> {
    let lowered = name.to_ascii_lowercase();
    SLOT_MEDIA
        .into_iter()
        .find(|accepts| slot_media_name(*accepts) == lowered)
        .ok_or_else(|| {
            let known: Vec<&str> = SLOT_MEDIA.iter().map(|a| slot_media_name(*a)).collect();
            PyValueError::new_err(format!(
                "unknown slot media {name:?}; one of {}",
                known.join(", ")
            ))
        })
}

/// A replaceable position: fill order, what media it accepts, and the author's
/// hint. `duration` is set when the slot was read from a template (the locked
/// length the filled media takes) and `None` on one a script just built.
#[pyclass(from_py_object)]
#[derive(Clone)]
pub struct Slot {
    #[pyo3(get)]
    pub order: u32,
    #[pyo3(get)]
    pub accepts: String,
    #[pyo3(get)]
    pub label: Option<String>,
    #[pyo3(get)]
    pub duration: Option<f64>,
}

impl Slot {
    /// The slot a template clip declares, with the length it locks.
    pub fn from_model(marker: &Replaceable, duration: Option<f64>) -> Self {
        Self {
            order: marker.order,
            accepts: slot_media_name(marker.accepts).to_string(),
            label: marker.label.as_ref().map(|l| l.to_string()),
            duration,
        }
    }

    /// The model marker this slot describes. `duration` is not part of it: a
    /// slot's length is the clip's, not something the marker carries.
    pub fn to_model(&self) -> PyResult<Replaceable> {
        Ok(Replaceable {
            order: self.order,
            accepts: parse_slot_media(&self.accepts)?,
            label: self.label.as_ref().map(|l| l.to_string()),
        })
    }
}

#[pymethods]
impl Slot {
    #[new]
    #[pyo3(signature = (order = 0, accepts = "any", label = None))]
    fn new(order: u32, accepts: &str, label: Option<String>) -> PyResult<Self> {
        parse_slot_media(accepts)?;
        Ok(Self {
            order,
            accepts: accepts.to_ascii_lowercase(),
            label,
            duration: None,
        })
    }

    fn __repr__(&self) -> String {
        let label = self.label.as_deref().unwrap_or("");
        match self.duration {
            Some(duration) => format!(
                "Slot(order={}, accepts={:?}, label={label:?}, duration={duration:.3}s)",
                self.order, self.accepts
            ),
            None => format!(
                "Slot(order={}, accepts={:?}, label={label:?})",
                self.order, self.accepts
            ),
        }
    }
}
