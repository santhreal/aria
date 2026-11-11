//! Python exception types and [`ModelError`] mapping.

use aria_models::{MediaId, ModelError, Project as Model, TrackId};
use pyo3::create_exception;
use pyo3::exceptions::PyException;
use pyo3::prelude::*;

create_exception!(aria, AriaError, PyException);
create_exception!(aria, OverlapError, AriaError);
create_exception!(aria, TrackKindError, AriaError);
create_exception!(aria, MediaError, AriaError);
create_exception!(aria, RenderError, AriaError);

/// Register exception types on the module.
pub fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("AriaError", m.py().get_type::<AriaError>())?;
    m.add("OverlapError", m.py().get_type::<OverlapError>())?;
    m.add("TrackKindError", m.py().get_type::<TrackKindError>())?;
    m.add("MediaError", m.py().get_type::<MediaError>())?;
    m.add("RenderError", m.py().get_type::<RenderError>())?;
    Ok(())
}

/// Map a model error that states no track or media of a project to the
/// appropriate Python exception: a load, a template, an effect lookup.
pub fn model_err(e: ModelError) -> PyErr {
    let message = e.to_string();
    exception_for(&e, message)
}

/// Map an edit `model` rejected to the appropriate Python exception. A script
/// holds no track or media id, so a track reads as its kind, name and place
/// in `tracks`, and a media as its file; every other refusal reads as
/// [`model_err`] states it.
pub fn edit_err(model: &Model) -> impl Fn(ModelError) -> PyErr + '_ {
    move |e| {
        let message = script_words(model, &e).unwrap_or_else(|| e.to_string());
        exception_for(&e, message)
    }
}

fn exception_for(e: &ModelError, message: String) -> PyErr {
    match e {
        ModelError::Overlap(_) => OverlapError::new_err(message),
        ModelError::IncompatibleTrackKind { .. }
        | ModelError::NotOnPicture { .. }
        | ModelError::NotOnSound { .. } => TrackKindError::new_err(message),
        ModelError::UnknownMedia(_)
        | ModelError::MediaReferenced(_)
        | ModelError::SourceOutOfBounds => MediaError::new_err(message),
        _ => AriaError::new_err(message),
    }
}

/// The refusal in a script's words, for each error that states a track or a
/// media by its id. `None` for every other error.
fn script_words(model: &Model, e: &ModelError) -> Option<String> {
    let track = |id| track_words(model, id);
    Some(match *e {
        ModelError::UnknownTrack(_) => "the track is no longer in the project".to_string(),
        ModelError::Overlap(id) => format!("clip overlaps an existing clip on {}", track(id)),
        ModelError::IncompatibleTrackKind { track: id, .. } => {
            format!("{} cannot hold this clip", track(id))
        }
        ModelError::NotOnPicture { track: id, .. } => {
            format!("the clip is on {}, which shows no picture", track(id))
        }
        ModelError::NotOnSound { track: id, .. } => format!(
            "the clip is on {}; only a clip on an audio track takes an audio role",
            track(id)
        ),
        ModelError::UnknownMedia(_) => "the media is no longer in the project".to_string(),
        ModelError::MediaReferenced(id) => format!(
            "{} is still used by one or more clips; delete them first",
            media_words(model, id)
        ),
        _ => return None,
    })
}

/// `the video track "B-roll" at tracks[1]`: the kind as `add_track` spells
/// it, the name when it has one, and its index in `Project.tracks`.
fn track_words(model: &Model, id: TrackId) -> String {
    let timeline = model.timeline();
    let Some((index, track)) = timeline
        .tracks_ordered()
        .enumerate()
        .find(|(_, track)| track.id == id)
    else {
        return "a track no longer in the project".to_string();
    };
    let kind = track.kind.id();
    if track.name.is_empty() {
        format!("the {kind} track at tracks[{index}]")
    } else {
        format!("the {kind} track {:?} at tracks[{index}]", track.name)
    }
}

/// `the media "/footage/a.mp4"`: the file it was imported from.
fn media_words(model: &Model, id: MediaId) -> String {
    match model.media(id) {
        Some(media) => format!("the media {:?}", media.path().display().to_string()),
        None => "a media no longer in the project".to_string(),
    }
}

/// Map any displayable error into [`RenderError`].
pub fn render_err<E: std::fmt::Display>(e: E) -> PyErr {
    RenderError::new_err(e.to_string())
}

/// Map probe / I/O failures into [`MediaError`].
pub fn media_err<E: std::fmt::Display>(e: E) -> PyErr {
    MediaError::new_err(e.to_string())
}
