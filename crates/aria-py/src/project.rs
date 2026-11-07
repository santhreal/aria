//! [`Project`] — the root editing + render handle.

use std::path::{Path, PathBuf};

use aria_decoder::probe;
use aria_models::{
    CanvasAspect, CanvasSettings, MediaSource, Project as Model, Rational, path_error,
};
use aria_render::{Renderer, canvas_size, encoder_capability, export_to_file};
use numpy::ndarray::Array3;
use numpy::{IntoPyArray, PyArray3};
use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;

use crate::convert::{fps_float, parse_color, parse_track_kind, rate_from_fps, time_at};
use crate::errors::{AriaError, media_err, model_err, render_err};
use crate::marker::Marker;
use crate::media::Media;
use crate::template::Template;
use crate::track::Track;

// The Python surface is the `#[pymethods]` block below; the marker and template
// bodies live in the child it names, like `Clip`'s.
#[path = "project_library.rs"]
mod project_library;

/// A scriptable Aria project: editing state plus a lazily created renderer.
#[pyclass(unsendable)]
pub struct Project {
    pub(crate) model: Model,
    renderer: Option<Renderer>,
}

#[pymethods]
impl Project {
    #[new]
    #[pyo3(signature = (name, fps = None, canvas = "auto", background = None))]
    fn new(
        name: &str,
        fps: Option<&Bound<'_, PyAny>>,
        canvas: &str,
        background: Option<&Bound<'_, PyAny>>,
    ) -> PyResult<Self> {
        let rate = match fps {
            Some(fps) => rate_from_fps(fps)?,
            None => Rational::FPS_30,
        };
        let mut model = Model::new(name, rate);
        let aspect = CanvasAspect::from_name(canvas)
            .ok_or_else(|| PyValueError::new_err(format!("unknown aspect {canvas:?}")))?;
        let background = match background {
            Some(color) => rgb_of(parse_color(color)?),
            None => [0, 0, 0],
        };
        model.timeline_mut().set_canvas(CanvasSettings {
            aspect,
            background,
            resolution: None,
        });
        Ok(Self {
            model,
            renderer: None,
        })
    }

    #[staticmethod]
    fn load(path: &str) -> PyResult<Self> {
        let model = Model::load_from_file(Path::new(path)).map_err(model_err)?;
        Ok(Self::from_model(model))
    }

    fn save(&self, path: &str) -> PyResult<()> {
        self.model
            .save_to_file(Path::new(path))
            .map_err(|e| AriaError::new_err(e.to_string()))
    }

    fn import_media(mut slf: PyRefMut<'_, Self>, py: Python, path: &str) -> PyResult<Media> {
        let path_buf = PathBuf::from(path);
        let canonical = path_buf
            .canonicalize()
            .map_err(|e| media_err(path_error(&path_buf, &e)))?;
        // A folder otherwise fails in the decoder as a GStreamer read error.
        if canonical.is_dir() {
            let folder = std::io::Error::from(std::io::ErrorKind::IsADirectory);
            return Err(media_err(path_error(&path_buf, &folder)));
        }
        if let Some(existing) = slf.model.find_media_by_path(&canonical) {
            return Ok(Media::new(slf.into_pyobject(py)?.unbind(), existing));
        }
        let probed = probe(&canonical).map_err(media_err)?;
        if probed.width == 0 && probed.height == 0 && !probed.has_audio {
            return Err(media_err(format!("{path}: no video or audio stream")));
        }
        let media = if probed.is_image {
            MediaSource::image(&canonical, probed.width, probed.height)
        } else {
            MediaSource::new(
                &canonical,
                probed.width,
                probed.height,
                probed.frame_rate.reduced(),
                probed.frame_count,
                probed.has_audio,
            )
        };
        let id = slf.model.add_media(media);
        Ok(Media::new(slf.into_pyobject(py)?.unbind(), id))
    }

    fn remove_media(mut slf: PyRefMut<'_, Self>, py: Python, media: &Media) -> PyResult<()> {
        if media.project().bind(py).as_ptr() != slf.as_ptr() {
            return Err(AriaError::new_err("media belongs to a different project"));
        }
        slf.model.remove_media(media.id()).map_err(model_err)?;
        Ok(())
    }

