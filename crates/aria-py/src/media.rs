//! [`Media`] pool handles and [`MediaSlice`] content descriptors.

use aria_models::{MediaId, MediaKind, TimeRange};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PySlice;

use crate::convert::{Seconds, fps_float, ticks};
use crate::errors::{AriaError, MediaError, media_words};
use crate::project::Project;

/// A probed source file in the project's media pool.
#[pyclass(unsendable)]
pub struct Media {
    project: Py<Project>,
    id: MediaId,
}

/// A trimmed window into a [`Media`] asset (content descriptor, not a copy).
#[pyclass(skip_from_py_object)]
#[derive(Clone, Copy)]
pub struct MediaSlice {
    pub(crate) media: MediaId,
    pub(crate) start_sec: f64,
    pub(crate) end_sec: Option<f64>,
}

impl Media {
    pub(crate) fn new(project: Py<Project>, id: MediaId) -> Self {
        Self { project, id }
    }

    pub(crate) fn id(&self) -> MediaId {
        self.id
    }

    pub(crate) fn project(&self) -> &Py<Project> {
        &self.project
    }

    pub(crate) fn with_project_ref<F, R>(&self, py: Python, f: F) -> PyResult<R>
    where
        F: FnOnce(&Project) -> PyResult<R>,
    {
        let project = self.project.bind(py).borrow();
        if project.model().media(self.id).is_none() {
            return Err(AriaError::new_err("stale media handle"));
        }
        f(&project)
    }

    pub(crate) fn require(project: &Project, id: MediaId) -> PyResult<&aria_models::MediaSource> {
        project
            .model()
            .media(id)
            .ok_or_else(|| AriaError::new_err("stale media handle"))
    }

    /// The window from `start` to `end` seconds of this media. Both
    /// `subclip` and slicing build a window through it, so an end before the
    /// start reads one way whichever the script wrote.
    fn window(&self, start: f64, end: Option<f64>) -> PyResult<MediaSlice> {
        if let Some(end) = end
            && end < start
        {
            return Err(PyValueError::new_err(format!(
                "the media window ends at {end} s, before its start at {start} s"
            )));
        }
        Ok(MediaSlice {
            media: self.id,
            start_sec: start,
            end_sec: end,
        })
    }
}