    #[getter]
    fn media(slf: PyRef<'_, Self>, py: Python) -> PyResult<Vec<Media>> {
        let ids: Vec<_> = slf.model.media_iter().map(|m| m.id).collect();
        let project = slf.into_pyobject(py)?.unbind();
        ids.into_iter()
            .map(|id| Ok(Media::new(project.clone_ref(py), id)))
            .collect()
    }

    #[pyo3(signature = (kind, name = "", index = None))]
    fn add_track(
        mut slf: PyRefMut<'_, Self>,
        py: Python,
        kind: &str,
        name: &str,
        index: Option<usize>,
    ) -> PyResult<Track> {
        let kind = parse_track_kind(kind)?;
        let id = match index {
            Some(idx) => slf.model.insert_track(kind, name, idx),
            None => slf.model.add_track(kind, name),
        };
        Ok(Track::new(slf.into_pyobject(py)?.unbind(), id))
    }

    #[getter]
    fn tracks(slf: PyRef<'_, Self>, py: Python) -> PyResult<Vec<Track>> {
        let ids: Vec<_> = slf
            .model
            .timeline()
            .tracks_ordered()
            .map(|t| t.id)
            .collect();
        let project = slf.into_pyobject(py)?.unbind();
        ids.into_iter()
            .map(|id| Ok(Track::new(project.clone_ref(py), id)))
            .collect()
    }

    fn track(slf: PyRef<'_, Self>, py: Python, name: &str) -> PyResult<Track> {
        let id = {
            let matches: Vec<_> = slf
                .model
                .timeline()
                .tracks_ordered()
                .filter(|t| t.name == name)
                .map(|t| t.id)
                .collect();
            match matches.len() {
                0 => return Err(PyValueError::new_err(format!("no track named {name:?}"))),
                1 => matches[0],
                _ => {
                    return Err(PyValueError::new_err(format!(
                        "multiple tracks named {name:?}"
                    )));
                }
            }
        };
        Ok(Track::new(slf.into_pyobject(py)?.unbind(), id))
    }

    #[getter]
    fn canvas(&self) -> String {
        self.model.timeline().canvas().aspect.name().to_string()
    }

    #[setter]
    fn set_canvas(&mut self, aspect: &str) -> PyResult<()> {
        let aspect = CanvasAspect::from_name(aspect)
            .ok_or_else(|| PyValueError::new_err(format!("unknown aspect {aspect:?}")))?;
        let bg = self.model.timeline().canvas().background;
        self.model.timeline_mut().set_canvas(CanvasSettings {
            aspect,
            background: bg,
            resolution: None,
        });
        Ok(())
    }

    #[getter]
    fn background(&self) -> (u8, u8, u8) {
        let bg = self.model.timeline().canvas().background;
        (bg[0], bg[1], bg[2])
    }

    #[setter]
    fn set_background(&mut self, color: &Bound<'_, PyAny>) -> PyResult<()> {
        let aspect = self.model.timeline().canvas().aspect;
        let background = rgb_of(parse_color(color)?);
        self.model.timeline_mut().set_canvas(CanvasSettings {
            aspect,
            background,
            resolution: None,
        });
        Ok(())
    }

    fn load_font(&mut self, path: &str) -> PyResult<()> {
        let data = std::fs::read(path).map_err(|e| render_err(path_error(Path::new(path), &e)))?;
        self.ensure_renderer()?;
        self.renderer
            .as_mut()
            .expect("renderer initialized")
            .load_font(data);
        Ok(())
    }