#[pymethods]
impl Media {
    #[getter]
    fn path(&self, py: Python) -> PyResult<String> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            Ok(media.path().display().to_string())
        })
    }

    #[getter]
    fn kind(&self, py: Python) -> PyResult<String> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            Ok(match media.kind() {
                MediaKind::Video => "video",
                MediaKind::Audio => "audio",
                MediaKind::Image => "image",
            }
            .to_string())
        })
    }

    #[getter]
    fn duration(&self, py: Python) -> PyResult<f64> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            Ok(media.duration.seconds())
        })
    }

    #[getter]
    fn size(&self, py: Python) -> PyResult<(u32, u32)> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            Ok((media.width, media.height))
        })
    }

    #[getter]
    fn fps(&self, py: Python) -> PyResult<f64> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            Ok(fps_float(media.frame_rate))
        })
    }

    #[getter]
    fn has_audio(&self, py: Python) -> PyResult<bool> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            Ok(media.has_audio)
        })
    }

    /// The part of the media from `start` to `end` seconds, or to its end.
    /// A bound that is not a finite time into the media, or an end before the
    /// start, is a `ValueError` here rather than at `Track.add`.
    #[pyo3(signature = (start, end = None))]
    fn subclip(&self, py: Python, start: f64, end: Option<f64>) -> PyResult<MediaSlice> {
        let rate =
            self.with_project_ref(
                py,
                |project| Ok(Self::require(project, self.id)?.frame_rate),
            )?;
        ticks(start, rate, Seconds::Media)?;
        if let Some(end) = end {
            ticks(end, rate, Seconds::Media)?;
        }
        self.window(start, end)
    }

    /// `m[3.0:8.0]` — slice sugar for [`Media::subclip`], in seconds.
    /// Negative bounds count back from the media's end.
    fn __getitem__(&self, py: Python, slice: &Bound<'_, PySlice>) -> PyResult<MediaSlice> {
        if !slice.getattr("step")?.is_none() {
            return Err(PyValueError::new_err("media slices do not support a step"));
        }
        let duration = self.duration(py)?;
        let resolve = |bound: Bound<'_, PyAny>, default: f64| -> PyResult<f64> {
            if bound.is_none() {
                return Ok(default);
            }
            let v: f64 = bound.extract().map_err(|_| {
                PyValueError::new_err("media slice bounds must be numbers of seconds")
            })?;
            if !v.is_finite() {
                return Err(PyValueError::new_err("media slice bounds must be finite"));
            }
            // A negative bound counts back from the end and stops at the start,
            // as a Python sequence slice does.
            let resolved = if v < 0.0 { (duration + v).max(0.0) } else { v };
            Ok(resolved)
        };
        let start = resolve(slice.getattr("start")?, 0.0)?;
        let stop = resolve(slice.getattr("stop")?, duration)?;
        self.window(start, Some(stop))
    }

    fn __repr__(&self, py: Python) -> PyResult<String> {
        self.with_project_ref(py, |project| {
            let media = Self::require(project, self.id)?;
            let name = media
                .path()
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("?");
            let kind = match media.kind() {
                MediaKind::Video => "video",
                MediaKind::Audio => "audio",
                MediaKind::Image => "image",
            };
            Ok(format!(
                "Media({kind} {:.3}s {}x{} @{:.3} '{name}')",
                media.duration.seconds(),
                media.width,
                media.height,
                media.frame_rate.as_f64()
            ))
        })
    }
}

#[pymethods]
impl MediaSlice {
    fn __repr__(&self) -> String {
        match self.end_sec {
            Some(end) => format!("MediaSlice({:.3}, {:.3})", self.start_sec, end),
            None => format!("MediaSlice({:.3}, end)", self.start_sec),
        }
    }
}

/// Resolve a slice's source range at placement time.
pub(crate) fn slice_source_range(
    project: &Project,
    slice: &MediaSlice,
    duration_override: Option<f64>,
) -> PyResult<TimeRange> {
    let media = Media::require(project, slice.media)?;
    let rate = media.frame_rate;
    let start = ticks(slice.start_sec, rate, Seconds::Media)?;
    let (end_tick, end_sec) = if let Some(dur) = duration_override {
        (
            start + ticks(dur, rate, Seconds::Length("duration"))?.max(1),
            slice.start_sec + dur,
        )
    } else {
        match slice.end_sec {
            Some(end) => (ticks(end, rate, Seconds::Media)?, end),
            None => (media.duration.value, media.duration.seconds()),
        }
    };
    let length = media.duration.seconds();
    let named = || media_words(project.model(), slice.media);
    // A window is built with its end at or after its start, so an empty one
    // here is shorter than a frame or starts past the media.
    if end_tick <= start {
        return Err(match slice.end_sec {
            Some(end) => PyValueError::new_err(format!(
                "the media window from {} s to {end} s is shorter than one frame",
                slice.start_sec
            )),
            None => MediaError::new_err(format!(
                "the media window starts at {} s, and {} ends at {length} s",
                slice.start_sec,
                named()
            )),
        });
    }
    if end_tick > media.duration.value && !media.is_image {
        return Err(MediaError::new_err(format!(
            "the media window ends at {end_sec} s, past the end of {} at {length} s",
            named()
        )));
    }
    Ok(TimeRange::at_rate(start, (end_tick - start).max(1), rate))
}

/// Full source range for a media asset.
pub(crate) fn full_source_range(project: &Project, id: MediaId) -> PyResult<TimeRange> {
    let media = Media::require(project, id)?;
    Ok(media.full_range())
}