    fn get_frame<'py>(&mut self, py: Python<'py>, t: f64) -> PyResult<Bound<'py, PyArray3<u8>>> {
        // Read before a renderer opens, so a bad time fails without a device.
        let time = time_at(t, self.rate())?;
        self.ensure_renderer()?;
        let renderer = self.renderer.as_mut().expect("renderer initialized");
        let model = &self.model;
        let image = py
            .detach(|| renderer.render_frame(model, time))
            .map_err(render_err)?;
        let shape = (image.height as usize, image.width as usize, 4);
        let array = Array3::from_shape_vec(shape, image.pixels)
            .map_err(|e| PyRuntimeError::new_err(format!("frame buffer/shape mismatch: {e}")))?;
        Ok(array.into_pyarray(py))
    }

    fn export(&mut self, py: Python<'_>, path: &str) -> PyResult<u64> {
        // The encoder writes one container whatever the name says, so a name
        // for another one is rejected before a renderer opens.
        let path = Path::new(path);
        let extension = encoder_capability().extension;
        if !path
            .extension()
            .is_some_and(|named| named.eq_ignore_ascii_case(extension))
        {
            return Err(PyValueError::new_err(format!(
                "export writes a .{extension} file; name the output \"{}\" rather than \"{}\"",
                path.with_extension(extension).display(),
                path.display()
            )));
        }
        self.ensure_renderer()?;
        let renderer = self.renderer.as_mut().expect("renderer initialized");
        let model = &self.model;
        py.detach(|| export_to_file(renderer, model, path))
            .map_err(render_err)
    }

    #[getter]
    fn duration(&self) -> f64 {
        self.model.timeline().duration().seconds()
    }

    #[getter]
    fn size(&self) -> (u32, u32) {
        canvas_size(&self.model)
    }

    #[getter]
    fn fps(&self) -> f64 {
        fps_float(self.model.timeline().frame_rate)
    }

    /// Drop a marker on the ruler at `t` seconds. Without a color the palette
    /// cycles, so markers added in a loop stay distinguishable.
    #[pyo3(signature = (t, name = "", color = None, comment = ""))]
    fn add_marker(
        slf: PyRefMut<'_, Self>,
        py: Python,
        t: f64,
        name: &str,
        color: Option<&str>,
        comment: &str,
    ) -> PyResult<Marker> {
        project_library::add_marker(slf, py, t, name, color, comment)
    }

    /// Every marker, in `(time, id)` order.
    #[getter]
    fn markers(slf: PyRef<'_, Self>, py: Python) -> PyResult<Vec<Marker>> {
        project_library::markers(slf, py)
    }

    /// Save the project as a reusable template: a snapshot whose replaceable
    /// clips (see `clip.replaceable`) become slots a caller fills with its own
    /// media. `bundle=True` writes a `.ariab` that carries the sample media too.
    #[pyo3(signature = (path, name, author = None, description = None, tags = None, bundle = false))]
    #[allow(clippy::too_many_arguments)]
    fn save_template(
        &self,
        path: &str,
        name: &str,
        author: Option<String>,
        description: Option<String>,
        tags: Option<Vec<String>>,
        bundle: bool,
    ) -> PyResult<Template> {
        project_library::save_template(self, path, name, author, description, tags, bundle)
    }

    fn __repr__(&self) -> String {
        let (w, h) = canvas_size(&self.model);
        format!(
            "Project(size=({w}, {h}), fps={:.3}, duration={:.3}s)",
            self.fps(),
            self.duration()
        )
    }
}

/// Drop the alpha channel of a parsed color (the canvas is opaque).
fn rgb_of(rgba: [u8; 4]) -> [u8; 3] {
    [rgba[0], rgba[1], rgba[2]]
}

/// The size the probe imports use reads for the video at `path`, upright.
fn upright_size(path: &Path) -> Option<(u32, u32)> {
    probe(path)
        .ok()
        .filter(|probed| !probed.is_image)
        .map(|probed| (probed.width, probed.height))
}

impl Project {
    /// Wrap a model read from a file or filled from a template. The size of
    /// each video a save from before the probe applied the container's
    /// rotation recorded sideways is corrected first. The renderer is created
    /// on first use, so a project that is only inspected never brings up a GPU.
    pub(crate) fn from_model(mut model: Model) -> Self {
        model.orient_media(upright_size);
        Self {
            model,
            renderer: None,
        }
    }

    pub(crate) fn rate(&self) -> Rational {
        self.model.timeline().frame_rate
    }

    pub(crate) fn model(&self) -> &Model {
        &self.model
    }

    pub(crate) fn model_mut(&mut self) -> &mut Model {
        &mut self.model
    }

    /// Run `edit`, which writes in more than one step, and put the model back
    /// as it was when any step fails, so the call fails whole. A call that
    /// writes in one model step needs none of this: the model leaves itself
    /// unchanged on its own failure.
    pub(crate) fn atomic<R>(&mut self, edit: impl FnOnce(&mut Self) -> PyResult<R>) -> PyResult<R> {
        let before = self.model.clone();
        let outcome = edit(self);
        if outcome.is_err() {
            self.model = before;
        }
        outcome
    }

    pub(crate) fn ensure_renderer(&mut self) -> PyResult<()> {
        if self.renderer.is_none() {
            self.renderer = Some(Renderer::new_headless().map_err(render_err)?);
        }
        Ok(())
    }
}
